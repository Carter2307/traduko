//! The Marian translation network: a Transformer encoder and decoder.
//!
//! Adapted from `candle-transformers` 0.11.0, `src/models/marian.rs`
//! (huggingface/candle, MIT OR Apache-2.0), with what the prototype measured
//! to matter on a CPU:
//!
//! - the decoder reads one new token per call and keeps what it computed for
//!   the earlier ones, so a step costs the same at token 5 and at token 50;
//! - the encoder output is projected once per sentence for the decoder's
//!   attention over it, not at every step;
//! - the kept state can be reordered, which lets all the beams of a search
//!   go through the decoder in one batch;
//! - everything is computed in 16-bit floats, as `weights` explains. The
//!   scores of the next token leave as 32-bit floats for the search.

use candle_core::{DType, Device, Tensor};
use candle_nn::{Activation, LayerNorm, Linear};
use serde::Deserialize;

use crate::failure::{Failure, Result};
use crate::search::Steps;
use crate::weights::Weights;

pub(crate) const CONFIG_FILE: &str = "config.json";

/// The part of `config.json` that the network needs.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Config {
    pub vocab_size: usize,
    pub d_model: usize,
    pub max_position_embeddings: usize,
    pub encoder_layers: usize,
    pub encoder_attention_heads: usize,
    pub encoder_ffn_dim: usize,
    pub decoder_layers: usize,
    pub decoder_attention_heads: usize,
    pub decoder_ffn_dim: usize,
    pub activation_function: Activation,
    pub scale_embedding: bool,
    pub eos_token_id: u32,
    pub pad_token_id: u32,
    pub decoder_start_token_id: u32,
}

impl Config {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|error| Failure::damaged(CONFIG_FILE, error))?;
        let config: Self = serde_json::from_slice(&bytes).map_err(|error| Failure::damaged(CONFIG_FILE, error))?;
        config.check().map_err(|why| Failure::damaged(CONFIG_FILE, why))?;
        Ok(config)
    }

    /// Refuses sizes that no Marian model has, so that the arithmetic on
    /// them later can neither overflow nor divide by zero.
    fn check(&self) -> std::result::Result<(), &'static str> {
        let sizes = [
            (self.vocab_size, 1 << 20),
            (self.d_model, 1 << 13),
            (self.max_position_embeddings, 1 << 13),
            (self.encoder_layers, 64),
            (self.decoder_layers, 64),
            (self.encoder_attention_heads, 256),
            (self.decoder_attention_heads, 256),
            (self.encoder_ffn_dim, 1 << 16),
            (self.decoder_ffn_dim, 1 << 16),
        ];
        if sizes.iter().any(|(size, largest)| *size == 0 || size > largest) {
            return Err("a size is out of range");
        }
        let heads = [self.encoder_attention_heads, self.decoder_attention_heads];
        if heads.iter().any(|heads| !self.d_model.is_multiple_of(*heads)) {
            return Err("the width is not a multiple of the number of heads");
        }
        let tokens = [self.eos_token_id, self.pad_token_id, self.decoder_start_token_id];
        if tokens.iter().any(|token| *token as usize >= self.vocab_size) {
            return Err("a special token is outside the vocabulary");
        }
        Ok(())
    }
}

type Step<T> = candle_core::Result<T>;

/// The four projections of one attention block.
struct Attention {
    query: Linear,
    key: Linear,
    value: Linear,
    out: Linear,
    heads: usize,
    head_width: usize,
    /// Queries are scaled by `1 / sqrt(head_width)` before the product.
    scale: f64,
}

impl Attention {
    fn load(weights: &mut Weights, prefix: &str, width: usize, heads: usize) -> Result<Self> {
        let head_width = width / heads;
        Ok(Self {
            query: linear(weights, &format!("{prefix}.q_proj"), width, width)?,
            key: linear(weights, &format!("{prefix}.k_proj"), width, width)?,
            value: linear(weights, &format!("{prefix}.v_proj"), width, width)?,
            out: linear(weights, &format!("{prefix}.out_proj"), width, width)?,
            heads,
            head_width,
            scale: (head_width as f64).powf(-0.5),
        })
    }

    fn queries(&self, xs: &Tensor) -> Step<Tensor> {
        xs.apply(&self.query)? * self.scale
    }

    /// `(batch, len, width)` to `(batch, heads, len, head_width)`.
    fn split_heads(&self, xs: &Tensor) -> Step<Tensor> {
        let (batch, len, _) = xs.dims3()?;
        xs.reshape((batch, len, self.heads, self.head_width))?.transpose(1, 2)?.contiguous()
    }

