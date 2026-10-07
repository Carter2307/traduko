//! The tokenizer of a Marian model, read from the files that ship with it.
//!
//! `source.spm` is a SentencePiece model (a protobuf message) and
//! `vocab.json` gives each piece the row it has in the weights. This module
//! does what the SentencePiece library does with them: clean the text with
//! the rule table stored in the model, then pick the most likely way to cut
//! it into pieces. Doing it here keeps the C++ library and the conversion to
//! a `tokenizer.json` out of the build.

use std::collections::HashMap;
use std::path::Path;

use crate::failure::{Failure, Result};

pub(crate) const SOURCE_FILE: &str = "source.spm";
pub(crate) const VOCAB_FILE: &str = "vocab.json";

/// SentencePiece writes a space as this character, so a piece can say that it
/// starts a word.
const SPACE_MARK: char = '\u{2581}';

/// How much worse than the rarest piece a character without any piece is.
/// The value is SentencePiece's.
const UNKNOWN_PENALTY: f32 = 10.0;

/// No Marian vocabulary comes close to this; a larger id means a broken file.
const LARGEST_VOCAB: u32 = 1 << 20;

/// `ModelProto.SentencePiece.Type`: a piece that can appear in a text.
const NORMAL_PIECE: u64 = 1;
/// `TrainerSpec.ModelType`: the only kind of model that is read here.
const UNIGRAM: u64 = 1;

/// Cuts source text into the ids the encoder reads.
pub(crate) struct Tokenizer {
    rules: Charsmap,
    /// How likely each piece of the SentencePiece model is (a logarithm).
    scores: HashMap<String, f32>,
    /// The row in the weights of every entry of `vocab.json`.
    rows: HashMap<String, u32>,
    longest_piece: usize,
    unknown_score: f32,
    unknown: u32,
}

/// The pieces by id, to turn what the decoder produced back into text.
pub(crate) struct Vocab {
    pieces: Vec<String>,
    unknown: u32,
}

/// A text as ids, with the parts the model cannot read (emoji, other
/// scripts) kept aside so they can be put back in the translation: there is
/// one entry in `unknown` for each unknown id.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Encoded {
    pub ids: Vec<u32>,
    pub unknown: Vec<String>,
}

/// Reads both files of a model folder.
pub(crate) fn load(dir: &Path) -> Result<(Tokenizer, Vocab)> {
    let vocab_bytes = std::fs::read(dir.join(VOCAB_FILE)).map_err(|error| Failure::damaged(VOCAB_FILE, error))?;
    let ids: HashMap<String, u32> =
        serde_json::from_slice(&vocab_bytes).map_err(|error| Failure::damaged(VOCAB_FILE, error))?;
    let unknown = *ids.get("<unk>").ok_or_else(|| Failure::damaged(VOCAB_FILE, "it has no <unk> entry"))?;
    if ids.values().any(|id| *id >= LARGEST_VOCAB) {
        return Err(Failure::damaged(VOCAB_FILE, "an id is too large"));
    }

    let model_bytes = std::fs::read(dir.join(SOURCE_FILE)).map_err(|error| Failure::damaged(SOURCE_FILE, error))?;
    let vocab = Vocab::new(&ids, unknown);
    let tokenizer = Tokenizer::parse(&model_bytes, ids, unknown)
        .ok_or_else(|| Failure::damaged(SOURCE_FILE, "it is not a SentencePiece unigram model"))?;
    Ok((tokenizer, vocab))
}

