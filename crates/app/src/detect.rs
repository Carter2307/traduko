//! Tells which language a text is written in, so that Traduko can pick the
//! direction by itself. It only answers when the text leaves little doubt:
//! a wrong guess would flip the languages under the user's hands.
//!
//! The judgment is the one of macOS: the language recognizer of the
//! NaturalLanguage framework, which works offline and knows the languages
//! Traduko has models for. It is asked twice. Once among all the languages it
//! knows, to hear when the text is in one that Traduko cannot translate: asked
//! to choose between French and English, it would call Italian French. Then
//! among the languages that are installed, which is where it is good with a
//! few words.

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSString};
use objc2_natural_language::NLLanguageRecognizer;
use traduko_engine::Language;

/// How likely the recognizer must find a language to be believed.
const SURE: f64 = 0.9;

/// Enough for a few words when one of them is an everyday word of that
/// language: "Hello" alone is no more than this.
const LIKELY: f64 = 0.5;

/// A text of that many words at most can lean on an everyday word.
const FEW_WORDS: usize = 3;

/// A long text says what it is in its first lines.
const ENOUGH: usize = 600;

/// Words that people type first and that belong to one language. Words that
/// are spelt the same in a neighbour are left out on purpose: "no", "si",
/// "table", "comment", "die", "was".
fn everyday_words(language: Language) -> &'static [&'static str] {
    match language.code() {
        "en" => &[
            "hello", "hi", "hey", "thanks", "thank", "sorry", "please", "goodbye", "bye", "yes", "good", "morning", "evening", "night",
            "welcome", "the", "you", "what", "how", "why", "where", "okay",
        ],
        "fr" => &[
            "bonjour", "bonsoir", "salut", "merci", "oui", "pardon", "désolé", "désolée", "voilà", "bienvenue", "bonne", "nuit", "beaucoup",
            "pourquoi", "aujourd'hui", "demain", "où", "très", "avec", "vous", "nous", "c'est",
        ],
        "es" => &[
            "hola", "gracias", "adiós", "buenos", "buenas", "días", "tardes", "noches", "perdón", "favor", "sí", "bienvenido", "bienvenida",
            "mañana", "hoy", "dónde", "qué", "cómo", "muy", "pero", "también", "usted",
        ],
        "de" => &[
            "hallo", "danke", "bitte", "tschüss", "guten", "morgen", "abend", "nacht", "entschuldigung", "nein", "willkommen", "heute", "wie",
            "warum", "nicht", "und", "ich", "sehr",
        ],
        _ => &[],
    }
}

/// The language of `text` among `candidates`, or `None` when it is not
/// clear enough, or when it is not one of them.
pub fn language(text: &str, candidates: &[Language]) -> Option<Language> {
    let words = words(text);
    if words.is_empty() || candidates.is_empty() {
        return None;
    }
    let text = words.join(" ");

    // In a language that is not installed: nothing to pick.
    if let Some((anything, likelihood)) = likeliest(&text, &[])
        && likelihood >= SURE
        && !candidates.contains(&anything)
    {
        return None;
    }

    let (language, likelihood) = likeliest(&text, candidates)?;
    let everyday = || {
        let known = everyday_words(language);
        words.iter().any(|word| known.contains(&word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric())))
    };
    let clear = likelihood >= SURE || (likelihood >= LIKELY && words.len() <= FEW_WORDS && everyday());
    clear.then_some(language)
}

/// The words of the start of `text`, without what belongs to no language:
/// numbers, links, addresses.
fn words(text: &str) -> Vec<&str> {
    let is_word = |word: &&str| {
        let link = word.contains("://") || word.starts_with("www.") || word.contains('@');
        word.contains(char::is_alphabetic) && !link
    };
    let start = text.char_indices().nth(ENOUGH).map_or(text, |(end, _)| &text[..end]);
    start.split_whitespace().filter(is_word).collect()
}

/// What the recognizer calls a language. Chinese is two of its languages,
/// by the way it is written.
fn tags(language: Language) -> Vec<Retained<NSString>> {
    match language.code() {
        "zh" => vec![NSString::from_str("zh-Hans"), NSString::from_str("zh-Hant")],
        code => vec![NSString::from_str(code)],
    }
}

thread_local! {
    /// One per thread: it is not made to be shared, and it is slow to make.
    static RECOGNIZER: Retained<NLLanguageRecognizer> = unsafe { NLLanguageRecognizer::new() };
}