    /// Attention of `queries` over `keys` and `values`, all split in heads:
    /// `(batch, query_len, width)`.
    fn attend(&self, queries: &Tensor, keys: &Tensor, values: &Tensor) -> Step<Tensor> {
        let (batch, heads, query_len, head_width) = queries.dims4()?;
        let key_len = keys.dim(2)?;
        let queries = queries.reshape((batch * heads, query_len, head_width))?;
        let keys = keys.reshape((batch * heads, key_len, head_width))?;
        let values = values.reshape((batch * heads, key_len, head_width))?;
        let weights = candle_nn::ops::softmax_last_dim(&queries.matmul(&keys.transpose(1, 2)?)?)?;
        weights
            .matmul(&values)?
            .reshape((batch, heads, query_len, head_width))?
            .transpose(1, 2)?
            .reshape((batch, query_len, heads * head_width))?
            .apply(&self.out)
    }
}

/// The two linear layers that follow attention in every block.
struct FeedForward {
    expand: Linear,
    shrink: Linear,
    activation: Activation,
    norm: LayerNorm,
}

impl FeedForward {
    fn load(weights: &mut Weights, prefix: &str, width: usize, inner: usize, activation: Activation) -> Result<Self> {
        Ok(Self {
            expand: linear(weights, &format!("{prefix}.fc1"), width, inner)?,
            shrink: linear(weights, &format!("{prefix}.fc2"), inner, width)?,
            activation,
            norm: layer_norm(weights, &format!("{prefix}.final_layer_norm"), width)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Step<Tensor> {
        let ys = xs.apply(&self.expand)?.apply(&self.activation)?.apply(&self.shrink)?;
        (ys + xs)?.apply(&self.norm)
    }
}

struct EncoderLayer {
    attention: Attention,
    attention_norm: LayerNorm,
    feed_forward: FeedForward,
}

impl EncoderLayer {
    fn load(weights: &mut Weights, prefix: &str, config: &Config) -> Result<Self> {
        let width = config.d_model;
        Ok(Self {
            attention: Attention::load(weights, &format!("{prefix}.self_attn"), width, config.encoder_attention_heads)?,
            attention_norm: layer_norm(weights, &format!("{prefix}.self_attn_layer_norm"), width)?,
            feed_forward: FeedForward::load(weights, prefix, width, config.encoder_ffn_dim, config.activation_function)?,
        })
    }

    /// One sentence at a time, so there is no padding and no mask.
    fn forward(&self, xs: &Tensor) -> Step<Tensor> {
        let attention = &self.attention;
        let queries = attention.split_heads(&attention.queries(xs)?)?;
        let keys = attention.split_heads(&xs.apply(&attention.key)?)?;
        let values = attention.split_heads(&xs.apply(&attention.value)?)?;
        let xs = (attention.attend(&queries, &keys, &values)? + xs)?.apply(&self.attention_norm)?;
        self.feed_forward.forward(&xs)
    }
}

struct DecoderLayer {
    attention: Attention,
    attention_norm: LayerNorm,
    source_attention: Attention,
    source_attention_norm: LayerNorm,
    feed_forward: FeedForward,
    /// Keys and values of the tokens decoded so far:
    /// `(beams, heads, len, head_width)`.
    past: Option<(Tensor, Tensor)>,
    /// Keys `(heads, head_width, source_len)` and values
    /// `(heads, source_len, head_width)` of the source sentence, shared by
    /// all the beams.
    source: Option<(Tensor, Tensor)>,
}

impl DecoderLayer {
    fn load(weights: &mut Weights, prefix: &str, config: &Config) -> Result<Self> {
        let (width, heads) = (config.d_model, config.decoder_attention_heads);
        Ok(Self {
            attention: Attention::load(weights, &format!("{prefix}.self_attn"), width, heads)?,
            attention_norm: layer_norm(weights, &format!("{prefix}.self_attn_layer_norm"), width)?,
            source_attention: Attention::load(weights, &format!("{prefix}.encoder_attn"), width, heads)?,
            source_attention_norm: layer_norm(weights, &format!("{prefix}.encoder_attn_layer_norm"), width)?,
            feed_forward: FeedForward::load(weights, prefix, width, config.decoder_ffn_dim, config.activation_function)?,
            past: None,
            source: None,
        })
    }

    /// Projects the encoder output `(1, source_len, width)` for this layer.
    fn read_source(&mut self, encoded: &Tensor) -> Step<()> {
        let attention = &self.source_attention;
        let keys = attention.split_heads(&encoded.apply(&attention.key)?)?.squeeze(0)?;
        let values = attention.split_heads(&encoded.apply(&attention.value)?)?.squeeze(0)?;
        self.source = Some((keys.transpose(1, 2)?.contiguous()?, values));
        self.past = None;
        Ok(())
    }

    /// `xs` is `(beams, 1, width)`: the newest token of every beam.
    fn step(&mut self, xs: &Tensor) -> Step<Tensor> {
        let (beams, _, width) = xs.dims3()?;

        // Over the tokens so far. The newest token may look at all of them,
        // so no mask is needed.
        let attention = &self.attention;
        let queries = attention.split_heads(&attention.queries(xs)?)?;
        let new_keys = attention.split_heads(&xs.apply(&attention.key)?)?;
        let new_values = attention.split_heads(&xs.apply(&attention.value)?)?;
        let (keys, values) = match &self.past {
            None => (new_keys, new_values),
            Some((keys, values)) => (Tensor::cat(&[keys, &new_keys], 2)?, Tensor::cat(&[values, &new_values], 2)?),
        };
        let attended = attention.attend(&queries, &keys, &values)?;
        self.past = Some((keys, values));
        let xs = (attended + xs)?.apply(&self.attention_norm)?;

        // Over the source sentence. The beams take the place of the sequence
        // axis: `(heads, beams, head_width)` against keys that have no beam
        // axis is a plain batched product, with no copy per beam.
        let attention = &self.source_attention;
        let Some((keys, values)) = &self.source else {
            candle_core::bail!("the decoder was stepped before it was given a source sentence")
        };
        let queries = attention
            .queries(&xs)?
            .reshape((beams, attention.heads, attention.head_width))?
            .transpose(0, 1)?
            .contiguous()?;
        let weights = candle_nn::ops::softmax_last_dim(&queries.matmul(keys)?)?;
        let attended = weights.matmul(values)?.transpose(0, 1)?.contiguous()?.reshape((beams, 1, width))?.apply(&attention.out)?;
        let xs = (xs + attended)?.apply(&self.source_attention_norm)?;

        self.feed_forward.forward(&xs)
    }

    fn reorder(&mut self, parents: &Tensor) -> Step<()> {
        if let Some((keys, values)) = &self.past {
            self.past = Some((keys.index_select(parents, 0)?, values.index_select(parents, 0)?));
        }
        Ok(())
    }
}

pub(crate) struct Marian {
    /// One row per vocabulary entry. Marian uses the same table to read
    /// tokens in and, transposed, to score the next token.
    embeddings: Tensor,
    next_token: Linear,
    /// Added to the scores of the next token, one value per vocabulary
    /// entry. Kept in 32-bit floats like the scores it is added to.
    next_token_bias: Vec<f32>,
    positions: Tensor,
    embedding_scale: f64,
    encoder: Vec<EncoderLayer>,
    decoder: Vec<DecoderLayer>,
}

impl Marian {
    pub fn load(config: &Config, weights: &mut Weights) -> Result<Self> {
        let (vocab, width) = (config.vocab_size, config.d_model);
        // Some conversions store the shared table under a decoder name only.
        let names = ["model.shared.weight", "model.decoder.embed_tokens.weight", "model.encoder.embed_tokens.weight"];
        let embeddings = weights.first_of(&names, &[vocab, width])?;
        let encoder = (0..config.encoder_layers)
            .map(|layer| EncoderLayer::load(weights, &format!("model.encoder.layers.{layer}"), config))
            .collect::<Result<Vec<_>>>()?;
        let decoder = (0..config.decoder_layers)
            .map(|layer| DecoderLayer::load(weights, &format!("model.decoder.layers.{layer}"), config))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            next_token: Linear::new(embeddings.clone(), None),
            next_token_bias: weights.tensor("final_logits_bias", &[1, vocab])?.to_dtype(DType::F32)?.flatten_all()?.to_vec1()?,
            embeddings,
            positions: sinusoidal_positions(config.max_position_embeddings, width)?,
            embedding_scale: if config.scale_embedding { (width as f64).sqrt() } else { 1.0 },
            encoder,
            decoder,
        })
    }

    /// Reads a source sentence. After this, [`Steps::step`] decodes its
    /// translation from position 0.
    pub fn read(&mut self, source: &[u32]) -> Step<()> {
        let mut xs = self.embed(source, (1, source.len()), 0)?;
        for layer in &self.encoder {
            xs = layer.forward(&xs)?;
        }
        for layer in &mut self.decoder {
            layer.read_source(&xs)?;
        }
        Ok(())
    }

    /// Token ids to `(batch, len, width)` vectors, the first one at
    /// `position`.
    fn embed(&self, ids: &[u32], (batch, len): (usize, usize), position: usize) -> Step<Tensor> {
        let ids = Tensor::new(ids, &Device::Cpu)?;
        let xs = (self.embeddings.index_select(&ids, 0)? * self.embedding_scale)?.reshape((batch, len, ()))?;
        xs.broadcast_add(&self.positions.narrow(0, position, len)?)
    }
}

impl Steps for Marian {
    fn step(&mut self, last: &[u32], position: usize) -> Step<Vec<f32>> {
        let mut xs = self.embed(last, (last.len(), 1), position)?;
        for layer in &mut self.decoder {
            xs = layer.step(&xs)?;
        }
        // One pass over the whole table per token: this product is where
        // most of the decoding time goes.
        let scores = xs.squeeze(1)?.apply(&self.next_token)?.to_dtype(DType::F32)?;
        let mut scores: Vec<f32> = scores.flatten_all()?.to_vec1()?;
        for beam in scores.chunks_exact_mut(self.next_token_bias.len()) {
            beam.iter_mut().zip(&self.next_token_bias).for_each(|(score, bias)| *score += bias);
        }
        Ok(scores)
    }

