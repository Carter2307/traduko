//! `dialect` — deterministic American <-> British English post-editing.
//!
//! Made for the output of a French -> English translation model that writes "generic"
//! English: the caller picks the variant, this crate rewrites the text.
//!
//! ```
//! use dialect::{convert, EnglishVariant, Options};
//! let us = convert("My favourite colour is grey.", EnglishVariant::American, &Options::default());
//! assert_eq!(us, "My favorite color is gray.");
//! ```
//!
//! Three layers, applied in this order:
//! 1. **Rules** (`assets/rules_*.tsv`): multi-word phrases and words that need context.
//!    Layer `S` = spelling homographs (program/programme, tire/tyre, check/cheque ...),
//!    layer `V` = safe vocabulary (lorry/truck), layer `X` = extended vocabulary.
//! 2. **Spelling map** (`assets/spelling_*.tsv`, generated from VarCon): one word -> one word.
//! 3. **Prefix fallback**: `un-`, `re-`, `non-` ... + a word of the map.
//!
//! What is never touched: URLs, e-mail addresses, code-like tokens, text between
//! backticks, mixed-case words (`iPhone`), and Capitalised words that are not the first
//! word of a sentence (proper-noun guard, see [`Options::protect_proper_nouns`]).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static SPELLING_A2B: &str = include_str!("../assets/spelling_a2b.tsv");
static SPELLING_B2A: &str = include_str!("../assets/spelling_b2a.tsv");
static RULES_A2B: &str = include_str!("../assets/rules_a2b.tsv");
static RULES_B2A: &str = include_str!("../assets/rules_b2a.tsv");

/// Target variant of English.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnglishVariant {
    American,
    British,
}

/// How much vocabulary (not spelling) is adapted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Vocabulary {
    /// Spelling only: "lorry" stays "lorry".
    Off,
    /// Swaps that are reliable without knowing the topic (lorry/truck, petrol/gasoline,
    /// "block of flats"/"apartment building"), with context guards for "lift" and "flat".
    Safe,
    /// `Safe` + swaps that assume a topic (football -> soccer, biscuit -> cookie).
    Extended,
}

/// Conversion options. `Options::default()` = spelling on, `Vocabulary::Safe`,
/// British "-ise", proper-noun guard on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Convert spelling (colour/color, organise/organize, centre/center ...).
    pub spelling: bool,
    /// Convert vocabulary (lorry/truck ...). Separate from spelling on purpose.
    pub vocabulary: Vocabulary,
    /// British target only: Oxford spelling, i.e. keep/produce "-ize" ("organize",
    /// "realize") while still writing "colour", "centre", "analyse".
    pub oxford_ize: bool,
    /// Leave Capitalised words alone unless they start a sentence and the next word is
    /// lower case ("the Labour Party", "Pearl Harbor", "World Health Organization").
    pub protect_proper_nouns: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            spelling: true,
            vocabulary: Vocabulary::Safe,
            oxford_ize: false,
            protect_proper_nouns: true,
        }
    }
}

impl Options {
    /// Spelling only, no vocabulary swap.
    pub fn spelling_only() -> Self {
        Options { vocabulary: Vocabulary::Off, ..Options::default() }
    }
}

/// Which layer made a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Spelling,
    Vocabulary,
    /// "a" <-> "an" fixed after a vocabulary swap.
    Article,
}

/// One edit, with the byte range of the new text in [`Conversion::text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub from: String,
    pub to: String,
    pub kind: ChangeKind,
}

/// Result of [`convert_detailed`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub text: String,
    pub changes: Vec<Change>,
}

/// Convert `text` to the `to` variant. Text that is already in that variant is returned
/// unchanged, so the function is idempotent.
pub fn convert(text: &str, to: EnglishVariant, opts: &Options) -> String {
    convert_detailed(text, to, opts).text
}

/// Like [`convert`], and also reports every edit (for an "adapted words" hint in a UI).
pub fn convert_detailed(text: &str, to: EnglishVariant, opts: &Options) -> Conversion {
    let data = &*DATA;
    let mut st = State::new(text);
    let rules = match to {
        EnglishVariant::American => &data.rules_b2a,
        EnglishVariant::British => &data.rules_a2b,
    };
    st.apply_rules(rules, opts);
    if opts.spelling {
        let map = match (to, opts.oxford_ize) {
            (EnglishVariant::American, _) => &data.to_american,
            (EnglishVariant::British, false) => &data.to_british,
            (EnglishVariant::British, true) => &data.to_british_oxford,
        };
        st.apply_spelling(map, opts);
    }
    st.finish()
}

/// Force the lazy parse of the embedded tables (about 1 ms). Optional: call it on a
/// background thread at start-up so the first translation does not pay for it.
pub fn warm_up() {
    LazyLock::force(&DATA);
}

/// Sizes of the embedded data, for diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct DataStats {
    pub embedded_bytes: usize,
    pub british_to_american_pairs: usize,
    pub american_to_british_pairs: usize,
    pub rules_british_to_american: usize,
    pub rules_american_to_british: usize,
}

