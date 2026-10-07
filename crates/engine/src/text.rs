//! Cuts a text into the sentences that are translated and everything
//! between them, which is copied.
//!
//! The models translate one sentence at a time and know nothing about line
//! breaks, indentation or list markers. So the text is taken apart first:
//! each line is a paragraph, its blanks and its marker ("- ", "2. ") are set
//! aside, and what is left is cut into sentences. Putting the translations
//! back between the untouched parts gives an output with the same shape as
//! the input.

use std::borrow::Cow;

/// A piece of the source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Part<'a> {
    /// Copied to the output: blanks, line breaks, list markers, and
    /// sentences with nothing to translate (numbers, a link).
    Keep(&'a str),
    Sentence(Sentence<'a>),
}

/// One sentence to translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sentence<'a> {
    /// What the model reads: the sentence with no blank around it and
    /// without its pictographs.
    words: Cow<'a, str>,
    /// The emoji and other pictographs of the sentence. The models drop
    /// them or write a stray dash in their place, so they go around the
    /// model and come back after the translation.
    pictographs: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Layout<'a> {
    parts: Vec<Part<'a>>,
}

impl<'a> Layout<'a> {
    pub fn of(text: &'a str) -> Self {
        let mut parts = Vec::new();
        for line in text.split_inclusive('\n') {
            let content = line.trim_end();
            let indent = blanks_at_start(content);
            let lead = indent + list_marker(&content[indent..]);
            push_kept(&line[..lead], &mut parts);
            push_sentences(&content[lead..], &mut parts);
            push_kept(&line[content.len()..], &mut parts);
        }
        Self { parts }
    }

    /// The sentences to translate, in order.
    pub fn sentences(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            Part::Sentence(sentence) => Some(sentence.words.as_ref()),
            Part::Keep(_) => None,
        })
    }

    /// The output once the first `translations.len()` sentences are
    /// translated. With all of them it has every part of the source; with
    /// fewer it stops right after the last translated sentence.
    pub fn render<T: AsRef<str>>(&self, translations: &[T]) -> String {
        let mut out = String::new();
        let mut translations = translations.iter();
        let mut complete_until = 0;
        for part in &self.parts {
            match part {
                Part::Keep(kept) => out.push_str(kept),
                Part::Sentence(sentence) => match translations.next() {
                    Some(translation) => {
                        out.push_str(translation.as_ref());
                        if !sentence.pictographs.is_empty() {
                            out.push(' ');
                            out.push_str(&sentence.pictographs);
                        }
                        complete_until = out.len();
                    }
                    None => {
                        out.truncate(complete_until);
                        return out;
                    }
                },
            }
        }
        out
    }
}

impl<'a> Sentence<'a> {
    /// Sets the pictographs of `sentence` aside. "Trop bien 🎉 !" is read by
    /// the model as "Trop bien !", and 🎉 follows its translation.
    fn of(sentence: &'a str) -> Self {
        if !sentence.contains(is_pictograph) {
            return Self { words: Cow::Borrowed(sentence), pictographs: String::new() };
        }
        let mut words = String::with_capacity(sentence.len());
        let mut pictographs = String::new();
        let mut in_run = false;
        for c in sentence.chars() {
            // A joiner or a variation selector belongs to the emoji before it.
            let in_this_run = is_pictograph(c) || (in_run && matches!(c, '\u{200D}' | '\u{FE0F}'));
            if in_this_run {
                if !in_run && !pictographs.is_empty() {
                    pictographs.push(' ');
                }
                pictographs.push(c);
            } else {
                // Where a run was, do not leave a blank before a comma or a
                // full stop, nor two blanks in a row.
                if in_run && matches!(c, ',' | '.') {
                    words.truncate(words.trim_end().len());
                }
                if !(c.is_whitespace() && words.ends_with(char::is_whitespace)) {
                    words.push(c);
                }
            }
            in_run = in_this_run;
        }
        Self { words: Cow::Owned(words.trim().to_string()), pictographs }
    }
}

/// Emoji, dingbats and the like: the blocks from playing cards to the
/// extended pictographs, and the older symbols and dingbats.
fn is_pictograph(c: char) -> bool {
    matches!(c, '\u{1F000}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}')
}

/// The characters that start a list item, a quotation or a heading.
const BULLETS: &str = "-*+•·‣◦▪●○■□–—>#";