    fn reorder(&mut self, parents: &[u32]) -> Step<()> {
        let parents = Tensor::new(parents, &Device::Cpu)?;
        self.decoder.iter_mut().try_for_each(|layer| layer.reorder(&parents))
    }
}

/// Marian's fixed position table: sines in the first half of each row,
/// cosines in the second. The Hub's files store it too, but it is cheaper to
/// compute than to read.
fn sinusoidal_positions(count: usize, width: usize) -> Step<Tensor> {
    let half = width / 2;
    let mut table = vec![0f32; count * width];
    for (position, row) in table.chunks_exact_mut(width).enumerate() {
        for column in 0..half {
            // In f64 like the reference implementation, then rounded.
            let angle = position as f64 / 10000f64.powf(2.0 * column as f64 / width as f64);
            row[column] = angle.sin() as f32;
            row[half + column] = angle.cos() as f32;
        }
    }
    Tensor::from_vec(table, (count, width), &Device::Cpu)?.to_dtype(DType::F16)
}

fn linear(weights: &mut Weights, prefix: &str, input: usize, output: usize) -> Result<Linear> {
    let weight = weights.tensor(&format!("{prefix}.weight"), &[output, input])?;
    let bias = weights.tensor(&format!("{prefix}.bias"), &[output])?;
    Ok(Linear::new(weight, Some(bias)))
}

fn layer_norm(weights: &mut Weights, prefix: &str, width: usize) -> Result<LayerNorm> {
    let weight = weights.tensor(&format!("{prefix}.weight"), &[width])?;
    let bias = weights.tensor(&format!("{prefix}.bias"), &[width])?;
    Ok(LayerNorm::new(weight, bias, 1e-5))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> std::result::Result<Config, String> {
        let config: Config = serde_json::from_str(json).map_err(|error| error.to_string())?;
        config.check().map_err(str::to_string)?;
        Ok(config)
    }

    const BASE: &str = r#"{"vocab_size": 59514, "d_model": 512, "max_position_embeddings": 512,
        "encoder_layers": 6, "encoder_attention_heads": 8, "encoder_ffn_dim": 2048,
        "decoder_layers": 6, "decoder_attention_heads": 8, "decoder_ffn_dim": 2048,
        "activation_function": "swish", "scale_embedding": true,
        "eos_token_id": 0, "pad_token_id": 59513, "decoder_start_token_id": 59513,
        "model_type": "marian", "num_beams": 4}"#;

