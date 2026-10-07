//! What Coco remembers between two runs, as one small JSON file in
//! `~/Library/Application Support/Coco`.

use std::path::{Path, PathBuf};

use coco_engine::{Direction, EnglishVariant, Quality};
use serde::{Deserialize, Serialize};

pub const APP_DIR: &str = "Coco";
pub const BUNDLE_ID: &str = "com.github.carter2307.coco";
const FILE: &str = "settings.json";

/// How large Coco is on the desktop.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MascotSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl MascotSize {
    /// Side of the mascot's window, in points. The body is about 70 % of it:
    /// the rest is room for the hop, the stretch and the wobble.
    pub fn side(self) -> f32 {
        match self {
            MascotSize::Small => 84.0,
            MascotSize::Medium => 112.0,
            MascotSize::Large => 148.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Bottom-left corner of the mascot window, in screen points.
    pub mascot: Option<(f64, f64)>,
    pub mascot_size: MascotSize,
    pub french_to_english: bool,
    pub british: bool,
    pub accurate: bool,
    /// What the user wants. macOS holds the real state of the login item.
    pub open_at_login: bool,
    /// The installed copy that was last registered at login, to tell "the
    /// user removed the login item" from "a new build replaced the app".
    pub registered_install: Option<String>,
    /// The first-run screens were gone through to the end.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mascot: None,
            mascot_size: MascotSize::default(),
            french_to_english: true,
            british: false,
            accurate: true,
            // The mascot is meant to be there after a restart.
            open_at_login: true,
            registered_install: None,
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn direction(&self) -> Direction {
        if self.french_to_english { Direction::FrToEn } else { Direction::EnToFr }
    }

    pub fn english(&self) -> EnglishVariant {
        if self.british { EnglishVariant::British } else { EnglishVariant::American }
    }

    /// The preferred model if it is on disk, else the other one.
    pub fn quality(&self, installed: &[Quality]) -> Quality {
        let wanted = if self.accurate { Quality::Accurate } else { Quality::Light };
        if installed.contains(&wanted) || installed.is_empty() { wanted } else { installed[0] }
    }

    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    /// Writes to a temporary file first, so a crash never leaves half a file.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let temporary = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, dir.join(FILE))
    }
}

/// Where the settings and the instance lock live. `COCO_SUPPORT_DIR` moves
/// them, so that a test run leaves the real preferences alone.
pub fn support_dir() -> PathBuf {
    std::env::var_os("COCO_SUPPORT_DIR")
        .map(PathBuf::from)
        .or_else(|| coco_login::bundle::app_support_dir(APP_DIR))
        .unwrap_or_else(|| std::env::temp_dir().join(APP_DIR))
}

/// Where the models are: `COCO_MODELS_DIR`, else the usual folder whatever
/// `COCO_SUPPORT_DIR` says, since the models are large and shared.
pub fn models_dir() -> PathBuf {
    std::env::var_os("COCO_MODELS_DIR")
        .map(PathBuf::from)
        .or_else(|| coco_login::bundle::app_support_dir(APP_DIR).map(|dir| dir.join("models")))
        .unwrap_or_else(|| support_dir().join("models"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let dir = std::env::temp_dir().join(format!("coco-settings-{}", std::process::id()));
        let settings = Settings {
            mascot: Some((120.0, 48.5)),
            mascot_size: MascotSize::Large,
            british: true,
            accurate: false,
            ..Settings::default()
        };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), settings);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        let dir = std::env::temp_dir().join(format!("coco-settings-broken-{}", std::process::id()));
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), b"{ not json").unwrap();
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_written_before_the_size_setting_gives_the_medium_mascot() {
        let dir = std::env::temp_dir().join(format!("coco-settings-older-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), br#"{ "british": true, "accurate": false }"#).unwrap();
        let settings = Settings::load(&dir);
        assert_eq!(settings.mascot_size, MascotSize::Medium);
        assert!(settings.british);
        // Nor did it see the first screens: they are shown once.
        assert!(!settings.onboarded);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_model_falls_back_to_the_one_on_disk() {
        let settings = Settings::default();
        assert_eq!(settings.quality(&[Quality::Light]), Quality::Light);
        assert_eq!(settings.quality(&[Quality::Light, Quality::Accurate]), Quality::Accurate);
    }
}
