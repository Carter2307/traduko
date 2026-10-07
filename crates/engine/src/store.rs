//! Where the models are on disk, and what they can translate together.
//!
//! ```text
//! <models>/light/fr-en/     model.safetensors  config.json
//!          light/en-fr/     source.spm  target.spm  vocab.json
//!          light/es-en/     (the same five files in every folder)
//!          light/en-es/
//!          accurate/fr-en/
//!          accurate/en-fr/
//! ```
//!
//! A folder is one model: a direction, named by its two language codes, in
//! one of the two qualities. `scripts/fetch-models.sh` fills these folders,
//! and so does `install` when the app downloads them itself. `target.spm`
//! is not read by this crate (the output is decoded with `vocab.json`
//! alone); it is there so that each folder stays a complete Marian model
//! for other tools.
//!
//! Nothing here lists the languages: whatever folder is complete can be
//! used. Two languages without a model of their own meet in English, which
//! every language has a model to and from.

use std::path::{Path, PathBuf};

use crate::{Direction, Language, Quality};

/// What a model folder must hold.
pub(crate) const FILES: [&str; 5] = ["model.safetensors", "config.json", "source.spm", "target.spm", "vocab.json"];

const QUALITIES: [Quality; 2] = [Quality::Light, Quality::Accurate];

/// The language that two others are translated through.
const PIVOT: Language = Language::ENGLISH;

/// One model: a direction in one of the two qualities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Hop {
    pub quality: Quality,
    pub direction: Direction,
}

pub(crate) fn folder(models_dir: &Path, hop: Hop) -> PathBuf {
    models_dir.join(hop.quality.folder()).join(hop.direction.code())
}

/// True when every file of a model is there. The files are not opened: this
/// runs at start-up, and a damaged file is reported when it is loaded.
pub(crate) fn is_complete(folder: &Path) -> bool {
    FILES.iter().all(|file| folder.join(file).is_file())
}

/// The models found on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    models: Vec<Hop>,
}

impl Installed {
    /// Looks at the folders of `models_dir`. A folder with a file missing,
    /// or whose name is not a direction, is not a model.
    pub fn scan(models_dir: &Path) -> Self {
        let mut models = Vec::new();
        for quality in QUALITIES {
            let Ok(folders) = std::fs::read_dir(models_dir.join(quality.folder())) else { continue };
            for folder in folders.flatten() {
                let direction = folder.file_name().to_str().and_then(Direction::from_code);
                if let Some(direction) = direction.filter(|direction| direction.from != direction.to && is_complete(&folder.path())) {
                    models.push(Hop { quality, direction });
                }
            }
        }
        Self::of(models)
    }