pub fn data_stats() -> DataStats {
    let d = &*DATA;
    DataStats {
        embedded_bytes: SPELLING_A2B.len() + SPELLING_B2A.len() + RULES_A2B.len() + RULES_B2A.len(),
        british_to_american_pairs: d.to_american.len(),
        american_to_british_pairs: d.to_british.len(),
        rules_british_to_american: d.rules_b2a.rules.len(),
        rules_american_to_british: d.rules_a2b.rules.len(),
    }
}

// ------------------------------------------------------------------------------------
// Embedded data
// ------------------------------------------------------------------------------------

type Map = HashMap<&'static str, &'static str>;

struct Data {
    to_american: Map,
    to_british: Map,
    to_british_oxford: Map,
    rules_b2a: RuleSet,
    rules_a2b: RuleSet,
}

static DATA: LazyLock<Data> = LazyLock::new(|| {
    let mut to_american = Map::with_capacity(8192);
    for line in SPELLING_B2A.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        if let (Some(b), Some(a)) = (cols.next(), cols.next()) {
            to_american.insert(b, a);
        }
    }
    let mut to_british = Map::with_capacity(8192);
    let mut to_british_oxford = Map::with_capacity(8192);
    for line in SPELLING_A2B.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(a), Some(b)) = (cols.next(), cols.next()) else { continue };
        to_british.insert(a, b);
        match cols.next() {
            None => {
                to_british_oxford.insert(a, b);
            }
            // "=": Oxford spelling keeps the American form; normalise "-ise" input to it.
            Some("=") => {
                to_british_oxford.insert(b, a);
            }
            Some(ox) => {
                to_british_oxford.insert(a, ox);
                to_british_oxford.insert(b, ox);
            }
        }
    }
    Data {
        to_american,
        to_british,
        to_british_oxford,
        rules_b2a: RuleSet::parse(RULES_B2A),
        rules_a2b: RuleSet::parse(RULES_A2B),
    }
});

// ------------------------------------------------------------------------------------
// Rules
// ------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Spelling,
    Safe,
    Extended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Elem {
    Word(&'static str),
    Num,
    Poss,
}

#[derive(Debug, Default)]
struct Guards {
    prev_in: Option<Vec<&'static str>>,
    prev_not: Vec<&'static str>,
    next_in: Option<Vec<&'static str>>,
    next_not: Vec<&'static str>,
    sent_in: Option<Vec<&'static str>>,
    sent_not: Vec<&'static str>,
    /// Words that must not appear anywhere in the text (document-level cue).
    doc_not: Vec<&'static str>,
    title: bool,
}

#[derive(Debug)]
struct Rule {
    layer: Layer,
    src: Vec<Elem>,
    /// Separator in front of element k: b' ', b'-' or 0 (first element, possessive).
    seps: Vec<u8>,
    /// `None` = freeze the source ("=").
    dst: Option<&'static str>,
    /// Word-by-word replacement when source and target have the same shape.
    dst_words: Option<Vec<&'static str>>,
    guards: Guards,
}

struct RuleSet {
    rules: Vec<Rule>,
    by_first: HashMap<&'static str, Vec<u32>>,
    num_first: Vec<u32>,
}

fn parse_phrase(s: &'static str) -> (Vec<Elem>, Vec<u8>) {
    let mut elems = Vec::new();
    let mut seps = Vec::new();
    let mut sep = 0u8;
    let mut start = 0;
    let bytes = s.as_bytes();
    let push = |piece: &'static str, sep: u8, elems: &mut Vec<Elem>, seps: &mut Vec<u8>| {
        if piece.is_empty() {
            return;
        }
        if piece == "#" {
            elems.push(Elem::Num);
            seps.push(sep);
        } else if let Some(base) = piece.strip_suffix("'s") {
            elems.push(Elem::Word(base));
            seps.push(sep);
            elems.push(Elem::Poss);
            seps.push(0);
        } else {
            elems.push(Elem::Word(piece));
            seps.push(sep);
        }
    };
    for (i, &b) in bytes.iter().enumerate() {
        if b == b' ' || b == b'-' {
            push(&s[start..i], sep, &mut elems, &mut seps);
            sep = b;
            start = i + 1;
        }
    }
    push(&s[start..], sep, &mut elems, &mut seps);
    if let Some(first) = seps.first_mut() {
        *first = 0;
    }
    (elems, seps)
}

