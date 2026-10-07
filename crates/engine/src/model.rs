//! One translation direction in memory: a tokenizer, the network, and the
//! rules for turning one sentence into another.

use std::path::Path;

use candle_core::Device;

use crate::failure::Result;
use crate::marian::{CONFIG_FILE, Config, Marian};
use crate::search::Search;
use crate::spm::{Tokenizer, Vocab};
use crate::weights::{FILE as WEIGHTS_FILE, Weights};
use crate::{failure::Failure, spm};

/// How many translations the search keeps side by side: one, which is greedy
/// decoding. Four is what the reference scores of these models use; measured
/// here it takes three times as long, and on the shared test sentences it
/// made about as many translations worse as better, with either model.
const BEAMS: usize = 1;

/// A sentence with more tokens than this is translated in two halves. The
/// models were trained on single sentences and lose the thread on very long
/// ones well before their hard limit of 512 positions; halves also keep one
/// unit of work short, and a request can only be dropped between units.
const MAX_SOURCE_TOKENS: usize = 256;

pub(crate) struct Model {
    tokenizer: Tokenizer,
    vocab: Vocab,
    network: Marian,
    search: Search,
    /// How many positions the network has, for source and for output.
    positions: usize,
}

impl Model {
    /// Reads a model folder.
    pub fn load(dir: &Path) -> Result<Self> {
        let config = Config::read(&dir.join(CONFIG_FILE))?;
        let (tokenizer, vocab) = spm::load(dir)?;
        if vocab.len() != config.vocab_size {
            return Err(Failure::damaged(spm::VOCAB_FILE, "it does not have the size the configuration says"));
        }
        let network = Marian::load(&config, &mut Weights::open(&dir.join(WEIGHTS_FILE))?)?;
        let search = Search {
            beams: BEAMS,
            max_tokens: 0,
            start: config.decoder_start_token_id,
            end: config.eos_token_id,
            // Marian starts decoding from the padding token and must not
            // produce it (`bad_words_ids` in the model's configuration).
            never: config.pad_token_id,
        };
        Ok(Self { tokenizer, vocab, network, search, positions: config.max_position_embeddings })
    }

    /// Translates one sentence, without leading or trailing blanks.
    ///
    /// `wanted` is asked before each output token. When it says no the work
    /// stops and the result is `None`: a request that was replaced frees the
    /// worker in milliseconds, not at the end of its sentence.
    pub fn translate(&mut self, sentence: &str, wanted: &mut (dyn FnMut() -> bool + Send)) -> Result<Option<String>> {
        let encoded = self.tokenizer.encode(sentence);
        if encoded.ids.is_empty() {
            return Ok(Some(String::new()));
        }
        // One more position for the end token.
        if encoded.ids.len() >= MAX_SOURCE_TOKENS.min(self.positions) {
            // One endless word is not language: it is copied as it is.
            let Some((left, right)) = cut_in_two(sentence) else { return Ok(Some(sentence.to_string())) };
            let Some(left) = self.translate(left, wanted)? else { return Ok(None) };
            let Some(right) = self.translate(right, wanted)? else { return Ok(None) };
            return Ok(Some(format!("{left} {right}")));
        }

        let mut source = encoded.ids;
        source.push(self.search.end);
        // A translation is seldom twice as long as its source; the allowance
        // is for the short ones.
        let max_tokens = (source.len() * 2 + 16).min(self.positions - 1);
        let search = Search { max_tokens, ..self.search };
        let network = &mut self.network;
        // A sentence is thousands of small matrix products. Inside candle's
        // thread pool its workers stay awake from one product to the next,
        // which the prototype and this crate both measured as 10 to 20%
        // faster than waking them up each time.
        let tokens = Device::Cpu.with_context(|| {
            network.read(&source)?;
            search.run(network, wanted)
        })?;
        let Some(tokens) = tokens else { return Ok(None) };
        let translation = self.vocab.decode(&tokens, &[self.search.end, self.search.never], &encoded.unknown);
        Ok(Some(with_apostrophes_of(sentence, translation)))
    }
}

/// The large models write a typographic apostrophe in some sentences and a
/// typewriter one in others, whatever the source has. A text that mixes both
/// looks careless, so a source without typographic apostrophes gets none.
fn with_apostrophes_of(source: &str, translation: String) -> String {
    const TYPOGRAPHIC: char = '\u{2019}';
    if source.contains(TYPOGRAPHIC) || !translation.contains(TYPOGRAPHIC) {
        return translation;
    }
    translation.replace(TYPOGRAPHIC, "'")
}

/// Cuts a sentence in two near its middle: at punctuation when there is some
/// close enough, else at a space.
fn cut_in_two(sentence: &str) -> Option<(&str, &str)> {
    let middle = sentence.len() / 2;
    let candidates = sentence.char_indices().filter_map(|(at, c)| {
        let after = at + c.len_utf8();
        let distance = after.abs_diff(middle);
        let rank = match c {
            ',' | ';' | ':' if distance <= sentence.len() / 4 => 0,
            ' ' => 1,
            ',' | ';' | ':' => 2,
            _ => return None,
        };
        Some((rank, distance, after))
    });
    let (_, _, at) = candidates.min()?;
    let (left, right) = (sentence[..at].trim(), sentence[at..].trim());
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_sentence_is_cut_at_the_comma_nearest_to_its_middle() {
        let sentence = "one two three, four five six seven, eight nine ten";
        assert_eq!(cut_in_two(sentence), Some(("one two three, four five six seven,", "eight nine ten")));
    }

    #[test]
    fn without_punctuation_near_the_middle_the_cut_is_at_a_space() {
        assert_eq!(cut_in_two("a, bbbbbbbb cccccccc dddddddd"), Some(("a, bbbbbbbb", "cccccccc dddddddd")));
        assert_eq!(cut_in_two("éééé èèèè"), Some(("éééé", "èèèè")));
    }

    #[test]
    fn the_translation_takes_the_apostrophes_of_the_source() {
        let typewriter = with_apostrophes_of("Je ne sais pas, c'est loin.", "I don\u{2019}t know, it\u{2019}s far.".to_string());
        assert_eq!(typewriter, "I don't know, it's far.");
        let typographic = with_apostrophes_of("C\u{2019}est loin.", "It\u{2019}s far.".to_string());
        assert_eq!(typographic, "It\u{2019}s far.");
    }

    #[test]
    fn one_endless_word_cannot_be_cut() {
        assert_eq!(cut_in_two(&"x".repeat(5000)), None);
        assert_eq!(cut_in_two(" x"), None);
        assert_eq!(cut_in_two(""), None);
    }
}
