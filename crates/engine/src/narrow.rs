//! Rewrites the weights of a Marian model at half the size.
//!
//! The Hub has the small opus-mt models in 32-bit floats. Marian weights are
//! small numbers that fit 16 bits well, the large models are published that
//! way, and 16 bits is the form the engine computes in: so the small ones
//! are installed in 16 bits too.
//!
//! Three things the engine does not read are left out: the position tables
//! (it computes them) and the copies of the embedding table that some
//! conversions store under a second and a third name.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;

use candle_core::{DType, Device, Tensor};

/// The name the engine looks for first.
const EMBEDDINGS: &str = "model.shared.weight";

/// Writes the tensors of `input`, a `model.safetensors`, to `output` in
/// 16-bit floats, and gives their number. The same input gives the same
/// bytes on every run. The error is a sentence for the user.
pub fn to_f16(input: &Path, output: &Path) -> Result<usize, String> {
    let failed = |error: &dyn std::fmt::Display| {
        // candle adds a backtrace: the first line says what happened.
        let text = error.to_string();
        format!("The model could not be rewritten in 16 bits: {}.", text.lines().next().unwrap_or("unknown error"))
    };

    let tensors = candle_core::safetensors::load(input, &Device::Cpu).map_err(|error| failed(&error))?;
    let mut halves: HashMap<String, Tensor> = HashMap::new();
    for (name, tensor) in tensors {
        let name = match name.as_str() {
            "model.encoder.embed_tokens.weight" | "model.decoder.embed_tokens.weight" | "lm_head.weight" => EMBEDDINGS.to_string(),
            position_table if position_table.ends_with("embed_positions.weight") => continue,
            _ => name,
        };
        // The three names of the embedding table are the same tensor.
        if let Entry::Vacant(place) = halves.entry(name) {
            place.insert(tensor.to_dtype(DType::F16).map_err(|error| failed(&error))?);
        }
    }
    if !halves.contains_key(EMBEDDINGS) {
        return Err(failed(&"it has no embedding table, so it is not a Marian model"));
    }

    // Written next to the destination, then renamed: a run that is stopped
    // half-way never leaves a short file under the final name.
    let mut unfinished = output.as_os_str().to_owned();
    unfinished.push(".part");
    candle_core::safetensors::save(&halves, &unfinished).map_err(|error| failed(&error))?;
    std::fs::rename(&unfinished, output).map_err(|error| failed(&error))?;
    Ok(halves.len())
}