fn parse_list(s: &'static str) -> Vec<&'static str> {
    s.split(',').filter(|x| !x.is_empty()).collect()
}

impl RuleSet {
    fn parse(src: &'static str) -> RuleSet {
        let mut rules = Vec::new();
        for line in src.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&'static str> = line.split('\t').collect();
            assert!(cols.len() >= 3, "bad rule line: {line}");
            let layer = match cols[0] {
                "S" => Layer::Spelling,
                "V" => Layer::Safe,
                "X" => Layer::Extended,
                other => panic!("unknown rule layer {other:?} in: {line}"),
            };
            let sources: Vec<&'static str> = cols[1].split('|').collect();
            let targets: Vec<&'static str> = cols[2].split('|').collect();
            assert!(
                targets.len() == sources.len() || targets.len() == 1,
                "source/target alternatives do not pair up: {line}"
            );
            for (k, source) in sources.iter().enumerate() {
                let target = if targets.len() == 1 { targets[0] } else { targets[k] };
                let mut guards = Guards::default();
                if let Some(g) = cols.get(3) {
                    for part in g.split(';').filter(|p| !p.is_empty()) {
                        if part == "title" {
                            guards.title = true;
                        } else if let Some(v) = part.strip_prefix("p+:") {
                            guards.prev_in = Some(parse_list(v));
                        } else if let Some(v) = part.strip_prefix("p-:") {
                            guards.prev_not = parse_list(v);
                        } else if let Some(v) = part.strip_prefix("n+:") {
                            guards.next_in = Some(parse_list(v));
                        } else if let Some(v) = part.strip_prefix("n-:") {
                            guards.next_not = parse_list(v);
                        } else if let Some(v) = part.strip_prefix("s+:") {
                            guards.sent_in = Some(parse_list(v));
                        } else if let Some(v) = part.strip_prefix("s-:") {
                            guards.sent_not = parse_list(v);
                        } else if let Some(v) = part.strip_prefix("d-:") {
                            guards.doc_not = parse_list(v);
                        } else {
                            panic!("unknown guard {part:?} in: {line}");
                        }
                    }
                }
                let (src, seps) = parse_phrase(source);
                assert!(!src.is_empty(), "empty rule source: {line}");
                let (dst, dst_words) = if target == "=" {
                    (None, None)
                } else {
                    let (d_elems, d_seps) = parse_phrase(target);
                    let same_shape = d_elems.len() == src.len()
                        && d_seps == seps
                        && d_elems.iter().zip(&src).all(|(d, s)| {
                            matches!(
                                (d, s),
                                (Elem::Word(_), Elem::Word(_)) | (Elem::Num, Elem::Num) | (Elem::Poss, Elem::Poss)
                            )
                        });
                    let words = same_shape.then(|| {
                        d_elems
                            .iter()
                            .map(|e| match e {
                                Elem::Word(w) => *w,
                                _ => "",
                            })
                            .collect()
                    });
                    assert!(same_shape || !target.contains('#'), "'#' needs the same shape on both sides: {line}");
                    (Some(target), words)
                };
                rules.push(Rule { layer, src, seps, dst, dst_words, guards });
            }
        }
        // Longest source first; vocabulary before spelling when the length is the same.
        let mut order: Vec<u32> = (0..rules.len() as u32).collect();
        order.sort_by_key(|&i| {
            let r = &rules[i as usize];
            (std::cmp::Reverse(r.src.len()), r.layer == Layer::Spelling, i)
        });
        let mut by_first: HashMap<&'static str, Vec<u32>> = HashMap::new();
        let mut num_first = Vec::new();
        for i in order {
            match rules[i as usize].src[0] {
                Elem::Word(w) => by_first.entry(w).or_default().push(i),
                Elem::Num => num_first.push(i),
                Elem::Poss => panic!("a rule cannot start with a possessive"),
            }
        }
        RuleSet { rules, by_first, num_first }
    }
}

// ------------------------------------------------------------------------------------
// Tokens
// ------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Word,
    /// `'s` split from the word in front of it.
    Poss,
    Number,
    Space,
    Newline,
    Punct,
    /// URL, e-mail, code-like token, text between backticks: copied as is.
    Protected,
}

struct Tok<'a> {
    kind: Kind,
    text: Cow<'a, str>,
    /// Already handled by a rule: later passes leave it alone.
    frozen: bool,
    /// Index of the sentence the token belongs to.
    sent: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Lower,
    Upper,
    Title,
    Mixed,
}

fn case_of(word: &str) -> Case {
    let mut letters = word.chars().filter(|c| c.is_alphabetic());
    let Some(first) = letters.next() else { return Case::Lower };
    let (mut lower, mut upper) = (0usize, 0usize);
    for c in letters {
        if c.is_uppercase() {
            upper += 1;
        } else {
            lower += 1;
        }
    }
    if first.is_uppercase() {
        match (lower, upper) {
            (_, 0) => Case::Title,
            (0, _) => Case::Upper,
            _ => Case::Mixed,
        }
    } else if upper == 0 {
        Case::Lower
    } else {
        Case::Mixed
    }
}

fn apply_case(replacement: &str, case: Case) -> String {
    match case {
        Case::Upper => replacement.to_uppercase(),
        Case::Title => capitalize(replacement),
        _ => replacement.to_string(),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn lower(s: &str) -> Cow<'_, str> {
    if s.bytes().all(|b| b.is_ascii_lowercase()) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.to_lowercase())
    }
}

fn is_hyphen(s: &str) -> bool {
    matches!(s, "-" | "\u{2010}" | "\u{2011}")
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

const NUMBER_WORDS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "fifteen", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety", "hundred",
    "thousand", "million",
];

