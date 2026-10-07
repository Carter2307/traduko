//! Remembers the sentences already translated.
//!
//! The UI asks again for the whole text after each pause in typing. With
//! this, only the sentence that changed goes through the model; the others
//! are looked up. It also makes going back to an earlier wording free.
//!
//! An entry is what one model made of one sentence. A translation through
//! English is two entries, and its English half serves every other language
//! that the same sentence is translated to.

use std::collections::HashMap;

use crate::store::Hop;

/// About 400 kB of short sentences: an afternoon of typing.
pub(crate) const CAPACITY: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    model: Hop,
    sentence: String,
}

struct Entry {
    /// What the model gave, before any change of English spelling: the same
    /// entry serves American and British requests.
    translation: String,
    last_used: u64,
}

/// A bounded cache that forgets the entry that was used the longest ago.
pub(crate) struct SentenceCache {
    entries: HashMap<Key, Entry>,
    capacity: usize,
    /// Counts the uses, to order the entries by age.
    clock: u64,
}

impl SentenceCache {
    pub fn new(capacity: usize) -> Self {
        Self { entries: HashMap::new(), capacity, clock: 0 }
    }

    pub fn get(&mut self, model: Hop, sentence: &str) -> Option<String> {
        self.clock += 1;
        let entry = self.entries.get_mut(&Key { model, sentence: sentence.to_string() })?;
        entry.last_used = self.clock;
        Some(entry.translation.clone())
    }

    pub fn put(&mut self, model: Hop, sentence: &str, translation: &str) {
        self.clock += 1;
        let key = Key { model, sentence: sentence.to_string() };
        self.entries.insert(key, Entry { translation: translation.to_string(), last_used: self.clock });
        if self.entries.len() > self.capacity {
            // A scan of the whole cache, once per new sentence when it is
            // full: microseconds next to the translation that came before.
            let oldest = self.entries.iter().min_by_key(|(_, entry)| entry.last_used).map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Direction, Quality};

    fn model(quality: Quality, direction: &str) -> Hop {
        Hop { quality, direction: Direction::from_code(direction).expect("a direction") }
    }

    #[test]
    fn a_translation_is_found_again_under_the_same_model_only() {
        let mut cache = SentenceCache::new(10);
        let light = model(Quality::Light, "fr-en");
        cache.put(light, "Bonjour.", "Hello.");
        assert_eq!(cache.get(light, "Bonjour."), Some("Hello.".to_string()));
        assert_eq!(cache.get(model(Quality::Accurate, "fr-en"), "Bonjour."), None);
        assert_eq!(cache.get(model(Quality::Light, "en-fr"), "Bonjour."), None);
        assert_eq!(cache.get(model(Quality::Light, "fr-de"), "Bonjour."), None);
        assert_eq!(cache.get(light, "Bonjour"), None);
    }

    #[test]
    fn when_full_it_forgets_the_entry_used_the_longest_ago() {
        let mut cache = SentenceCache::new(2);
        let light = model(Quality::Light, "fr-en");
        cache.put(light, "un", "one");
        cache.put(light, "deux", "two");
        // Reading "un" makes "deux" the oldest.
        assert!(cache.get(light, "un").is_some());
        cache.put(light, "trois", "three");
        assert_eq!(cache.get(light, "deux"), None);
        assert!(cache.get(light, "un").is_some());
        assert!(cache.get(light, "trois").is_some());
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn a_new_translation_replaces_the_old_one() {
        let mut cache = SentenceCache::new(2);
        let light = model(Quality::Light, "fr-en");
        cache.put(light, "un", "one");
        cache.put(light, "un", "a");
        assert_eq!(cache.get(light, "un"), Some("a".to_string()));
        assert_eq!(cache.entries.len(), 1);
    }
}
