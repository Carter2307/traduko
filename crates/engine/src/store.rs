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
    size(folder).is_some()
}

/// What the files of a model take together, in bytes, when every one of
/// them is there.
fn size(folder: &Path) -> Option<u64> {
    let file = |file: &&str| std::fs::metadata(folder.join(file)).ok().filter(|found| found.is_file()).map(|found| found.len());
    FILES.iter().map(file).sum()
}

/// The models found on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    models: Vec<Hop>,
    /// What each of them takes on disk, in bytes, in the same order.
    bytes: Vec<u64>,
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
                if let Some(direction) = direction.filter(|direction| direction.from != direction.to)
                    && let Some(bytes) = size(&folder.path())
                {
                    models.push((Hop { quality, direction }, bytes));
                }
            }
        }
        Self::weighing(models)
    }

    /// These models, without a size: for what does not look at the disk.
    #[cfg(test)]
    pub(crate) fn of(models: Vec<Hop>) -> Self {
        Self::weighing(models.into_iter().map(|hop| (hop, 0)).collect())
    }

    fn weighing(mut models: Vec<(Hop, u64)>) -> Self {
        // The order of a folder listing is not a promise.
        models.sort_by_key(|(hop, _)| (hop.quality as u8, hop.direction.from, hop.direction.to));
        let (models, bytes) = models.into_iter().unzip();
        Self { models, bytes }
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

    /// The models that are on disk for nothing while `quality` is the one
    /// asked for, and what each takes there: those of the other quality
    /// whose direction `quality` has too. No route takes them: see
    /// [`Installed::route`].
    pub(crate) fn unused(&self, quality: Quality) -> impl Iterator<Item = (Hop, u64)> + '_ {
        let weighed = self.models.iter().copied().zip(self.bytes.iter().copied());
        weighed.filter(move |(hop, _)| hop.quality != quality && self.has(quality, hop.direction))
    }

    /// What the models that nothing uses take on disk, in bytes: what
    /// [`remove_unused`](crate::remove_unused) frees.
    pub fn unused_size(&self, quality: Quality) -> u64 {
        self.unused(quality).map(|(_, bytes)| bytes).sum()
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
        assert_eq!(Installed::scan(&models).models, [hop(Light, "es-en"), hop(Light, "fr-en")], "a missing file makes a model incomplete");

        fill(&folder(&models, hop(Accurate, "en-fr")), &FILES);
        let found = Installed::scan(&models);
        assert_eq!(found.models, [hop(Light, "es-en"), hop(Light, "fr-en"), hop(Accurate, "en-fr")]);
        // One byte in each of the five files.
        assert_eq!(found.bytes, [5, 5, 5]);
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
    fn a_model_whose_direction_the_asked_quality_has_too_is_unused() {
        let models = installed(&[(Light, "fr-en"), (Light, "en-fr"), (Accurate, "fr-en"), (Accurate, "en-fr"), (Light, "de-en"), (Light, "en-de")]);
        let unused = |quality| models.unused(quality).map(|(hop, _)| hop).collect::<Vec<_>>();
        assert_eq!(unused(Accurate), [hop(Light, "en-fr"), hop(Light, "fr-en")]);
        assert_eq!(unused(Light), [hop(Accurate, "en-fr"), hop(Accurate, "fr-en")]);

        // Half a set is still needed for the direction that the other lacks.
        let half = installed(&[(Light, "fr-en"), (Light, "en-fr"), (Accurate, "fr-en")]);
        assert_eq!(half.unused(Accurate).map(|(hop, _)| hop).collect::<Vec<_>>(), [hop(Light, "fr-en")]);
        // One quality alone has nothing to spare, whichever is asked for.
        let alone = installed(&[(Light, "fr-en"), (Light, "en-fr"), (Light, "de-en")]);
        assert_eq!((alone.unused(Light).count(), alone.unused(Accurate).count()), (0, 0));
    }

    #[test]
    fn no_route_takes_an_unused_model_and_none_changes_without_them() {
        let all = [(Light, "fr-en"), (Light, "en-fr"), (Accurate, "fr-en"), (Accurate, "en-fr"), (Light, "de-en"), (Light, "en-de"), (Accurate, "de-en"), (Light, "es-en")];
        let models = installed(&all);
        for quality in QUALITIES {
            let unused: Vec<Hop> = models.unused(quality).map(|(hop, _)| hop).collect();
            let kept = Installed::of(models.models.iter().copied().filter(|hop| !unused.contains(hop)).collect());
            assert!(!unused.is_empty() && kept.unused(quality).count() == 0);
            let languages = models.languages();
            for direction in languages.iter().flat_map(|from| languages.iter().map(|to| Direction::new(*from, *to))) {
                let route = models.route(quality, direction);
                assert!(route.iter().flatten().all(|hop| !unused.contains(hop)), "{quality:?} {direction:?}");
                assert_eq!(kept.route(quality, direction), route, "{quality:?} {direction:?}");
            }
        }
    }

    #[test]
    fn the_unused_models_are_weighed_by_their_files() {
        let models = std::env::temp_dir().join(format!("coco-store-unused-{}", std::process::id()));
        for (model, weights) in [(hop(Light, "fr-en"), 300), (hop(Light, "en-fr"), 200), (hop(Accurate, "fr-en"), 1000), (hop(Accurate, "en-fr"), 900), (hop(Light, "de-en"), 50)] {
            fill(&folder(&models, model), &FILES);
            std::fs::write(folder(&models, model).join(FILES[0]), vec![0u8; weights]).expect("write the weights");
        }
        let found = Installed::scan(&models);
        // The weights and the four files of one byte, for each direction.
        assert_eq!(found.unused_size(Accurate), 300 + 200 + 8);
        assert_eq!(found.unused_size(Light), 1000 + 900 + 8);
        assert_eq!(Installed::default().unused_size(Light), 0);
        std::fs::remove_dir_all(models).ok();
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