impl Tokenizer {
    fn parse(model: &[u8], rows: HashMap<String, u32>, unknown: u32) -> Option<Self> {
        let mut scores = HashMap::new();
        let mut rules = Charsmap::default();
        let mut lowest = f32::INFINITY;
        let mut longest_piece = 0;

        let mut fields = Fields::new(model);
        while let Some((number, value)) = fields.next()? {
            match (number, value) {
                // ModelProto.pieces
                (1, Value::Bytes(bytes)) => {
                    let (text, score, kind) = parse_piece(bytes)?;
                    if kind == NORMAL_PIECE {
                        lowest = lowest.min(score);
                        longest_piece = longest_piece.max(text.len());
                        scores.insert(text, score);
                    }
                }
                // ModelProto.trainer_spec
                (2, Value::Bytes(bytes)) => {
                    let mut spec = Fields::new(bytes);
                    while let Some((number, value)) = spec.next()? {
                        // TrainerSpec.model_type
                        if matches!((number, value), (3, Value::Varint(kind)) if kind != UNIGRAM) {
                            return None;
                        }
                    }
                }
                // ModelProto.normalizer_spec
                (3, Value::Bytes(bytes)) => {
                    let mut spec = Fields::new(bytes);
                    while let Some((number, value)) = spec.next()? {
                        match (number, value) {
                            // NormalizerSpec.precompiled_charsmap
                            (2, Value::Bytes(table)) => rules = Charsmap::parse(table)?,
                            // add_dummy_prefix, remove_extra_whitespaces and
                            // escape_whitespaces: `normalize` does all three,
                            // as every Marian model asks.
                            (3..=5, Value::Varint(0)) => return None,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        let unknown_score = lowest - UNKNOWN_PENALTY;
        (!scores.is_empty()).then_some(Self { rules, scores, rows, longest_piece, unknown_score, unknown })
    }

    pub fn encode(&self, text: &str) -> Encoded {
        let text = self.normalize(text);
        let mut encoded = Encoded::default();
        let mut cut = self.best_cut(&text).into_iter().peekable();
        while let Some((start, mut end, is_piece)) = cut.next() {
            // SentencePiece joins neighbours that have no piece into one
            // unknown part.
            while let Some((_, next_end, _)) = cut.next_if(|(_, _, next_is_piece)| !is_piece && !next_is_piece) {
                end = next_end;
            }
            // The row is looked up by the text of the part, as the reference
            // tokenizer does. So a piece that `vocab.json` leaves out (some
            // dashes) is unknown, and a character without a piece here that
            // the other language has ("ï" in English) is not.
            let part = &text[start..end];
            match self.rows.get(part) {
                Some(row) => encoded.ids.push(*row),
                None => {
                    encoded.ids.push(self.unknown);
                    encoded.unknown.push(part.to_string());
                }
            }
        }
        encoded
    }

    /// SentencePiece's `Normalizer::Normalize`: apply the rule table (it
    /// folds compatibility forms and turns every kind of blank into a
    /// space), squeeze the spaces, and mark each word start.
    fn normalize(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + SPACE_MARK.len_utf8());
        out.push(SPACE_MARK);
        // True at the start as well, which drops the leading spaces.
        let mut after_space = true;
        let mut rest = text;
        while let Some(first) = rest.chars().next() {
            let untouched = &rest[..first.len_utf8()];
            let (consumed, replacement) = self.rules.longest_rule(rest.as_bytes()).unwrap_or((untouched.len(), untouched));
            let kept = if after_space { replacement.trim_start_matches(' ') } else { replacement };
            if !kept.is_empty() {
                out.extend(kept.chars().map(|c| if c == ' ' { SPACE_MARK } else { c }));
                after_space = kept.ends_with(' ');
            }
            // A rule always covers whole characters; if a broken table did
            // not, skip one character rather than cut inside it.
            rest = rest.get(consumed..).unwrap_or(&rest[untouched.len()..]);
        }
        while out.ends_with(SPACE_MARK) {
            out.pop();
        }
        out
    }

    /// The most likely cut of `text` into pieces, as `(start, end,
    /// is_piece)` in order; a character that no piece covers comes out
    /// alone, with `false`. This is the Viterbi search of SentencePiece's
    /// `EncodeOptimized`: `best[end]` is the best way to reach byte `end`.
    fn best_cut(&self, text: &str) -> Vec<(usize, usize, bool)> {
        #[derive(Clone, Copy)]
        struct Arrival {
            score: f32,
            start: usize,
            is_piece: bool,
        }
        fn arrive(best: &mut [Option<Arrival>], end: usize, arrival: Arrival) {
            if best[end].is_none_or(|known| arrival.score > known.score) {
                best[end] = Some(arrival);
            }
        }

        let mut best: Vec<Option<Arrival>> = vec![None; text.len() + 1];
        for (start, first) in text.char_indices() {
            let so_far = best[start].map_or(0.0, |arrival| arrival.score);
            let one_char = start + first.len_utf8();
            let mut one_char_piece = false;
            for (offset, c) in text[start..].char_indices() {
                let end = start + offset + c.len_utf8();
                if end - start > self.longest_piece {
                    break;
                }
                if let Some(score) = self.scores.get(&text[start..end]) {
                    arrive(&mut best, end, Arrival { score: so_far + score, start, is_piece: true });
                    one_char_piece |= end == one_char;
                }
            }
            if !one_char_piece {
                let score = so_far + self.unknown_score;
                arrive(&mut best, one_char, Arrival { score, start, is_piece: false });
            }
        }

        // Every character boundary was reached, by a piece or as unknown, so
        // walking back from the end always lands on the start.
        let mut cut = Vec::new();
        let mut end = text.len();
        while let Some(arrival) = best[end].filter(|_| end > 0) {
            cut.push((arrival.start, end, arrival.is_piece));
            end = arrival.start;
        }
        cut.reverse();
        cut
    }
}

impl Vocab {
    fn new(ids: &HashMap<String, u32>, unknown: u32) -> Self {
        let size = ids.values().max().map_or(0, |largest| *largest as usize + 1);
        let mut pieces = vec![String::new(); size];
        for (piece, id) in ids {
            pieces[*id as usize].clone_from(piece);
        }
        Self { pieces, unknown }
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    /// The text of decoder ids. `skip` lists the ids that are not text (end
    /// of sentence, padding).
    ///
    /// An unknown token in the output stands for something the model could
    /// not read in the source. When the output has as many of them as the
    /// source, each one is the matching source part, copied as it was: an
    /// emoji comes through. Otherwise there is no way to tell which is
    /// which, and they are dropped.
    pub fn decode(&self, ids: &[u32], skip: &[u32], source_unknown: &[String]) -> String {
        let unknown_count = ids.iter().filter(|id| **id == self.unknown).count();
        let mut copies = (unknown_count == source_unknown.len()).then(|| source_unknown.iter());
        let mut text = String::new();
        for id in ids.iter().filter(|id| !skip.contains(id)) {
            if *id == self.unknown {
                if let Some(copy) = copies.as_mut().and_then(Iterator::next) {
                    text.push_str(copy);
                }
            } else if let Some(piece) = self.pieces.get(*id as usize) {
                text.push_str(piece);
            }
        }
        text.replace(SPACE_MARK, " ").trim().to_string()
    }
}

/// The rule table of a SentencePiece model: a double-array trie (the layout
/// of the Darts library) from byte strings to offsets in `replacements`,
/// where each replacement ends with a zero byte.
#[derive(Default)]
struct Charsmap {
    units: Vec<u32>,
    replacements: String,
}

impl Charsmap {
    fn parse(table: &[u8]) -> Option<Self> {
        if table.is_empty() {
            return Some(Self::default());
        }
        let (size, rest) = table.split_first_chunk::<4>()?;
        let (trie, replacements) = rest.split_at_checked(u32::from_le_bytes(*size) as usize)?;
        let units = trie.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        Some(Self { units, replacements: String::from_utf8(replacements.to_vec()).ok()? })
    }

    /// The longest rule that matches the start of `input`: how many bytes it
    /// covers and what replaces them.
    fn longest_rule(&self, input: &[u8]) -> Option<(usize, &str)> {
        let offset = |unit: u32| ((unit >> 10) << ((unit & (1 << 9)) >> 6)) as usize;
        let mut node = offset(*self.units.first()?);
        let mut longest = None;
        for (index, byte) in input.iter().enumerate() {
            node ^= *byte as usize;
            let Some(unit) = self.units.get(node).copied() else { break };
            // The low byte holds the label; the top bit marks a value unit.
            if unit & ((1 << 31) | 0xFF) != *byte as u32 {
                break;
            }
            node ^= offset(unit);
            // Bit 8 says that a rule ends here; its value is in the unit
            // the offset points to.
            if (unit >> 8) & 1 == 1
                && let Some(value) = self.units.get(node)
            {
                longest = Some((index + 1, (value & ((1 << 31) - 1)) as usize));
            }
        }
        let (consumed, at) = longest?;
        let replacement = self.replacements.get(at..)?;
        Some((consumed, replacement.split('\0').next().unwrap_or(replacement)))
    }
}

/// `(piece, score, type)` of one `ModelProto.SentencePiece`.
fn parse_piece(bytes: &[u8]) -> Option<(String, f32, u64)> {
    let (mut text, mut score, mut kind) = (None, 0.0, NORMAL_PIECE);
    let mut fields = Fields::new(bytes);
    while let Some((number, value)) = fields.next()? {
        match (number, value) {
            (1, Value::Bytes(bytes)) => text = Some(String::from_utf8(bytes.to_vec()).ok()?),
            (2, Value::Fixed32(bits)) => score = f32::from_bits(bits),
            (3, Value::Varint(value)) => kind = value,
            _ => {}
        }
    }
    Some((text?, score, kind))
}

/// Walks the fields of a protobuf message. That is all of protobuf a
/// SentencePiece model needs.
struct Fields<'a> {
    bytes: &'a [u8],
}

enum Value<'a> {
    Varint(u64),
    Fixed64,
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// The next field as `(number, value)`. The outer `None` is a malformed
    /// message, the inner one its end.
    fn next(&mut self) -> Option<Option<(u64, Value<'a>)>> {
        if self.bytes.is_empty() {
            return Some(None);
        }
        let key = self.varint()?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => {
                self.take(8)?;
                Value::Fixed64
            }
            2 => {
                let len = self.varint()?;
                Value::Bytes(self.take(usize::try_from(len).ok()?)?)
            }
            5 => Value::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
            _ => return None,
        };
        Some(Some((key >> 3, value)))
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.take(1)?.first()?;
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(len)?;
        self.bytes = rest;
        Some(taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn bytes_field(number: u64, payload: &[u8]) -> Vec<u8> {
        [varint(number << 3 | 2), varint(payload.len() as u64), payload.to_vec()].concat()
    }

    fn piece(text: &str, score: f32, kind: u64) -> Vec<u8> {
        let fields = [
            bytes_field(1, text.as_bytes()),
            [varint(2 << 3 | 5), score.to_le_bytes().to_vec()].concat(),
            [varint(3 << 3), varint(kind)].concat(),
        ];
        bytes_field(1, &fields.concat())
    }

    /// A tiny model: `<unk>` first like the real ones, then a few pieces.
    fn tiny() -> (Tokenizer, Vocab) {
        let model = [
            piece("<unk>", 0.0, 2),
            piece("</s>", 0.0, 3),
            piece("\u{2581}", -3.0, 1),
            piece("\u{2581}chat", -4.0, 1),
            piece("\u{2581}ch", -5.0, 1),
            piece("at", -5.0, 1),
            piece("s", -3.5, 1),
            piece("\u{2581}le", -2.0, 1),
            piece("absent", -1.0, 1),
        ]
        .concat();
        let ids: HashMap<String, u32> = [
            ("</s>", 0),
            ("<unk>", 1),
            ("\u{2581}", 2),
            ("\u{2581}chat", 3),
            ("\u{2581}ch", 4),
            ("at", 5),
            ("s", 6),
            ("\u{2581}le", 7),
            ("ï", 8),
        ]
        .into_iter()
        .map(|(piece, id)| (piece.to_string(), id))
        .collect();
        let vocab = Vocab::new(&ids, 1);
        (Tokenizer::parse(&model, ids, 1).expect("a valid model"), vocab)
    }

    #[test]
    fn the_likeliest_cut_wins_and_blanks_are_squeezed() {
        let (tokenizer, _) = tiny();
        // "▁chat" (-4) beats "▁ch" + "at" (-10).
        assert_eq!(tokenizer.encode("  le   chats ").ids, [7, 3, 6]);
        assert_eq!(tokenizer.encode(""), Encoded::default());
        assert_eq!(tokenizer.encode("   "), Encoded::default());
    }

    #[test]
    fn a_run_without_pieces_is_one_unknown_token_and_is_kept_aside() {
        let (tokenizer, _) = tiny();
        let encoded = tokenizer.encode("le 😀🎉 chat");
        assert_eq!(encoded.ids, [7, 2, 1, 3]);
        assert_eq!(encoded.unknown, ["😀🎉"]);
    }

    #[test]
    fn the_vocabulary_decides_what_is_unknown_not_the_pieces() {
        let (tokenizer, _) = tiny();
        // "absent" is a piece that the vocabulary leaves out.
        let encoded = tokenizer.encode("absent😀");
        assert_eq!(encoded.ids, [2, 1, 1]);
        assert_eq!(encoded.unknown, ["absent", "😀"]);
        // "ï" is no piece, but the vocabulary lists it.
        assert_eq!(tokenizer.encode("le ï"), Encoded { ids: vec![7, 2, 8], unknown: vec![] });
    }

    #[test]
    fn decoding_puts_the_source_parts_back_only_when_they_can_be_matched() {
        let (_, vocab) = tiny();
        let unknown = ["😀".to_string()];
        assert_eq!(vocab.decode(&[7, 3, 2, 1, 0], &[0], &unknown), "le chat 😀");
        assert_eq!(vocab.decode(&[7, 1, 3, 1, 0], &[0], &unknown), "le chat");
        assert_eq!(vocab.decode(&[7, 3, 99], &[0], &[]), "le chat");
    }

    #[test]
    fn a_truncated_model_is_refused() {
        let model = piece("\u{2581}le", -2.0, 1);
        assert!(Tokenizer::parse(&model[..model.len() - 3], HashMap::new(), 1).is_none());
        assert!(Tokenizer::parse(&[], HashMap::new(), 1).is_none());
    }
}