/// The language that `text` is most likely in, and how likely, from 0 to 1.
/// `among` narrows the answer to some languages; empty, it does not.
fn likeliest(text: &str, among: &[Language]) -> Option<(Language, f64)> {
    let constraints: Vec<Retained<NSString>> = among.iter().flat_map(|language| tags(*language)).collect();
    RECOGNIZER.with(|recognizer| {
        // SAFETY: the recognizer stays on the thread that made it, and it is
        // given strings where it expects strings.
        let guesses = unsafe {
            recognizer.reset();
            recognizer.setLanguageConstraints(&NSArray::from_retained_slice(&constraints));
            recognizer.processString(&NSString::from_str(text));
            recognizer.languageHypothesesWithMaximum(1)
        };
        let (tags, likelihoods) = guesses.to_vecs();
        let (tag, likelihood) = (tags.first()?.to_string(), likelihoods.first()?.doubleValue());
        // "zh-Hans" is "zh" here.
        let language = Language::from_code(tag.split('-').next()?)?;
        Some((language, likelihood))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages(codes: &[&str]) -> Vec<Language> {
        codes.iter().map(|code| Language::from_code(code).expect("a language code")).collect()
    }

    /// The code of the language found among French, English, Spanish and
    /// German.
    fn found(text: &str) -> Option<String> {
        language(text, &languages(&["fr", "en", "es", "de"])).map(|language| language.code().to_string())
    }

    #[test]
    fn plain_sentences_are_recognised() {
        for (text, code) in [
            ("Le camion est garé devant l'immeuble.", "fr"),
            ("The lorry is parked in front of the building.", "en"),
            ("Je voudrais réserver une table pour deux.", "fr"),
            ("I'd like to book a table for two.", "en"),
            ("On se retrouve à la gare à 18h30 ?", "fr"),
            ("Let's meet at the station at 6:30 pm.", "en"),
            ("Comment allez-vous ?", "fr"),
            ("Pourriez-vous m'envoyer le rapport ?", "fr"),
            ("¿Dónde está la estación de tren?", "es"),
            ("Me gustaría reservar una mesa para dos.", "es"),
            ("Wo ist der Bahnhof?", "de"),
            ("Ich möchte einen Tisch für zwei reservieren.", "de"),
        ] {
            assert_eq!(found(text).as_deref(), Some(code), "{text:?}");
        }
    }

    #[test]
    fn one_sure_word_is_enough_for_a_short_text() {
        for (text, code) in [
            ("Bonjour", "fr"),
            ("merci beaucoup", "fr"),
            ("Bonne nuit", "fr"),
            ("Hello", "en"),
            ("thanks", "en"),
            ("Good morning", "en"),
            ("Sorry", "en"),
            ("Hola", "es"),
            ("gracias", "es"),
            ("Buenos días", "es"),
            ("Hallo", "de"),
            ("danke", "de"),
            ("Guten Morgen", "de"),
        ] {
            assert_eq!(found(text).as_deref(), Some(code), "{text:?}");
        }
    }

    #[test]
    fn it_stays_silent_when_in_doubt() {
        for text in ["", "   ", "table", "Paris 2026", "taxi", "12:30", "😀", "https://example.com/page", "jean.dupont@exemple.fr"] {
            assert_eq!(found(text), None, "{text:?}");
        }
    }

    #[test]
    fn english_with_words_that_look_french_stays_english() {
        // Words shared by both languages, and acronyms, must not flip it.
        for text in ["AI tools", "Pour over coffee", "No comment", "Surplus plus tax", "I live in LA", "The UN said so"] {
            assert_ne!(found(text).as_deref(), Some("fr"), "{text:?} was taken for French");
        }
        // A French name in an English sentence.
        assert_eq!(found("I moved to Orléans three years ago and I never left.").as_deref(), Some("en"));
    }

    #[test]
    fn a_language_that_is_not_installed_is_not_taken_for_one_that_is() {
        for text in [
            "Vorrei prenotare un tavolo per due.",
            "Eu gostaria de reservar uma mesa.",
            "Ik wil graag een tafel reserveren.",
            "Привет, как дела?",
            "こんにちは、元気ですか",
        ] {
            assert_eq!(found(text), None, "{text:?}");
        }
        // The same sentences when their language is there.
        let more = languages(&["fr", "en", "it", "ru"]);
        assert_eq!(language("Vorrei prenotare un tavolo per due.", &more), Language::from_code("it"));
        assert_eq!(language("Привет, как дела?", &more), Language::from_code("ru"));
    }

    #[test]
    fn only_the_candidates_are_ever_answered() {
        let french_and_english = languages(&["fr", "en"]);
        assert_eq!(language("Wo ist der Bahnhof?", &french_and_english), None);
        assert_eq!(language("Bonjour tout le monde", &french_and_english), Some(Language::FRENCH));
        assert_eq!(language("Bonjour tout le monde", &[]), None);
    }

    #[test]
    fn a_link_or_a_number_in_a_sentence_does_not_hide_its_language() {
        assert_eq!(found("Écris-moi à jean.dupont@exemple.fr ou va sur https://exemple.fr/contact.").as_deref(), Some("fr"));
        assert_eq!(found(&"Voici une phrase assez courte. ".repeat(500)).as_deref(), Some("fr"));
    }
}
