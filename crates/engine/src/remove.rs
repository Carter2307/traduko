//! Takes off the disk the models that are there for nothing.
//!
//! French and English have a model in each quality once both sets are
//! installed, and a translation only takes the one of the chosen quality:
//! see `store`. The other one is a few hundred megabytes that nothing
//! reads. `install` brings it back when it is wanted again.

use std::fs;
use std::path::Path;

use crate::store::{self, Installed};
use crate::{Quality, install};

/// Removes the models that no translation uses while `quality` is the
/// chosen one, and gives the bytes that it freed: what
/// [`Installed::unused_size`] said. Blocks for the time it takes, and for
/// as long as an installation runs.
///
/// A model that is in memory goes on translating: its weights were read
/// whole. On a failure the text says why, for the user; what was removed
/// before it stays removed.
pub fn remove_unused(models_dir: &Path, quality: Quality) -> Result<u64, String> {
    let _turns = install::turns();
    let mut freed = 0;
    // Looked at now: what was on disk a moment ago may have changed.
    for (model, bytes) in Installed::scan(models_dir).unused(quality) {
        let folder = store::folder(models_dir, model);
        fs::remove_dir_all(&folder).map_err(|error| format!("Cannot remove {}: {error}.", folder.display()))?;
        freed += bytes;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Direction;
    use crate::store::Hop;
    use Quality::{Accurate, Light};

    /// A models folder with these models, each of five files of ten bytes.
    fn models(name: &str, on_disk: &[(Quality, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("coco-remove-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (quality, code) in on_disk {
            let folder = folder(&dir, *quality, code);
            fs::create_dir_all(&folder).expect("create the model folder");
            for file in store::FILES {
                fs::write(folder.join(file), [0u8; 10]).expect("write a model file");
            }
        }
        dir
    }

    fn folder(models: &Path, quality: Quality, code: &str) -> PathBuf {
        store::folder(models, Hop { quality, direction: Direction::from_code(code).expect("a direction") })
    }

    const BOTH_SETS: [(Quality, &str); 6] =
        [(Light, "fr-en"), (Light, "en-fr"), (Accurate, "fr-en"), (Accurate, "en-fr"), (Light, "de-en"), (Light, "en-de")];

    #[test]
    fn the_set_that_is_not_chosen_leaves_the_disk_and_the_rest_stays() {
        let models = models("light", &BOTH_SETS);
        assert_eq!(Installed::scan(&models).unused_size(Accurate), 100);

        assert_eq!(remove_unused(&models, Accurate), Ok(100));
        assert!(!folder(&models, Light, "fr-en").exists() && !folder(&models, Light, "en-fr").exists());
        let left = Installed::scan(&models);
        assert_eq!(left.sets(), [Accurate]);
        // German only exists in the small models: they are in use.
        assert!(left.translates(Direction::from_code("de-fr").unwrap()));
        assert_eq!(left.qualities(), [Light, Accurate]);

        // Nothing is left to remove, and asking again is not an error.
        assert_eq!((left.unused_size(Accurate), left.unused_size(Light)), (0, 0));
        assert_eq!(remove_unused(&models, Accurate), Ok(0));
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn the_large_models_are_the_ones_to_go_when_the_small_ones_are_chosen() {
        let models = models("accurate", &BOTH_SETS);
        assert_eq!(remove_unused(&models, Light), Ok(100));
        let left = Installed::scan(&models);
        assert_eq!((left.sets(), left.qualities()), (vec![Light], vec![Light]));
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn what_sits_beside_the_files_of_a_model_goes_with_it() {
        let models = models("beside", &[(Light, "es-en"), (Accurate, "es-en")]);
        let spanish = folder(&models, Light, "es-en");
        fs::write(spanish.join("model.safetensors.sha256"), b"0000\n").unwrap();
        // The five files are what is counted; the folder goes whole.
        assert_eq!(remove_unused(&models, Accurate), Ok(50));
        assert!(!spanish.exists());
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn one_quality_alone_or_no_model_at_all_has_nothing_to_remove() {
        let models = models("alone", &[(Light, "fr-en"), (Light, "en-fr"), (Accurate, "de-en")]);
        assert_eq!(remove_unused(&models, Accurate), Ok(0));
        assert_eq!(remove_unused(&models, Light), Ok(0));
        assert_eq!(Installed::scan(&models).languages().len(), 3);
        fs::remove_dir_all(&models).ok();
        // No folder of models either.
        assert_eq!(remove_unused(&models, Light), Ok(0));
    }
}