fn is_number_word(lower_word: &str) -> bool {
    NUMBER_WORDS.contains(&lower_word)
}

/// Abbreviations whose full stop does not end a sentence.
const ABBREVIATIONS: &[&str] = &["mr", "mrs", "ms", "dr", "prof", "st", "mt", "sr", "jr", "vs", "etc", "fig", "approx"];

/// Prefixes accepted in front of a word of the spelling map ("unorganised", "recolour").
const PREFIXES: &[&str] = &[
    "counter", "hyper", "inter", "micro", "multi", "super", "under", "anti", "auto", "over", "post", "semi",
    "dis", "mis", "non", "pre", "sub", "co", "de", "re", "un",
];

/// A whitespace-delimited chunk that looks like a URL, an e-mail address, a path, an
/// identifier or another piece of code. Such chunks are never edited.
fn is_technical(chunk: &str) -> bool {
    // call(), colour(s): a letter directly followed by "(" (checked before the trim below)
    let mut prev_alpha = false;
    for c in chunk.chars() {
        if c == '(' && prev_alpha {
            return true;
        }
        prev_alpha = c.is_alphabetic();
    }
    let core = chunk.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '*'
                | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}' | '\u{00AB}' | '\u{00BB}' | '\u{2026}'
        )
    });
    if core.is_empty() {
        return false;
    }
    if core.contains("://") || core.starts_with("www.") {
        return true;
    }
    let cs: Vec<char> = core.chars().collect();
    if cs[0] == '-' && cs.len() > 1 && (cs[1].is_alphabetic() || cs[1] == '-') {
        return true; // -flag, --flag
    }
    let mut slashes = 0;
    for (k, &c) in cs.iter().enumerate() {
        let prev = if k > 0 { Some(cs[k - 1]) } else { None };
        let next = cs.get(k + 1).copied();
        match c {
            '@' | '_' | '\\' | '=' | '<' | '>' | '|' | '~' | '^' => return true,
            '#' | '$' => {
                if next.is_some_and(|n| n.is_alphabetic()) {
                    return true;
                }
            }
            '.' | ':' => {
                if let (Some(p), Some(n)) = (prev, next) {
                    if p.is_alphanumeric() && n.is_alphanumeric() && (p.is_alphabetic() || n.is_alphabetic()) {
                        return true; // example.com, file.txt, key:value
                    }
                }
            }
            '&' | '+' => {
                if prev.is_some_and(|p| p.is_alphanumeric()) {
                    return true; // R&D, C++
                }
            }
            '(' => {
                if prev.is_some_and(|p| p.is_alphabetic()) {
                    return true; // call(), colour(s)
                }
            }
            '/' => slashes += 1,
            _ => {}
        }
        if let Some(p) = prev {
            if (p.is_alphabetic() && c.is_ascii_digit()) || (p.is_ascii_digit() && c.is_alphabetic()) {
                return true; // h264, x86, 3D
            }
            if p.is_lowercase() && c.is_uppercase() {
                return true; // camelCase, iPhone
            }
        }
    }
    slashes > 0
        && (slashes >= 2 || cs[0] == '/' || cs[0] == '~' || core.starts_with("./") || core.starts_with("../") || core.contains('.'))
}

/// Byte ranges of Markdown code: ``` fenced blocks ``` and `inline spans` that close on
/// the same line. A backtick with no partner is plain punctuation (`quoted' man-page style).
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    if !text.contains('`') {
        return spans;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        if text[i..].starts_with("```") {
            match text[i + 3..].find("```") {
                Some(rel) => {
                    let end = i + 3 + rel + 3;
                    spans.push((i, end));
                    i = end;
                }
                None => i += 3,
            }
            continue;
        }
        let line_end = text[i + 1..].find('\n').map_or(text.len(), |r| i + 1 + r);
        match text[i + 1..line_end].find('`') {
            Some(rel) => {
                let end = i + 1 + rel + 1;
                spans.push((i, end));
                i = end;
            }
            None => i += 1,
        }
    }
    spans
}

fn tokenize(text: &str) -> Vec<Tok<'_>> {
    let mut toks: Vec<Tok<'_>> = Vec::with_capacity(text.len() / 3 + 4);
    let spans = code_spans(text);
    let mut span_idx = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let ws = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
        if ws > 0 {
            let s = &rest[..ws];
            let kind = if s.contains('\n') { Kind::Newline } else { Kind::Space };
            toks.push(Tok { kind, text: Cow::Borrowed(s), frozen: false, sent: 0 });
            i += ws;
            continue;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let chunk = &rest[..len];
        while span_idx < spans.len() && spans[span_idx].1 <= i {
            span_idx += 1;
        }
        let in_code = spans.get(span_idx).is_some_and(|&(start, _)| start < i + len);
        if in_code || is_technical(chunk) {
            toks.push(Tok { kind: Kind::Protected, text: Cow::Borrowed(chunk), frozen: true, sent: 0 });
        } else {
            split_chunk(chunk, &mut toks);
        }
        i += len;
    }
    // Sentence numbers: a sentence ends at . ! ? … (not after "Mr.") or at a line break.
    let mut sent = 0u32;
    for k in 0..toks.len() {
        toks[k].sent = sent;
        let ends = match toks[k].kind {
            Kind::Newline => true,
            Kind::Punct => match &*toks[k].text {
                "!" | "?" | "\u{2026}" => true,
                "." => !(k > 0 && toks[k - 1].kind == Kind::Word && ABBREVIATIONS.contains(&&*lower(&toks[k - 1].text))),
                _ => false,
            },
            _ => false,
        };
        if ends {
            sent += 1;
        }
    }
    toks
}