/// The length of the list marker that starts `line`, with the blanks after
/// it; 0 when there is none. A marker is kept as it is: the models tend to
/// drop or rewrite it.
fn list_marker(line: &str) -> usize {
    let Some(marker) = bullet(line).or_else(|| numbering(line)) else { return 0 };
    let rest = &line[marker..];
    let blanks = blanks_at_start(rest);
    if blanks == 0 {
        // "-5 degrees" and "*bold*" are text; a marker alone on its line is
        // still a marker.
        return if rest.is_empty() { marker } else { 0 };
    }
    // A task box may follow a bullet: "- [ ] " or "- [x] ".
    let after_box = ["[ ]", "[x]", "[X]"].iter().find_map(|task_box| rest[blanks..].strip_prefix(task_box));
    match after_box.map(blanks_at_start) {
        Some(box_blanks) if box_blanks > 0 && bullet(line).is_some() => marker + blanks + 3 + box_blanks,
        _ => marker + blanks,
    }
}

/// "-", "•", ">", "##", "—": up to six bullet characters.
fn bullet(line: &str) -> Option<usize> {
    let rest = line.trim_start_matches(|c: char| BULLETS.contains(c));
    let count = line[..line.len() - rest.len()].chars().count();
    (1..=6).contains(&count).then_some(line.len() - rest.len())
}

/// "1.", "12)", "a)" and "(b)". Not "2024." (a year starts a sentence more
/// often than a list) and not "A." (more often an initial).
fn numbering(line: &str) -> Option<usize> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let mut chars = line.chars();
    match (chars.next()?, chars.next(), chars.next()) {
        _ if (1..=3).contains(&digits) && line[digits..].starts_with(['.', ')']) => Some(digits + 1),
        (letter, Some(')'), _) if letter.is_ascii_alphabetic() => Some(2),
        ('(', Some(inside), Some(')')) if inside.is_ascii_alphanumeric() => Some(3),
        _ => None,
    }
}

fn blanks_at_start(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

/// Cuts a line (without blanks at its ends) into sentences and the blanks
/// between them.
fn push_sentences<'a>(line: &'a str, parts: &mut Vec<Part<'a>>) {
    let mut start = 0;
    for (end, next) in sentence_breaks(line) {
        let sentence = &line[start..end];
        if needs_translation(sentence) {
            parts.push(Part::Sentence(Sentence::of(sentence)));
        } else {
            push_kept(sentence, parts);
        }
        push_kept(&line[end..next], parts);
        start = next;
    }
}

fn push_kept<'a>(kept: &'a str, parts: &mut Vec<Part<'a>>) {
    if !kept.is_empty() {
        parts.push(Part::Keep(kept));
    }
}

/// False for what a model can only damage: text without a single letter
/// ("12:30", "---", an emoji) and a lone link or address.
fn needs_translation(sentence: &str) -> bool {
    let one_word = !sentence.contains(char::is_whitespace);
    let link = sentence.contains("://") || sentence.starts_with("www.") || sentence.contains('@');
    sentence.contains(char::is_alphabetic) && !(one_word && link)
}

/// Where each sentence of `line` ends and where the next one starts, in
/// bytes. The last pair is always the end of the line.
fn sentence_breaks(line: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let offset = |index: usize| chars.get(index).map_or(line.len(), |(at, _)| *at);
    let mut breaks = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let (at, c) = chars[index];
        index += 1;
        if !is_end_mark(c) {
            continue;
        }
        // Take "?!" and "..." as one mark, then the quotes and brackets that
        // close with the sentence.
        let run_start = index;
        while chars.get(index).is_some_and(|(_, c)| is_end_mark(*c)) {
            index += 1;
        }
        let lone_dot = c == '.' && index == run_start;
        while chars.get(index).is_some_and(|(_, c)| is_closer(*c)) {
            index += 1;
        }
        let end = index;
        while chars.get(index).is_some_and(|(_, c)| c.is_whitespace()) {
            index += 1;
        }
        // A sentence ends before a blank and a new start. "3.5", "example.com"
        // and "etc. and" go on.
        let Some((_, next)) = chars.get(index).copied().filter(|_| index > end) else { continue };
        if starts_sentence(next) && !(lone_dot && is_abbreviation(&line[..at], next)) {
            breaks.push((offset(end), offset(index)));
        }
    }
    breaks.push((line.len(), line.len()));
    breaks
}

fn is_end_mark(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '…')
}

fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | '’' | '”' | '»' | ')' | ']')
}

fn starts_sentence(c: char) -> bool {
    c.is_uppercase() || c.is_numeric() || matches!(c, '"' | '“' | '«' | '(' | '[' | '—' | '–' | '-' | '¿' | '¡' | '\'' | '‘')
}

/// Short forms that are followed by a name, so a capital after them does
/// not start a sentence. Only with their own capital: "Col. Moutarde" but
/// "il a franchi le col."
const TITLES: &[&str] = &[
    "m", "mm", "mr", "mrs", "ms", "mme", "mmes", "mlle", "mlles", "dr", "drs", "pr", "prof", "st", "ste", "sr", "jr",
    "mgr", "gen", "col", "lt", "sgt", "capt", "rev", "hon", "mt",
];

