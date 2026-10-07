//! Downloads models the way the app does, without the app.
//!
//!     cargo run --release -p coco-engine --example install -- light|accurate|LANGUAGE [MODELS_DIR]
//!
//! `light` and `accurate` are the two sets of French and English. A
//! language is a code like `es`: its two small models, to English and from
//! it. Without a folder, the models go where the app reads them: see
//! `coco_engine::default_models_dir`.

use std::path::PathBuf;
use std::process::ExitCode;

use coco_engine::{InstallUpdate, Language, Quality, default_models_dir, download_size, install, install_language, language_download_size, languages};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let models = arguments.get(1).map_or_else(default_models_dir, PathBuf::from);
    let what = arguments.first().map_or("", String::as_str);
    let language = Language::from_code(what).filter(|language| languages().contains(language));
    let (name, bytes, updates) = match (what, language) {
        ("light", _) => ("Light set".to_string(), download_size(Quality::Light), install(models.clone(), Quality::Light)),
        ("accurate", _) => ("Accurate set".to_string(), download_size(Quality::Accurate), install(models.clone(), Quality::Accurate)),
        (_, Some(language)) => (language.name().to_string(), language_download_size(language), install_language(models.clone(), language)),
        _ => {
            let codes: Vec<String> = languages().iter().map(|language| language.code().to_string()).collect();
            eprintln!("usage: install light|accurate|LANGUAGE [MODELS_DIR]\nlanguages: {}", codes.join(" "));
            return ExitCode::from(2);
        }
    };
    println!("{name}, {} MB to download, into {}", bytes / 1_000_000, models.display());

    let mut last = None;
    for update in futures_executor::block_on_stream(updates) {
        match update {
            // One line per megabyte would be a long page: one per tenth.
            InstallUpdate::Downloading { done, total } => {
                let tenth = done * 10 / total.max(1);
                if last != Some(tenth) {
                    println!("{} of {} MB", done / 1_000_000, total / 1_000_000);
                    last = Some(tenth);
                }
            }
            InstallUpdate::Checking => println!("checking"),
            InstallUpdate::Done => {
                println!("installed");
                return ExitCode::SUCCESS;
            }
            InstallUpdate::Failed(why) => {
                eprintln!("install: {why}");
                return ExitCode::FAILURE;
            }
        }
    }
    eprintln!("install: the download stopped without a word");
    ExitCode::FAILURE
}