fn split_chunk<'a>(chunk: &'a str, toks: &mut Vec<Tok<'a>>) {
    let mut push = |kind: Kind, s: &'a str| toks.push(Tok { kind, text: Cow::Borrowed(s), frozen: false, sent: 0 });
    let chars: Vec<(usize, char)> = chunk.char_indices().collect();
    let end_of = |k: usize| chars.get(k).map_or(chunk.len(), |&(b, _)| b);
    let mut k = 0;
    while k < chars.len() {
        let (start, c) = chars[k];
        if c.is_alphabetic() {
            let mut j = k + 1;
            while j < chars.len() {
                let cj = chars[j].1;
                if cj.is_alphabetic() || (is_apostrophe(cj) && chars.get(j + 1).is_some_and(|&(_, n)| n.is_alphabetic())) {
                    j += 1;
                } else {
                    break;
                }
            }
            let word = &chunk[start..end_of(j)];
            // possessive: letters + ' + s, and no other apostrophe in the word
            let mut split = None;
            if j - k >= 3 && matches!(chars[j - 1].1, 's' | 'S') && is_apostrophe(chars[j - 2].1) {
                let base = &chunk[start..chars[j - 2].0];
                if !base.chars().any(is_apostrophe) {
                    split = Some(chars[j - 2].0 - start);
                }
            }
            match split {
                Some(at) => {
                    push(Kind::Word, &word[..at]);
                    push(Kind::Poss, &word[at..]);
                }
                None => push(Kind::Word, word),
            }
            k = j;
        } else if c.is_ascii_digit() {
            let mut j = k + 1;
            while j < chars.len() {
                let cj = chars[j].1;
                if cj.is_ascii_digit()
                    || (matches!(cj, '.' | ',' | ':') && chars.get(j + 1).is_some_and(|&(_, n)| n.is_ascii_digit()))
                {
                    j += 1;
                } else {
                    break;
                }
            }
            push(Kind::Number, &chunk[start..end_of(j)]);
            k = j;
        } else {
            push(Kind::Punct, &chunk[start..end_of(k + 1)]);
            k += 1;
        }
    }
}

// ------------------------------------------------------------------------------------
// Conversion state
// ------------------------------------------------------------------------------------

/// What a guard sees on one side of a match.
enum Neighbour<'a> {
    Boundary,
    Word(Cow<'a, str>),
    Number,
    Poss,
    Hyphen,
    Punct(&'a str),
    Other,
}

fn neighbour_in(list: &[&str], n: &Neighbour<'_>) -> bool {
    list.iter().any(|item| match n {
        Neighbour::Boundary => *item == "|",
        Neighbour::Word(w) => *item == w.as_ref() || (*item == "#" && is_number_word(w)),
        Neighbour::Number => *item == "#",
        Neighbour::Poss => *item == "'s",
        Neighbour::Hyphen => *item == "-",
        Neighbour::Punct(p) => *item == "|" || item == p,
        Neighbour::Other => false,
    })
}

struct PendingChange {
    first: usize,
    last: usize,
    from: String,
    kind: ChangeKind,
}

struct State<'a> {
    toks: Vec<Tok<'a>>,
    changes: Vec<PendingChange>,
    /// Lower-cased words seen Capitalised in the middle of a sentence somewhere in the
    /// text: names. "Mr. Grey lives here. Grey is ..." keeps the second "Grey" too.
    names: HashSet<String>,
    /// Every lower-cased word of the text, built on first use by a `d-` guard.
    doc_words: std::cell::OnceCell<HashSet<String>>,
}

impl<'a> State<'a> {
    fn new(text: &'a str) -> Self {
        let mut st = State {
            toks: tokenize(text),
            changes: Vec::new(),
            names: HashSet::new(),
            doc_words: std::cell::OnceCell::new(),
        };
        for i in 0..st.toks.len() {
            let t = &st.toks[i];
            if t.kind == Kind::Word
                && t.text.len() > 1
                && t.text.is_ascii()
                && case_of(&t.text) == Case::Title
                && !st.sentence_initial(i)
            {
                let name = t.text.to_ascii_lowercase();
                st.names.insert(name);
            }
        }
        st
    }