    pub(crate) fn of(mut models: Vec<Hop>) -> Self {
        // The order of a folder listing is not a promise.
        models.sort_by_key(|hop| (hop.quality as u8, hop.direction.from, hop.direction.to));
        Self { models }
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// The qualities that have a model.
    pub fn qualities(&self) -> Vec<Quality> {
        QUALITIES.into_iter().filter(|quality| self.models.iter().any(|hop| hop.quality == *quality)).collect()
    }

    /// The qualities whose French and English set is whole: both ways.
    /// This is what `install` downloads for a quality.
    pub fn sets(&self) -> Vec<Quality> {
        let french = Direction::new(Language::FRENCH, Language::ENGLISH);
        let whole = |quality: &Quality| [french, french.swapped()].iter().all(|direction| self.has(*quality, *direction));
        QUALITIES.into_iter().filter(whole).collect()
    }

    pub(crate) fn has(&self, quality: Quality, direction: Direction) -> bool {
        self.models.contains(&Hop { quality, direction })
    }

    /// The languages that a model reads or writes, in the order of their
    /// codes.
    pub fn languages(&self) -> Vec<Language> {
        let mut languages: Vec<Language> = self.models.iter().flat_map(|hop| [hop.direction.from, hop.direction.to]).collect();
        languages.sort();
        languages.dedup();
        languages
    }

    /// True when a text can be taken along `direction`, with one model or
    /// through English.
    pub fn translates(&self, direction: Direction) -> bool {
        self.route(Quality::Light, direction).is_some()
    }

    /// True when `direction` has no model of its own and goes through
    /// English: two translations, each with its own mistakes.
    pub fn goes_through_english(&self, direction: Direction) -> bool {
        self.route(Quality::Light, direction).is_some_and(|route| route.len() > 1)
    }

    /// The models that take a text along `direction`, in order: one, or two
    /// through English. `None` when the models on disk cannot do it.
    ///
    /// `quality` is a wish. A direction that only exists in the other
    /// quality is served by that one: the small models cover more languages
    /// than the large ones, and a translation is better than a refusal.
    pub(crate) fn route(&self, quality: Quality, direction: Direction) -> Option<Vec<Hop>> {
        let model = |direction: Direction| {
            let other = QUALITIES.into_iter().filter(|other| *other != quality);
            std::iter::once(quality).chain(other).map(|quality| Hop { quality, direction }).find(|hop| self.models.contains(hop))
        };
        if direction.from == direction.to {
            return None;
        }
        if let Some(direct) = model(direction) {
            return Some(vec![direct]);
        }
        if direction.from == PIVOT || direction.to == PIVOT {
            return None;
        }
        Some(vec![model(Direction::new(direction.from, PIVOT))?, model(Direction::new(PIVOT, direction.to))?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Quality::{Accurate, Light};

    fn direction(code: &str) -> Direction {
        Direction::from_code(code).expect("a direction")
    }

    fn hop(quality: Quality, code: &str) -> Hop {
        Hop { quality, direction: direction(code) }
    }

    fn installed(models: &[(Quality, &str)]) -> Installed {
        Installed::of(models.iter().map(|(quality, code)| hop(*quality, code)).collect())
    }

    fn fill(folder: &Path, files: &[&str]) {
        std::fs::create_dir_all(folder).expect("create the model folder");
        for file in files {
            std::fs::write(folder.join(file), b"x").expect("write a model file");
        }
    }

    #[test]
    fn the_complete_folders_named_after_a_direction_are_the_models() {
        let models = std::env::temp_dir().join(format!("coco-store-{}", std::process::id()));
        assert!(Installed::scan(&models).is_empty());

        fill(&folder(&models, hop(Light, "fr-en")), &FILES);
        fill(&folder(&models, hop(Light, "es-en")), &FILES);
        fill(&folder(&models, hop(Accurate, "en-fr")), &FILES[..4]);
        fill(&models.join("light/notes"), &FILES);
        fill(&models.join("light/fr-fr"), &FILES);
        fill(&models.join("heavy/fr-en"), &FILES);
        assert_eq!(Installed::scan(&models), installed(&[(Light, "es-en"), (Light, "fr-en")]), "a missing file makes a model incomplete");

        fill(&folder(&models, hop(Accurate, "en-fr")), &FILES);
        let found = Installed::scan(&models);
        assert_eq!(found, installed(&[(Light, "es-en"), (Light, "fr-en"), (Accurate, "en-fr")]));
        assert_eq!(found.qualities(), [Light, Accurate]);
        assert_eq!(found.languages(), ["en", "es", "fr"].map(|code| Language::from_code(code).unwrap()));
        std::fs::remove_dir_all(models).ok();
    }

    #[test]
    fn the_folders_are_named_after_the_quality_and_the_direction() {
        assert_eq!(folder(Path::new("/models"), hop(Accurate, "en-fr")), Path::new("/models/accurate/en-fr"));
        assert_eq!(folder(Path::new("/models"), hop(Light, "de-en")), Path::new("/models/light/de-en"));
    }

    #[test]
    fn a_set_is_french_and_english_both_ways() {
        let one_way = installed(&[(Light, "fr-en"), (Accurate, "fr-en"), (Accurate, "en-fr"), (Light, "es-en"), (Light, "en-es")]);
        assert_eq!(one_way.sets(), [Accurate]);
        assert_eq!(one_way.qualities(), [Light, Accurate]);
    }

    #[test]
    fn a_direction_with_a_model_of_its_own_uses_it() {
        let models = installed(&[(Light, "fr-en"), (Accurate, "fr-en"), (Light, "en-fr")]);
        assert_eq!(models.route(Light, direction("fr-en")), Some(vec![hop(Light, "fr-en")]));
        assert_eq!(models.route(Accurate, direction("fr-en")), Some(vec![hop(Accurate, "fr-en")]));
        assert!(!models.goes_through_english(direction("fr-en")));
    }

    #[test]
    fn a_direction_that_the_asked_quality_lacks_is_served_by_the_other_one() {
        let models = installed(&[(Accurate, "fr-en"), (Light, "en-fr"), (Light, "es-en")]);
        assert_eq!(models.route(Accurate, direction("en-fr")), Some(vec![hop(Light, "en-fr")]));
        assert_eq!(models.route(Light, direction("fr-en")), Some(vec![hop(Accurate, "fr-en")]));
        assert_eq!(models.route(Accurate, direction("es-en")), Some(vec![hop(Light, "es-en")]));
    }

    #[test]
    fn two_languages_without_a_model_of_their_own_meet_in_english() {
        let models = installed(&[(Accurate, "fr-en"), (Light, "fr-en"), (Light, "en-de"), (Light, "de-en"), (Light, "en-fr")]);
        assert_eq!(models.route(Accurate, direction("fr-de")), Some(vec![hop(Accurate, "fr-en"), hop(Light, "en-de")]));
        assert_eq!(models.route(Light, direction("de-fr")), Some(vec![hop(Light, "de-en"), hop(Light, "en-fr")]));
        assert!(models.goes_through_english(direction("fr-de")));

        // A model for the direction itself is better than two.
        let direct = installed(&[(Light, "fr-en"), (Light, "en-de"), (Light, "fr-de")]);
        assert_eq!(direct.route(Accurate, direction("fr-de")), Some(vec![hop(Light, "fr-de")]));
    }

    #[test]
    fn what_the_models_cannot_do_has_no_route() {
        let models = installed(&[(Light, "fr-en"), (Light, "en-fr"), (Light, "es-en")]);
        // Nothing writes Spanish.
        assert_eq!(models.route(Light, direction("fr-es")), None);
        assert_eq!(models.route(Light, direction("en-es")), None);
        assert!(!models.translates(direction("en-es")));
        // A language to itself is not a translation, even through English.
        assert_eq!(models.route(Light, direction("fr-fr")), None);
        assert_eq!(models.route(Light, direction("en-en")), None);
        assert!(models.translates(direction("es-fr")));
        assert_eq!(Installed::default().route(Light, direction("fr-en")), None);
    }
}