    #[test]
    fn the_configuration_of_the_light_model_is_accepted() {
        let config = config(BASE).expect("a valid configuration");
        assert_eq!((config.d_model, config.pad_token_id), (512, 59513));
    }

    #[test]
    fn sizes_that_would_break_the_arithmetic_are_refused() {
        assert!(config(&BASE.replace("\"encoder_attention_heads\": 8", "\"encoder_attention_heads\": 0")).is_err());
        assert!(config(&BASE.replace("\"decoder_attention_heads\": 8", "\"decoder_attention_heads\": 7")).is_err());
        assert!(config(&BASE.replace("\"vocab_size\": 59514", "\"vocab_size\": 99999999999")).is_err());
        assert!(config(&BASE.replace("\"eos_token_id\": 0", "\"eos_token_id\": 59514")).is_err());
    }

    /// The whole model depends on this. It fails when candle is built with
    /// its `accelerate` feature, which has no 16-bit product.
    #[test]
    fn candle_multiplies_matrices_of_16_bit_floats() {
        let ones = |rows, columns| Tensor::ones((rows, columns), DType::F16, &Device::Cpu);
        let product = ones(2, 3).and_then(|a| a.matmul(&ones(3, 4)?)).and_then(|c| c.to_dtype(DType::F32)?.to_vec2::<f32>());
        assert_eq!(product.expect("a 16-bit matrix product"), [[3.0; 4]; 2]);
    }

    #[test]
    fn the_position_table_has_sines_then_cosines() {
        let table = sinusoidal_positions(4, 8).and_then(|t| t.to_dtype(DType::F32)?.to_vec2::<f32>()).expect("a table");
        assert_eq!(table[0], [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert!((table[1][0] - 1f32.sin()).abs() < 1e-3);
        assert!((table[1][4] - 1f32.cos()).abs() < 1e-3);
    }
}