    fn neighbour(&self, from: usize, forward: bool) -> Neighbour<'_> {
        let mut j = from as isize;
        loop {
            j += if forward { 1 } else { -1 };
            if j < 0 || j as usize >= self.toks.len() {
                return Neighbour::Boundary;
            }
            let t = &self.toks[j as usize];
            return match t.kind {
                Kind::Space => continue,
                Kind::Newline => Neighbour::Boundary,
                Kind::Word => Neighbour::Word(lower(&t.text)),
                Kind::Number => Neighbour::Number,
                Kind::Poss => Neighbour::Poss,
                Kind::Punct if is_hyphen(&t.text) => Neighbour::Hyphen,
                Kind::Punct => Neighbour::Punct(&t.text),
                Kind::Protected => Neighbour::Other,
            };
        }
    }

    /// True when the word at `i` is the first word of a sentence.
    fn sentence_initial(&self, i: usize) -> bool {
        let mut j = i;
        while j > 0 {
            j -= 1;
            let t = &self.toks[j];
            match t.kind {
                Kind::Space => continue,
                Kind::Newline => return true,
                Kind::Punct => match &*t.text {
                    "\"" | "'" | "\u{201C}" | "\u{2018}" | "\u{00AB}" | "(" | "[" | "*" | "\u{2014}" | "\u{2013}"
                    | "\u{2022}" | ">" | "\u{00BF}" | "\u{00A1}" => continue,
                    // a hyphen is a list bullet only when it stands alone
                    "-" if j == 0 || matches!(self.toks[j - 1].kind, Kind::Space | Kind::Newline) => continue,
                    "!" | "?" | "\u{2026}" => return true,
                    "." => {
                        return !(j > 0
                            && self.toks[j - 1].kind == Kind::Word
                            && ABBREVIATIONS.contains(&&*lower(&self.toks[j - 1].text)));
                    }
                    _ => return false,
                },
                _ => return false,
            }
        }
        true
    }

    /// True when the word after token `end` is Capitalised (and is not the pronoun "I").
    fn next_word_is_title(&self, end: usize) -> bool {
        let mut j = end + 1;
        if self.toks.get(j).is_some_and(|t| t.kind == Kind::Poss) {
            j += 1;
        }
        if !self.toks.get(j).is_some_and(|t| t.kind == Kind::Space) {
            return false;
        }
        match self.toks.get(j + 1) {
            Some(t) if t.kind == Kind::Word => {
                t.text.chars().count() > 1 && case_of(&t.text) == Case::Title
            }
            _ => false,
        }
    }

    /// Proper-noun guard for a Capitalised span `first..=last`.
    fn title_is_convertible(&self, first: usize, last: usize) -> bool {
        self.sentence_initial(first)
            && !self.next_word_is_title(last)
            && !self.names.contains(self.toks[first].text.to_ascii_lowercase().as_str())
    }

    fn try_match(&self, rule: &Rule, i: usize) -> Option<usize> {
        let mut j = i;
        for (k, elem) in rule.src.iter().enumerate() {
            if k > 0 {
                if *elem == Elem::Poss {
                    j += 1;
                } else {
                    let sep = self.toks.get(j + 1)?;
                    let ok = match rule.seps[k] {
                        b' ' => sep.kind == Kind::Space,
                        b'-' => sep.kind == Kind::Punct && is_hyphen(&sep.text),
                        _ => false,
                    };
                    if !ok {
                        return None;
                    }
                    j += 2;
                }
            }
            let t = self.toks.get(j)?;
            if t.frozen {
                return None;
            }
            let ok = match elem {
                Elem::Word(w) => t.kind == Kind::Word && t.text.eq_ignore_ascii_case(w),
                Elem::Num => t.kind == Kind::Number || (t.kind == Kind::Word && is_number_word(&lower(&t.text))),
                Elem::Poss => t.kind == Kind::Poss,
            };
            if !ok {
                return None;
            }
        }
        Some(j)
    }

    fn guards_pass(&self, g: &Guards, first: usize, last: usize) -> bool {
        if g.prev_in.is_some() || !g.prev_not.is_empty() {
            let n = self.neighbour(first, false);
            if g.prev_in.as_ref().is_some_and(|l| !neighbour_in(l, &n)) || neighbour_in(&g.prev_not, &n) {
                return false;
            }
        }
        if g.next_in.is_some() || !g.next_not.is_empty() {
            let n = self.neighbour(last, true);
            if g.next_in.as_ref().is_some_and(|l| !neighbour_in(l, &n)) || neighbour_in(&g.next_not, &n) {
                return false;
            }
        }
        if g.sent_in.is_some() || !g.sent_not.is_empty() {
            let sent = self.toks[first].sent;
            let mut lo = first;
            while lo > 0 && self.toks[lo - 1].sent == sent {
                lo -= 1;
            }
            let mut hi = last;
            while hi + 1 < self.toks.len() && self.toks[hi + 1].sent == sent {
                hi += 1;
            }
            let has = |list: &[&str]| {
                (lo..=hi).any(|k| {
                    (k < first || k > last)
                        && self.toks[k].kind == Kind::Word
                        && list.contains(&&*lower(&self.toks[k].text))
                })
            };
            if g.sent_in.as_ref().is_some_and(|l| !has(l)) || has(&g.sent_not) {
                return false;
            }
        }
        if !g.doc_not.is_empty() {
            let words = self.doc_words.get_or_init(|| {
                self.toks
                    .iter()
                    .filter(|t| t.kind == Kind::Word && t.text.is_ascii())
                    .map(|t| t.text.to_ascii_lowercase())
                    .collect()
            });
            if g.doc_not.iter().any(|w| words.contains(*w)) {
                return false;
            }
        }
        true
    }

    fn apply_rules(&mut self, set: &RuleSet, opts: &Options) {
        let enabled = |layer: Layer| match layer {
            Layer::Spelling => opts.spelling,
            Layer::Safe => opts.vocabulary >= Vocabulary::Safe,
            Layer::Extended => opts.vocabulary >= Vocabulary::Extended,
        };
        let mut i = 0;
        while i < self.toks.len() {
            let t = &self.toks[i];
            let candidates: Option<&Vec<u32>> = match t.kind {
                _ if t.frozen => None,
                Kind::Word => {
                    let key = lower(&t.text);
                    let by_word = set.by_first.get(key.as_ref());
                    if by_word.is_none() && is_number_word(&key) {
                        Some(&set.num_first)
                    } else {
                        by_word
                    }
                }
                Kind::Number => Some(&set.num_first),
                _ => None,
            };
            let mut next = i + 1;
            if let Some(candidates) = candidates {
                for &r in candidates {
                    let rule = &set.rules[r as usize];
                    if !enabled(rule.layer) {
                        continue;
                    }
                    let Some(last) = self.try_match(rule, i) else { continue };
                    if !self.guards_pass(&rule.guards, i, last) {
                        continue;
                    }
                    if self.apply_rule(rule, i, last, opts) {
                        next = last + 1;
                        break;
                    }
                }
            }
            i = next;
        }
    }

    /// Returns false when the capital letters of the match say "proper noun".
    fn apply_rule(&mut self, rule: &Rule, first: usize, last: usize, opts: &Options) -> bool {
        let word_idx: Vec<usize> = (first..=last).filter(|&k| self.toks[k].kind == Kind::Word).collect();
        let cases: Vec<Case> = word_idx.iter().map(|&k| case_of(&self.toks[k].text)).collect();
        let first_is_word = self.toks[first].kind == Kind::Word;
        if cases.contains(&Case::Mixed) {
            return false;
        }
        let first_title = first_is_word && cases.first() == Some(&Case::Title);
        if rule.guards.title && !first_title {
            return false;
        }
        let Some(dst) = rule.dst else {
            for k in first..=last {
                self.toks[k].frozen = true;
            }
            return true;
        };
        if opts.protect_proper_nouns {
            let later_title = cases.iter().skip(if first_is_word { 1 } else { 0 }).any(|c| *c == Case::Title);
            if later_title || (first_title && !self.title_is_convertible(first, last)) {
                return false;
            }
        }
        let kind = if rule.layer == Layer::Spelling { ChangeKind::Spelling } else { ChangeKind::Vocabulary };
        let from: String = (first..=last).map(|k| &*self.toks[k].text).collect();
        let old_first_word = word_idx.first().map(|&k| lower(&self.toks[k].text).into_owned());

        if let Some(words) = &rule.dst_words {
            let mut e = 0; // element index: tokens first..=last alternate element / separator
            let mut k = first;
            while k <= last {
                let elem = rule.src[e];
                if let Elem::Word(_) = elem {
                    let case = case_of(&self.toks[k].text);
                    self.toks[k].text = Cow::Owned(apply_case(words[e], case));
                }
                self.toks[k].frozen = true;
                e += 1;
                k += 1;
                // skip the separator token in front of the next element (none before a possessive)
                if e < rule.src.len() && rule.src[e] != Elem::Poss {
                    self.toks[k].frozen = true;
                    k += 1;
                }
            }
        } else {
            let all_upper = !cases.is_empty()
                && cases.iter().all(|c| *c == Case::Upper)
                && word_idx.iter().any(|&k| self.toks[k].text.chars().count() > 1);
            let text = if all_upper {
                dst.to_uppercase()
            } else if first_title {
                capitalize(dst)
            } else {
                dst.to_string()
            };
            self.toks[first].text = Cow::Owned(text);
            self.toks[first].frozen = true;
            for k in first + 1..=last {
                self.toks[k].text = Cow::Borrowed("");
                self.toks[k].frozen = true;
            }
        }
        self.changes.push(PendingChange { first, last, from, kind });

        if kind == ChangeKind::Vocabulary && first_is_word {
            let new_first_word = dst.split([' ', '-']).next().unwrap_or(dst);
            if let Some(old) = old_first_word {
                self.fix_article(first, &old, new_first_word);
            }
        }
        true
    }

    /// "a flat" -> "an apartment", "an elevator" -> "a lift".
    fn fix_article(&mut self, first: usize, old_word: &str, new_word: &str) {
        if first < 2 || self.toks[first - 1].kind != Kind::Space || self.toks[first - 2].kind != Kind::Word {
            return;
        }
        let needs_an = takes_an(new_word);
        if takes_an(old_word) == needs_an {
            return;
        }
        let art = &self.toks[first - 2];
        let replacement = match (&*lower(&art.text), needs_an) {
            ("a", true) => "an",
            ("an", false) => "a",
            _ => return,
        };
        let case = case_of(&art.text);
        let from = art.text.to_string();
        // "A" is Title for one letter; "AN" would be Upper
        let text = if case == Case::Upper && from.len() > 1 { replacement.to_uppercase() } else { apply_case(replacement, case) };
        self.toks[first - 2].text = Cow::Owned(text);
        self.changes.push(PendingChange { first: first - 2, last: first - 2, from, kind: ChangeKind::Article });
    }

    fn apply_spelling(&mut self, map: &Map, opts: &Options) {
        for i in 0..self.toks.len() {
            let t = &self.toks[i];
            if t.kind != Kind::Word || t.frozen || !t.text.is_ascii() || t.text.len() < 2 {
                continue;
            }
            let case = case_of(&t.text);
            if case == Case::Mixed {
                continue;
            }
            let key = lower(&t.text);
            let replacement: Option<Cow<'static, str>> = match map.get(key.as_ref()) {
                Some(r) => Some(Cow::Borrowed(*r)),
                None => PREFIXES.iter().find_map(|p| {
                    let rest = key.strip_prefix(p)?;
                    if rest.len() < 4 {
                        return None;
                    }
                    map.get(rest).map(|r| Cow::Owned(format!("{p}{r}")))
                }),
            };
            let Some(replacement) = replacement else { continue };
            if case == Case::Title && opts.protect_proper_nouns && !self.title_is_convertible(i, i) {
                continue;
            }
            let from = self.toks[i].text.to_string();
            self.toks[i].text = Cow::Owned(apply_case(&replacement, case));
            self.toks[i].frozen = true;
            self.changes.push(PendingChange { first: i, last: i, from, kind: ChangeKind::Spelling });
        }
    }

    fn finish(mut self) -> Conversion {
        let mut starts = Vec::with_capacity(self.toks.len() + 1);
        let mut text = String::with_capacity(self.toks.iter().map(|t| t.text.len()).sum::<usize>() + 16);
        for t in &self.toks {
            starts.push(text.len());
            text.push_str(&t.text);
        }
        starts.push(text.len());
        self.changes.sort_by_key(|c| c.first);
        let changes = self
            .changes
            .into_iter()
            .map(|c| {
                // a wholesale replacement leaves empty tokens after the first one
                let (start, end) = (starts[c.first], starts[c.last + 1]);
                Change { start, end, from: c.from, to: text[start..end].to_string(), kind: c.kind }
            })
            .collect();
        Conversion { text, changes }
    }
}