/// Short forms that are only short forms before a number: "p. 12", "art. 5",
/// "No. 3". Before anything else they are words that end a sentence.
const BEFORE_NUMBERS: &[&str] =
    &["n", "no", "nos", "p", "pp", "art", "fig", "vol", "chap", "tél", "tel", "env", "approx"];

/// Short forms that never end a sentence on their own.
const ALWAYS: &[&str] = &["cf", "vs", "e.g", "i.e", "u.s", "u.k", "u.s.a", "inc", "ltd", "corp", "av", "apr", "bd"];

/// True when the full stop after `before` belongs to a short form or to an
/// initial. `next` is the first character after the blank that follows.
fn is_abbreviation(before: &str, next: char) -> bool {
    let is_word_start = |c: char| c.is_whitespace() || matches!(c, '(' | '[' | '«' | '"' | '“' | '\'' | '’' | '‘' | '-');
    let word = before.rfind(is_word_start).map_or(before, |at| {
        let separator = before[at..].chars().next().map_or(1, char::len_utf8);
        &before[at + separator..]
    });
    let mut letters = word.chars();
    let Some(first) = letters.next() else { return false };
    // "J. K. Rowling".
    if first.is_uppercase() && letters.next().is_none() {
        return true;
    }
    let lower = word.to_lowercase();
    ALWAYS.contains(&lower.as_str())
        || (first.is_uppercase() && TITLES.contains(&lower.as_str()))
        || (next.is_numeric() && BEFORE_NUMBERS.contains(&lower.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Part::Keep;

    /// A sentence without pictographs, as a part.
    #[allow(non_snake_case)]
    fn Sentence(words: &str) -> Part<'_> {
        Part::Sentence(super::Sentence { words: Cow::Borrowed(words), pictographs: String::new() })
    }

    fn sentences(text: &str) -> Vec<String> {
        Layout::of(text).sentences().map(str::to_string).collect()
    }

    /// Translating every sentence to itself must give the text back.
    fn assert_round_trip(text: &str) {
        let layout = Layout::of(text);
        let same: Vec<&str> = layout.sentences().collect();
        assert_eq!(layout.render(&same), text);
    }

    #[test]
    fn every_part_of_the_text_comes_back() {
        for text in [
            "",
            "   ",
            "\n\n",
            "Merci",
            "  Hello.\n\nSecond paragraph!  Third?   ",
            "One.\r\nTwo\r\n\r\nThree.",
            "\tIndented line.\n\t\tDeeper. Still deeper.\n",
            "- un\n- deux\n  * trois\n\n1. quatre\n2) cinq\n",
            "Vraiment ?! Oui... Bon.",
            "«\u{a0}Viens\u{a0}!\u{a0}» dit-il. Elle rit.",
            "Voir https://example.com/a.b?c=d. Merci !",
            "混合 text avec des mots. Конец.",
        ] {
            assert_round_trip(text);
        }
    }

    #[test]
    fn a_paragraph_is_cut_into_its_sentences() {
        assert_eq!(
            Layout::of("  Hello there.  How are you?\n").parts,
            [Keep("  "), Sentence("Hello there."), Keep("  "), Sentence("How are you?"), Keep("\n")]
        );
    }

    #[test]
    fn each_line_is_a_paragraph_and_blank_lines_stay() {
        assert_eq!(
            Layout::of("Titre\n\nUn. Deux.\n").parts,
            [Sentence("Titre"), Keep("\n"), Keep("\n"), Sentence("Un."), Keep(" "), Sentence("Deux."), Keep("\n")]
        );
    }

    #[test]
    fn list_markers_are_kept_out_of_the_sentences() {
        assert_eq!(sentences("- Acheter du pain\n  • Appeler Marie\n3. Dormir\nb) Lire\n(c) Partir"), [
            "Acheter du pain",
            "Appeler Marie",
            "Dormir",
            "Lire",
            "Partir"
        ]);
        assert_eq!(sentences("- [ ] Ranger\n- [x] Payer\n> Cité\n## Titre\n— Bonjour, dit-il."), [
            "Ranger",
            "Payer",
            "Cité",
            "Titre",
            "Bonjour, dit-il."
        ]);
        assert_eq!(Layout::of("- [ ] Ranger").parts, [Keep("- [ ] "), Sentence("Ranger")]);
        assert_eq!(Layout::of(" -\n1.").parts, [Keep(" -"), Keep("\n"), Keep("1.")]);
    }

    #[test]
    fn what_only_looks_like_a_marker_stays_in_the_sentence() {
        assert_eq!(sentences("-5 degrés ce matin."), ["-5 degrés ce matin."]);
        assert_eq!(Layout::of("2024. Une année.").parts, [Keep("2024."), Keep(" "), Sentence("Une année.")]);
        assert_eq!(sentences("*Important* : lisez ceci."), ["*Important* : lisez ceci."]);
        assert_eq!(sentences("A. Dupont est venu."), ["A. Dupont est venu."]);
        assert_eq!(sentences("[ ] seul"), ["[ ] seul"]);
    }

    #[test]
    fn short_forms_and_numbers_do_not_end_a_sentence() {
        assert_eq!(sentences("M. Dupont est arrivé à 18h30. Il a vu le Dr. Martin."), [
            "M. Dupont est arrivé à 18h30.",
            "Il a vu le Dr. Martin."
        ]);
        assert_eq!(sentences("It costs 3.5 euros. Cheap!"), ["It costs 3.5 euros.", "Cheap!"]);
        assert_eq!(sentences("Let's meet at 6:30 p.m. at the station."), ["Let's meet at 6:30 p.m. at the station."]);
        assert_eq!(sentences("See p. 12 and art. 5. Then fig. 3."), ["See p. 12 and art. 5.", "Then fig. 3."]);
        assert_eq!(sentences("J. K. Rowling wrote it. Cf. Tolkien."), ["J. K. Rowling wrote it.", "Cf. Tolkien."]);
        assert_eq!(sentences("Visit www.example.com. Thanks"), ["Visit www.example.com.", "Thanks"]);
    }

    #[test]
    fn a_word_that_is_also_a_short_form_still_ends_a_sentence() {
        assert_eq!(sentences("Il a franchi le col. Puis il est rentré."), ["Il a franchi le col.", "Puis il est rentré."]);
        assert_eq!(sentences("I said no. Then he left."), ["I said no.", "Then he left."]);
        assert_eq!(sentences("Des pommes, des poires, etc. Il a tout pris."), ["Des pommes, des poires, etc.", "Il a tout pris."]);
    }

    #[test]
    fn marks_come_in_runs_and_quotes_close_with_the_sentence() {
        assert_eq!(sentences("Vraiment ?! Oui... Bon."), ["Vraiment ?!", "Oui...", "Bon."]);
        assert_eq!(sentences("« Viens ! » dit-il. Elle rit."), ["« Viens ! » dit-il.", "Elle rit."]);
        assert_eq!(sentences("He said \"Stop.\" Then he left."), ["He said \"Stop.\"", "Then he left."]);
        assert_eq!(sentences("ok. fine. whatever"), ["ok. fine. whatever"]);
    }

    #[test]
    fn text_without_letters_and_lone_links_are_copied_not_translated() {
        assert_eq!(Layout::of("12:30\nhttps://example.com/x\n😀\n---").sentences().count(), 0);
        assert_eq!(sentences("Voir https://example.com pour la suite."), ["Voir https://example.com pour la suite."]);
        assert_round_trip("12:30\nhttps://example.com/x\n😀\n---");
    }

    #[test]
    fn a_partial_output_stops_after_the_last_translated_sentence() {
        let layout = Layout::of("  - Un. Deux.\n  - Trois.\n");
        assert_eq!(layout.render::<&str>(&[]), "");
        assert_eq!(layout.render(&["One."]), "  - One.");
        assert_eq!(layout.render(&["One.", "Two."]), "  - One. Two.");
        assert_eq!(layout.render(&["One.", "Two.", "Three."]), "  - One. Two.\n  - Three.\n");
    }

    #[test]
    fn a_very_long_text_is_handled() {
        let text = "Une phrase assez courte. ".repeat(800);
        assert_eq!(text.len(), 20_000);
        assert_eq!(sentences(&text).len(), 800);
        assert_round_trip(&text);
        assert_round_trip(&"x".repeat(20_000));
        assert_round_trip(&"é\u{200d}".repeat(3000));
        assert_eq!(sentences(&"é😀\u{200d}".repeat(3000)), ["é".repeat(3000)]);
    }

    #[test]
    fn pictographs_go_around_the_model_and_follow_the_translation() {
        let layout = Layout::of("J'adore ce film 😀 ! On se voit demain 🎉🎉 ?\n👍 Super 👨\u{200d}👩\u{200d}👧, merci ❤\u{fe0f}");
        let words: Vec<&str> = layout.sentences().collect();
        assert_eq!(words, ["J'adore ce film !", "On se voit demain ?", "Super, merci"]);
        let translated = layout.render(&["I love this movie!", "See you tomorrow?", "Great, thanks"]);
        assert_eq!(translated, "I love this movie! 😀 See you tomorrow? 🎉🎉\nGreat, thanks 👍 👨\u{200d}👩\u{200d}👧 ❤\u{fe0f}");
        // The same words with other emoji are the same sentence for the cache.
        assert_eq!(sentences("On se voit demain 😴 ?"), ["On se voit demain ?"]);
    }
}
