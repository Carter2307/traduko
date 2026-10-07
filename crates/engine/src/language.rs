//! The languages, and the way from one to another.
//!
//! A language is its two-letter code and nothing else: the engine has no
//! list of the ones it knows. What Traduko can translate is what the model
//! folders on disk say (`store`), so a language is added by installing its
//! models, not by changing this crate. The table here only gives names to
//! the codes, for the UI and for messages.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A language, by its ISO 639-1 code: "fr", "en".
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Language([u8; 2]);

/// Code, name in English, and the name the language gives itself.
const NAMES: [(&str, &str, &str); 26] = [
    ("ar", "Arabic", "العربية"),
    ("cs", "Czech", "Čeština"),
    ("da", "Danish", "Dansk"),
    ("de", "German", "Deutsch"),
    ("el", "Greek", "Ελληνικά"),
    ("en", "English", "English"),
    ("es", "Spanish", "Español"),
    ("fi", "Finnish", "Suomi"),
    ("fr", "French", "Français"),
    ("he", "Hebrew", "עברית"),
    ("hi", "Hindi", "हिन्दी"),
    ("hu", "Hungarian", "Magyar"),
    ("id", "Indonesian", "Bahasa Indonesia"),
    ("it", "Italian", "Italiano"),
    ("ja", "Japanese", "日本語"),
    ("ko", "Korean", "한국어"),
    ("nl", "Dutch", "Nederlands"),
    ("pl", "Polish", "Polski"),
    ("pt", "Portuguese", "Português"),
    ("ro", "Romanian", "Română"),
    ("ru", "Russian", "Русский"),
    ("sv", "Swedish", "Svenska"),
    ("tr", "Turkish", "Türkçe"),
    ("uk", "Ukrainian", "Українська"),
    ("vi", "Vietnamese", "Tiếng Việt"),
    ("zh", "Chinese", "中文"),
];

impl Language {
    pub const ENGLISH: Self = Self(*b"en");
    pub const FRENCH: Self = Self(*b"fr");

    /// For a table written in the source: `Language::of(b"es")`. A code
    /// that is not two lowercase letters does not compile there.
    pub(crate) const fn of(code: &[u8; 2]) -> Self {
        assert!(code[0].is_ascii_lowercase() && code[1].is_ascii_lowercase(), "a language code is two lowercase letters");
        Self(*code)
    }

    /// The language of a code, which is two lowercase letters. Any such
    /// code is accepted: see the top of this file.
    pub fn from_code(code: &str) -> Option<Self> {
        let letters = <[u8; 2]>::try_from(code.as_bytes()).ok()?;
        letters.iter().all(u8::is_ascii_lowercase).then_some(Self(letters))
    }

    pub fn code(&self) -> &str {
        // Two ASCII letters, by construction.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Its name in English, or its code when the table has no name for it.
    pub fn name(&self) -> &str {
        self.names().map_or(self.code(), |(_, name, _)| name)
    }

    /// The name it gives itself: "Français".
    pub fn native_name(&self) -> &str {
        self.names().map_or(self.code(), |(_, _, native)| native)
    }

    fn names(&self) -> Option<(&'static str, &'static str, &'static str)> {
        NAMES.into_iter().find(|(code, _, _)| *code == self.code())
    }
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Written as its code, in the settings file.
impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Self::from_code(&code).ok_or_else(|| serde::de::Error::custom(format!("{code:?} is not a language code")))
    }
}

/// From one language to another.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Direction {
    pub from: Language,
    pub to: Language,
}

impl Direction {
    pub const fn new(from: Language, to: Language) -> Self {
        Self { from, to }
    }

    pub const fn swapped(self) -> Self {
        Self { from: self.to, to: self.from }
    }

    /// The direction of a model folder's name: "fr-en".
    pub fn from_code(code: &str) -> Option<Self> {
        let (from, to) = code.split_once('-')?;
        Some(Self { from: Language::from_code(from)?, to: Language::from_code(to)? })
    }

    /// "fr-en": the name of its model folder.
    pub fn code(&self) -> String {
        format!("{}-{}", self.from.code(), self.to.code())
    }
}

impl fmt::Debug for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.from.code(), self.to.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_is_two_lowercase_letters() {
        assert_eq!(Language::from_code("fr"), Some(Language::FRENCH));
        assert_eq!(Language::from_code("en").map(|language| language.code().to_string()), Some("en".to_string()));
        for code in ["", "f", "fra", "FR", "f1", "é", "f-"] {
            assert_eq!(Language::from_code(code), None, "{code:?}");
        }
    }

    #[test]
    fn a_language_without_a_name_goes_by_its_code() {
        assert_eq!((Language::FRENCH.name(), Language::FRENCH.native_name()), ("French", "Français"));
        let unnamed = Language::from_code("xx").unwrap();
        assert_eq!((unnamed.name(), unnamed.native_name()), ("xx", "xx"));
    }

    #[test]
    fn the_names_are_in_the_order_of_their_codes_and_none_comes_twice() {
        assert!(NAMES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(NAMES.iter().all(|(code, _, _)| Language::from_code(code).is_some()));
    }

    #[test]
    fn a_direction_is_named_like_its_folder() {
        let direction = Direction::from_code("fr-en").unwrap();
        assert_eq!(direction, Direction::new(Language::FRENCH, Language::ENGLISH));
        assert_eq!(direction.swapped().code(), "en-fr");
        for code in ["fr", "fr-", "fr-en-de", "fr_en", "FR-EN", ".."] {
            assert_eq!(Direction::from_code(code), None, "{code:?}");
        }
    }

    #[test]
    fn a_language_is_saved_as_its_code() {
        assert_eq!(serde_json::to_string(&Language::FRENCH).unwrap(), r#""fr""#);
        assert_eq!(serde_json::from_str::<Language>(r#""de""#).unwrap().name(), "German");
        assert!(serde_json::from_str::<Language>(r#""german""#).is_err());
    }
}