/// Does the indefinite article in front of `word` have to be "an"?
fn takes_an(word: &str) -> bool {
    let w = word.to_lowercase();
    if ["hour", "honest", "honour", "honor", "heir"].iter().any(|p| w.starts_with(p)) {
        return true;
    }
    if ["uni", "use", "usu", "uti", "eu", "one", "once"].iter().any(|p| w.starts_with(p)) {
        return false;
    }
    matches!(w.chars().next(), Some('a' | 'e' | 'i' | 'o' | 'u'))
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn tables_load() {
        let s = data_stats();
        assert!(s.british_to_american_pairs > 7000, "{s:?}");
        assert!(s.american_to_british_pairs > 7000, "{s:?}");
        assert!(s.rules_american_to_british > 100 && s.rules_british_to_american > 80, "{s:?}");
    }

    #[test]
    fn phrase_parser() {
        let (e, s) = parse_phrase("driver's license");
        assert_eq!(e, vec![Elem::Word("driver"), Elem::Poss, Elem::Word("license")]);
        assert_eq!(s, vec![0, 0, b' ']);
        let (e, s) = parse_phrase("#-story");
        assert_eq!(e, vec![Elem::Num, Elem::Word("story")]);
        assert_eq!(s, vec![0, b'-']);
    }

    #[test]
    fn case_classes() {
        assert_eq!(case_of("colour"), Case::Lower);
        assert_eq!(case_of("Colour"), Case::Title);
        assert_eq!(case_of("COLOUR"), Case::Upper);
        assert_eq!(case_of("iPhone"), Case::Mixed);
        assert_eq!(case_of("McColour"), Case::Mixed);
        assert_eq!(case_of("I"), Case::Title);
    }

    #[test]
    fn technical_chunks() {
        for s in [
            "https://example.com/colour", "www.colour.co.uk", "me@colour.org", "favourite_colour", "backgroundColor",
            "colour.txt", "--colour", "~/colour", "src/colour/mod.rs", "colour()", "#colour", "$colour", "h264",
            "key=colour", "<colour>", "R&D",
        ] {
            assert!(is_technical(s), "{s} should be technical");
        }
        for s in ["colour", "colour,", "(colour)", "\"colour\"", "colour/shade", "colour\u{2014}and", "*colour*", "$500", "50%"] {
            assert!(!is_technical(s), "{s} should not be technical");
        }
    }
}
