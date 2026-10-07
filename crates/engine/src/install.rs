//! Downloads models and puts them where `store` finds them: the French and
//! English set of a quality, or another language to and from English.
//!
//! This is `scripts/fetch-models.sh` for an app that was not installed by
//! the script: the same files, from the same commits, checked the same way.
//! A test keeps the two tables in step.
//!
//! The engine has no network code of its own. Each file comes through the
//! `curl` that macOS ships, and the weights are checked against a known
//! SHA-256 twice: as downloaded, and as installed. Only safetensors weights
//! are fetched: never a pickle (.bin), which can run code when it is loaded.
//!
//! A download that stops (no network, the app quits, nobody reads the
//! updates any more) leaves what it has in `model.safetensors.download`,
//! and the next one goes on from there.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use sha2::{Digest, Sha256};

use crate::store::{self, Hop};
use crate::weights::FILE as WEIGHTS;
use crate::{Direction, Language, Quality, narrow};

#[derive(Clone, Debug, PartialEq)]
pub enum InstallUpdate {
    /// Bytes of weights on disk so far and in all, both directions together.
    Downloading { done: u64, total: u64 },
    /// A file is whole: it is being checked against its SHA-256 and, for
    /// the small models, rewritten in 16 bits.
    Checking,
    Done,
    /// The models are not installed; the text says why, for the user. What
    /// was downloaded is kept unless it was wrong.
    Failed(String),
}

/// One model of the Hub, at a pinned commit.
struct Source {
    quality: Quality,
    direction: Direction,
    repository: &'static str,
    commit: &'static str,
    /// Size of `model.safetensors` on the Hub, in bytes.
    bytes: u64,
    /// Its SHA-256 as downloaded, and as installed. The two differ for the
    /// small models, which the Hub has in 32-bit floats: see `narrow`.
    ///
    /// `None` is a model that had not been installed anywhere when its line
    /// was written, so what it becomes in 16 bits was not known. The
    /// download is checked all the same, and the rewriting gives the same
    /// bytes on every run: the SHA-256 it comes out with is kept in
    /// [`RECORD`], next to the weights, to check them the next time.
    downloaded: &'static str,
    installed: Option<&'static str>,
}

/// Where the SHA-256 of installed weights is kept when the table has none.
const RECORD: &str = "model.safetensors.sha256";

impl Source {
    fn url(&self, file: &str) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{file}", self.repository, self.commit)
    }

    fn model(&self) -> Hop {
        Hop { quality: self.quality, direction: self.direction }
    }

    /// True for the models of French and English: the sets.
    fn is_of_a_set(&self) -> bool {
        self.direction == FRENCH || self.direction == FRENCH.swapped()
    }

    /// The language it brings, English being the other one.
    fn language(&self) -> Language {
        if self.direction.from == Language::ENGLISH { self.direction.to } else { self.direction.from }
    }

    /// The SHA-256 that installed weights must have, when it is known.
    fn expected(&self, folder: &Path) -> Option<String> {
        let recorded = || fs::read_to_string(folder.join(RECORD)).ok().map(|record| record.trim().to_string());
        self.installed.map(str::to_string).or_else(recorded)
    }
}

const FRENCH: Direction = Direction::new(Language::FRENCH, Language::ENGLISH);

/// "es", "en" as a direction, for the table.
const fn way(from: &[u8; 2], to: &[u8; 2]) -> Direction {
    Direction::new(Language::of(from), Language::of(to))
}

/// The table of `scripts/fetch-models.sh`, with the sizes: the two sets of
/// French and English first, then the other languages, each to English and
/// from it.
///
/// The Hub has most of the small models as safetensors only in a pull
/// request of its conversion bot (SFconvertbot): the commit is then the
/// head of the latest one.
const SOURCES: [Source; 30] = [
    Source {
        quality: Quality::Light,
        direction: way(b"fr", b"en"),
        repository: "Helsinki-NLP/opus-mt-fr-en",
        commit: "c4aed37b318c763fd177aa449b44e3b783cc6c02",
        bytes: 300_803_608,
        downloaded: "6e3837f34b903802c3d0d670362b997cee6e87584a1108eb3fa89e4625e4424a",
        installed: Some("44622407d10fd34e5c98f532b70bb635dceb6f92af1d6600419cad9986753844"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"fr"),
        repository: "Helsinki-NLP/opus-mt-en-fr",
        commit: "c96fcd7f38a4c0d1ac9b61e255b47f3f7ff55e5e",
        bytes: 300_803_256,
        downloaded: "c32d3003ced798c78b9ca90fb69c8e23ad5b4c64c500628bdf3303bbc9272816",
        installed: Some("1b66c1c9c0baa7cca82e52a3711662da0d5f2e103bd60c810f4198dc94d44158"),
    },
    Source {
        quality: Quality::Accurate,
        direction: way(b"fr", b"en"),
        repository: "Helsinki-NLP/opus-mt-tc-big-fr-en",
        commit: "5fa3b3c9fa3bcd65fbf31206c3ec7419c9d9cd7d",
        bytes: 461_429_130,
        downloaded: "1e9d73c1af19660aad2e0733e12f8d06745eb54c77f48bae4831e377ccaf6d6e",
        installed: Some("1e9d73c1af19660aad2e0733e12f8d06745eb54c77f48bae4831e377ccaf6d6e"),
    },
    Source {
        quality: Quality::Accurate,
        direction: way(b"en", b"fr"),
        repository: "Helsinki-NLP/opus-mt-tc-big-en-fr",
        commit: "6e062862ced6f5622a589ab2aadc2a1d4978db78",
        bytes: 461_429_130,
        downloaded: "5c88b4f7a63934b8be72372b86661e616227c20be4b75612eeee7dca96494217",
        installed: Some("5c88b4f7a63934b8be72372b86661e616227c20be4b75612eeee7dca96494217"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"es", b"en"),
        repository: "Helsinki-NLP/opus-mt-es-en",
        commit: "725b7965a8cac11ebe80ea671e72e0b7e8b28a9f",
        bytes: 312_062_580,
        downloaded: "07d9fc8881ac9bc8f06fbe3576ca16045c684c7d529e9733cbeeaaf2c78f9539",
        installed: Some("a1089b29a562c210e4c0fb16283ceca39e810f5ecd75013f5138efc7d50a1852"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"es"),
        repository: "Helsinki-NLP/opus-mt-en-es",
        commit: "fdaddf76f50fcc1583ba42f95965862a7ab30f97",
        bytes: 312_062_468,
        downloaded: "b3ecbf954573c2fd95d05d3ad4618baf961793db0da07c2add2e8a3a6cd78d0b",
        installed: Some("2ecae2e77bbe980b5c69277ff4c7a7474e63c392ea89a0bc01b907033843d6d0"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"de", b"en"),
        repository: "Helsinki-NLP/opus-mt-de-en",
        commit: "6f6b23ef5ef8a586414ad9d2b7dda64ca5352935",
        bytes: 297_903_668,
        downloaded: "5b1a66c79f6e871eb01b8818819e29bbdb6a25139af210b97b6efbf862493b4d",
        installed: Some("7c7e35679866fffc0ed799fb6c559956564e331053f747e23fed53418526532b"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"de"),
        repository: "Helsinki-NLP/opus-mt-en-de",
        commit: "0012d6d6ff4b4dd06bee042d3294295f0e587abd",
        bytes: 297_903_668,
        downloaded: "3fb78700bce624990eff6468d0e14fbde18a6aedac35821fa8c0c4ead5fae014",
        installed: Some("4d47cc7827f85586b4d04f7629069ad61b49d0efb1d97fe2e42f48d684c3aca4"),
    },
    Source {
        quality: Quality::Light,
        direction: way(b"it", b"en"),
        repository: "Helsinki-NLP/opus-mt-it-en",
        commit: "d43c8316fd1645309b94b88ec52b13e505cc91d9",
        bytes: 343_618_124,
        downloaded: "a6e1dd4180f8aeec864ea5d79678025fa013e93df935d46159ebd6eb164782c1",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"it"),
        repository: "Helsinki-NLP/opus-mt-en-it",
        commit: "5483bd70fe6e934c56875647d5dbad58f09bbd16",
        bytes: 342_912_236,
        downloaded: "7012b826195f82a68efc878b39e763d2fbf8ebb485811375e9d9632899eba7d0",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"nl", b"en"),
        repository: "Helsinki-NLP/opus-mt-nl-en",
        commit: "385c15b2b304774437eac7c2b584e4868ca1c6f0",
        bytes: 316_222_336,
        downloaded: "048b19007be3c1f438566de765b8803035415d295706b9bfd9df7e083f6154db",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"nl"),
        repository: "Helsinki-NLP/opus-mt-en-nl",
        commit: "cb106147f56946f00045d2f9768539949e660489",
        bytes: 316_221_872,
        downloaded: "1e227db75d9134498000e1e91c9c8f6f6347d586d5fa2b7b08ea2c4e630c24db",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"ru", b"en"),
        repository: "Helsinki-NLP/opus-mt-ru-en",
        commit: "7c84e70e05294db4fead6135d04585020413f78b",
        bytes: 306_967_352,
        downloaded: "f73dc54675dc0da9ca6704098cadae3562f149b1f93c585f9d21c8502a760421",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"ru"),
        repository: "Helsinki-NLP/opus-mt-en-ru",
        commit: "3ff4dbd98515ba253f08f183e0de0b7ccf189ee3",
        bytes: 306_967_464,
        downloaded: "07f5d90a45955a73412a118f0635315a28ea5a5d82522f3be008e71d452526c0",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"sv", b"en"),
        repository: "Helsinki-NLP/opus-mt-sv-en",
        commit: "a66266b25c226b4666d58f8b8c1ee62a74386cfc",
        bytes: 294_483_096,
        downloaded: "482ca42946152a8c0af2f5ae7093823e5682e50af017ed7ab9d67f53a1ce6903",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"sv"),
        repository: "Helsinki-NLP/opus-mt-en-sv",
        commit: "4e83c9f90366cb438892040c46c66aa3b8406981",
        bytes: 294_482_984,
        downloaded: "1f10f4b66c86e4d98cdfa01e842ed2735312a6a99da08978b64e579e875fb0cb",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"uk", b"en"),
        repository: "Helsinki-NLP/opus-mt-uk-en",
        commit: "5c5e4e6e58d47ea2d90b657a7b0d270d06a89be2",
        bytes: 305_056_940,
        downloaded: "a52221e3b5ad186534f7c5a1af9b89728db9ce437a08cb27c8c5d08cc13014fc",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"uk"),
        repository: "Helsinki-NLP/opus-mt-en-uk",
        commit: "55104bebd23cd2d87761e650aa6ac7369b379d1c",
        bytes: 305_056_940,
        downloaded: "a0f3dce3522dc5c991df5b5b669c38622234f56eeb048f9ad95e4459e0ee1088",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"hi", b"en"),
        repository: "Helsinki-NLP/opus-mt-hi-en",
        commit: "338f1e1226738d9095c1e8b6f8931c46c0de85a4",
        bytes: 304_113_020,
        downloaded: "f09b34ffd2a8d8046b1733f0796b390c2be907effe7c6a70ed162fae9aae68c1",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"hi"),
        repository: "Helsinki-NLP/opus-mt-en-hi",
        commit: "43211ff5af2aad1a5da49a7017bf790346dc0eef",
        bytes: 305_801_816,
        downloaded: "46ae1116913bce01c9d848a78f62da2bd986d728bced4dc1acd5fedf4338ac5e",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"da", b"en"),
        repository: "Helsinki-NLP/opus-mt-da-en",
        commit: "0dd912927fa4b60317a44ed04c9ffd4c56d4b4ee",
        bytes: 299_604_776,
        downloaded: "a8effdcf176755cc436f107719db0b082d9156c9ae54e61dd426f2f3b4f36243",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"da"),
        repository: "Helsinki-NLP/opus-mt-en-da",
        commit: "bee92961eb97c1a70d247637ab527439ed915b14",
        bytes: 299_604_776,
        downloaded: "af93346239b6935d940ea0d00aff951915d7732027644f1cf9cb5f17e1adf5a4",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"fi", b"en"),
        repository: "Helsinki-NLP/opus-mt-fi-en",
        commit: "22095576b4b6a72271c6ad7bcb32b91066e7ca94",
        bytes: 301_100_684,
        downloaded: "2f8b591f7c7543ecf557653e8b90900478fc134adc88df62404b30cf95958c28",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"fi"),
        repository: "Helsinki-NLP/opus-mt-en-fi",
        commit: "f294bfca8f7be502fde2362e4d0ac09fb232415b",
        bytes: 312_062_468,
        downloaded: "2a43d7e0416faf10acb80a106d470f02791f0d6be7a36bcc011cd67943fd07d2",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"cs", b"en"),
        repository: "Helsinki-NLP/opus-mt-cs-en",
        commit: "0d5030270f2d0bf4456b821b77dce67f9367e9fd",
        bytes: 306_948_884,
        downloaded: "28fcc2e7d7cfcea474fe35fe9ac0471dcfa165d5716db5189a19b8035c6e48aa",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"cs"),
        repository: "Helsinki-NLP/opus-mt-en-cs",
        commit: "cfeb086a3a864c78e3e32da25bcad89bcb4a0359",
        bytes: 306_948_884,
        downloaded: "79f82c7a1228c0e56cd014af4f8712405908df1bd9099e0d873f0bf3b6374204",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"hu", b"en"),
        repository: "Helsinki-NLP/opus-mt-hu-en",
        commit: "8e850310a8d25c50c1be55f42b707a770ade49a6",
        bytes: 306_975_560,
        downloaded: "65104eb75a197ffe96d802fc8203f09eff681040e69713df75b7532d32384552",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"hu"),
        repository: "Helsinki-NLP/opus-mt-en-hu",
        commit: "dfa8f0482de57e05614fac8f93d83665cfa846a7",
        bytes: 306_975_560,
        downloaded: "1bc13ce06cb34e227cecbc241ab3eac2204cb8a7faffae379ceb1e6dff888fb9",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"id", b"en"),
        repository: "Helsinki-NLP/opus-mt-id-en",
        commit: "b7a06a6a2bb269a4de8cdad71940035eae97cea7",
        bytes: 291_121_808,
        downloaded: "e71532a9cfa6392e7ac5f725d3c9dc82ff6c5a9701b1a407db6dcc25bf4440ce",
        installed: None,
    },
    Source {
        quality: Quality::Light,
        direction: way(b"en", b"id"),
        repository: "Helsinki-NLP/opus-mt-en-id",
        commit: "916d091c1efaba75c4ba7cd2f544ef2fbcc1a194",
        bytes: 291_121_808,
        downloaded: "e7169d62592b1a9fefe33b7ab410c5d1d619e39b1df3d07175bd57d8331ffcd1",
        installed: None,
    },
];

/// The models of a set: French and English, both ways, in one quality.
fn set(quality: Quality) -> Vec<&'static Source> {
    SOURCES.iter().filter(|source| source.quality == quality && source.is_of_a_set()).collect()
}

/// The models of a language: to English and from it. They are small ones,
/// the only kind that every language has.
fn models_of(language: Language) -> Vec<&'static Source> {
    SOURCES.iter().filter(|source| source.quality == Quality::Light && source.language() == language).collect()
}

/// How much [`install`] downloads for a set, in bytes. The light set takes
/// half of that on disk once installed.
pub fn download_size(quality: Quality) -> u64 {
    set(quality).iter().map(|source| source.bytes).sum()
}

/// The languages that [`install_language`] can add to French and English,
/// in the order of the table.
pub fn languages() -> Vec<Language> {
    let mut languages: Vec<Language> = SOURCES.iter().filter(|source| !source.is_of_a_set()).map(Source::language).collect();
    languages.dedup();
    languages
}

/// How much [`install_language`] downloads for a language, in bytes: half
/// of that stays on disk. Nothing for a language that is not in the table.
pub fn language_download_size(language: Language) -> u64 {
    models_of(language).iter().map(|source| source.bytes).sum()
}

/// One installation of a set at a time: two would write the same files. A
/// second one waits for the first, which is short when the first was just
/// stopped and is on its way out.
static TURNS: [Mutex<()>; 2] = [Mutex::new(()), Mutex::new(())];

/// The same for the languages, which take turns among themselves. French
/// is also the light set: it waits for that one too.
static LANGUAGE_TURN: Mutex<()> = Mutex::new(());

/// Waits for the installations that are running, and holds the next ones
/// back for as long as what it gives is kept: no folder is written while
/// models are taken off the disk.
pub(crate) fn turns() -> Vec<MutexGuard<'static, ()>> {
    TURNS.iter().chain([&LANGUAGE_TURN]).map(|turn| turn.lock().unwrap_or_else(PoisonError::into_inner)).collect()
}

/// Starts downloading a set into `models_dir`. Never blocks. The stream ends
/// after `Done` or `Failed`; dropping it stops the download.
pub fn install(models_dir: PathBuf, quality: Quality) -> UnboundedReceiver<InstallUpdate> {
    start(models_dir, set(quality), &TURNS[quality as usize])
}

/// Starts downloading the models of a language, to English and from it,
/// into `models_dir`. The stream is the one of [`install`]. A language that
/// is not in the table fails at once: see [`languages`].
pub fn install_language(models_dir: PathBuf, language: Language) -> UnboundedReceiver<InstallUpdate> {
    start(models_dir, models_of(language), &LANGUAGE_TURN)
}

fn start(models_dir: PathBuf, sources: Vec<&'static Source>, turn: &'static Mutex<()>) -> UnboundedReceiver<InstallUpdate> {
    let (updates, stream) = unbounded();
    let refused = updates.clone();
    let work = move || {
        let _turn = turn.lock().unwrap_or_else(PoisonError::into_inner);
        if sources.is_empty() {
            let _ = updates.unbounded_send(InstallUpdate::Failed("There is no model to download for this language.".into()));
            return;
        }
        let last = match run(&Curl, &sources, &models_dir, &updates) {
            Ok(()) => InstallUpdate::Done,
            Err(Stop::Failed(why)) => InstallUpdate::Failed(why),
            Err(Stop::Unwanted) => return,
        };
        let _ = updates.unbounded_send(last);
    };
    if std::thread::Builder::new().name("coco-install".into()).spawn(work).is_err() {
        let _ = refused.unbounded_send(InstallUpdate::Failed("The download could not start.".into()));
    }
    stream
}

/// Why an installation ended early.
#[derive(Debug, PartialEq)]
enum Stop {
    /// Nobody reads the updates any more.
    Unwanted,
    Failed(String),
}

type Step<T> = Result<T, Stop>;

/// Brings one file of the Hub to disk. The real one is [`Curl`]; the tests
/// have a shelf of their own.
trait Fetch {
    /// Appends to `to` what it does not hold yet of `url`. `wanted` gets the
    /// size of the file so far, a few times per second, and says whether to
    /// go on.
    fn fetch(&self, url: &str, to: &Path, wanted: &mut dyn FnMut(u64) -> bool) -> Step<()>;
}

fn run(fetch: &impl Fetch, sources: &[&Source], models_dir: &Path, updates: &UnboundedSender<InstallUpdate>) -> Step<()> {
    let say = |update| updates.unbounded_send(update).map_err(|_| Stop::Unwanted);
    let total: u64 = sources.iter().map(|source| source.bytes).sum();
    let mut before = 0;
    for source in sources {
        let folder = store::folder(models_dir, source.model());
        fs::create_dir_all(&folder).map_err(|error| unwritable(&folder, &error))?;

        install_weights(fetch, source, &folder, updates, before, total)?;
        before += source.bytes;
        say(InstallUpdate::Downloading { done: before, total })?;

        // A file gets its final name only when it is whole.
        for file in store::FILES.into_iter().filter(|file| *file != WEIGHTS) {
            let place = folder.join(file);
            if place.is_file() {
                continue;
            }
            let part = folder.join(format!("{file}.part"));
            let _ = fs::remove_file(&part);
            fetch.fetch(&source.url(file), &part, &mut |_| !updates.is_closed())?;
            fs::rename(&part, &place).map_err(|error| unwritable(&folder, &error))?;
        }
    }
    Ok(())
}

fn install_weights(
    fetch: &impl Fetch,
    source: &Source,
    folder: &Path,
    updates: &UnboundedSender<InstallUpdate>,
    before: u64,
    total: u64,
) -> Step<()> {
    let say = |update| updates.unbounded_send(update).map_err(|_| Stop::Unwanted);
    let weights = folder.join(WEIGHTS);
    if weights.is_file()
        && let Some(expected) = source.expected(folder)
    {
        say(InstallUpdate::Checking)?;
        if sha256(&weights, updates)? == expected {
            return Ok(());
        }
    }

    let download = folder.join(format!("{WEIGHTS}.download"));
    let size = |path: &Path| fs::metadata(path).map_or(0, |file| file.len());
    // More than the whole file: whatever this is, it is not a part of it.
    if size(&download) > source.bytes {
        let _ = fs::remove_file(&download);
    }
    if size(&download) != source.bytes {
        fetch.fetch(&source.url(WEIGHTS), &download, &mut |so_far| {
            say(InstallUpdate::Downloading { done: before + so_far.min(source.bytes), total }).is_ok()
        })?;
    }

    say(InstallUpdate::Checking)?;
    if sha256(&download, updates)? != source.downloaded {
        let _ = fs::remove_file(&download);
        return Err(Stop::Failed("The download is not the model that was expected, so it was removed. Try again.".into()));
    }
    if source.installed == Some(source.downloaded) {
        return fs::rename(&download, &weights).map_err(|error| unwritable(folder, &error));
    }

    let part = folder.join(format!("{WEIGHTS}.part"));
    narrow::to_f16(&download, &part).map_err(Stop::Failed)?;
    let narrowed = sha256(&part, updates)?;
    match source.installed {
        Some(expected) if narrowed != expected => {
            let _ = fs::remove_file(&part);
            return Err(Stop::Failed("The model did not come out of its rewriting in 16 bits as expected. Try again.".into()));
        }
        Some(_) => {}
        None => fs::write(folder.join(RECORD), format!("{narrowed}\n")).map_err(|error| unwritable(folder, &error))?,
    }
    fs::rename(&part, &weights).map_err(|error| unwritable(folder, &error))?;
    let _ = fs::remove_file(&download);
    Ok(())
}

fn unwritable(folder: &Path, error: &std::io::Error) -> Stop {
    Stop::Failed(format!("Cannot write in {}: {error}.", folder.display()))
}

/// The SHA-256 of a file, in hexadecimal. Half a gigabyte takes a moment,
/// so this gives up as soon as nobody waits for the answer.
fn sha256(path: &Path, updates: &UnboundedSender<InstallUpdate>) -> Step<String> {
    let unreadable = |error: std::io::Error| Stop::Failed(format!("Cannot read {}: {error}.", path.display()));
    let mut file = File::open(path).map_err(unreadable)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(unreadable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if updates.is_closed() {
            return Err(Stop::Unwanted);
        }
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The `curl` of macOS, one process per file.
///
/// curl writes to a pipe and this process writes the file. If the app dies,
/// the pipe closes and curl stops with it: a download never goes on behind
/// the user's back.
struct Curl;

const CURL: &str = "/usr/bin/curl";
/// How often a running download is looked at.
const LOOK: Duration = Duration::from_millis(100);
/// curl's exit codes for a server that will not give the rest of a file:
/// no byte ranges, or the range starts after the end.
const CANNOT_RESUME: [i32; 2] = [33, 36];

impl Fetch for Curl {
    fn fetch(&self, url: &str, to: &Path, wanted: &mut dyn FnMut(u64) -> bool) -> Step<()> {
        let unwritable = |error: std::io::Error| Stop::Failed(format!("Cannot write {}: {error}.", to.display()));
        let have = fs::metadata(to).map_or(0, |file| file.len());
        let file = OpenOptions::new().create(true).append(true).open(to).map_err(unwritable)?;

        let mut curl = Command::new(CURL)
            .args(["--fail", "--location", "--silent", "--show-error"])
            .args(["--proto", "=https", "--proto-redir", "=https"])
            .args(["--connect-timeout", "20"])
            // Give up on a connection that carried nothing for a minute.
            .args(["--speed-limit", "1", "--speed-time", "60"])
            .args(["--continue-at", &have.to_string()])
            .args(["--output", "-", url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| Stop::Failed(format!("Cannot run {CURL}: {error}.")))?;
        let (Some(pipe), Some(mut complaints)) = (curl.stdout.take(), curl.stderr.take()) else {
            let _ = curl.kill();
            return Err(Stop::Failed(format!("Cannot read from {CURL}.")));
        };

        // The copy has its own thread, so that this one can stop curl at
        // once even when the network has gone quiet.
        let written = Arc::new(AtomicU64::new(have));
        let copy = {
            let written = written.clone();
            std::thread::spawn(move || copy(pipe, file, &written))
        };
        let status = loop {
            match curl.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if wanted(written.load(Ordering::Relaxed)) => std::thread::sleep(LOOK),
                stopped => {
                    let _ = curl.kill();
                    let _ = curl.wait();
                    let _ = copy.join();
                    return Err(match stopped {
                        Err(error) => Stop::Failed(format!("Lost track of {CURL}: {error}.")),
                        _ => Stop::Unwanted,
                    });
                }
            }
        };
        // curl is gone: the copy ends with what was left in the pipe.
        let copied = copy.join().unwrap_or_else(|_| Err(std::io::Error::other("the copy stopped")));
        let mut said = String::new();
        let _ = complaints.read_to_string(&mut said);

        if let Err(error) = copied {
            return Err(unwritable(error));
        }
        if status.success() {
            wanted(written.load(Ordering::Relaxed));
            return Ok(());
        }
        // The part on disk cannot be continued: start again without it.
        let refused_range = status.code().is_some_and(|code| CANNOT_RESUME.contains(&code)) || said.contains(" 416");
        if have > 0 && refused_range {
            fs::remove_file(to).map_err(unwritable)?;
            return self.fetch(url, to, wanted);
        }
        Err(Stop::Failed(reason(status.code(), &said)))
    }
}

/// Moves what curl sends to the file, counting. Stops at the first write
/// that fails, which closes the pipe and so stops curl.
fn copy(mut pipe: impl Read, mut file: File, written: &AtomicU64) -> std::io::Result<()> {
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = pipe.read(&mut buffer)?;
        if read == 0 {
            return file.flush();
        }
        file.write_all(&buffer[..read])?;
        written.fetch_add(read as u64, Ordering::Relaxed);
    }
}

/// What to tell the user when curl gave up. `said` is its own line, such as
/// "curl: (6) Could not resolve host: huggingface.co".
fn reason(code: Option<i32>, said: &str) -> String {
    match code {
        Some(6 | 7) => "Cannot reach huggingface.co. Check the connection, then try again.".into(),
        Some(18 | 28 | 52 | 55 | 56) => "The connection dropped. Try again: the download goes on from where it stopped.".into(),
        Some(23) => "The download could not be written to the disk. Check that there is room, then try again.".into(),
        _ => {
            let line = said.lines().last().unwrap_or("").trim();
            let line = line.split_once(") ").map_or(line, |(_, text)| text).trim_end_matches('.');
            if line.is_empty() { "The download stopped. Try again.".into() } else { format!("The download stopped: {line}. Try again.") }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::store::Installed;

    const ENGLISH: Direction = FRENCH.swapped();

    /// Files that can be fetched without a network, and what was asked.
    #[derive(Default)]
    struct Shelf {
        files: HashMap<String, Vec<u8>>,
        asked: Mutex<Vec<String>>,
        /// Says no to `wanted` after this many bytes, like a closed window.
        stop_after: Option<usize>,
    }

    impl Fetch for Shelf {
        fn fetch(&self, url: &str, to: &Path, wanted: &mut dyn FnMut(u64) -> bool) -> Step<()> {
            self.asked.lock().unwrap().push(url.to_string());
            let bytes = self.files.get(url).ok_or_else(|| Stop::Failed(format!("{url} is not on the shelf")))?;
            let have = fs::metadata(to).map_or(0, |file| file.len()) as usize;
            let upto = self.stop_after.map_or(bytes.len(), |stop| stop.min(bytes.len()));
            let mut file = OpenOptions::new().create(true).append(true).open(to).unwrap();
            file.write_all(&bytes[have.min(upto)..upto]).unwrap();
            if wanted(upto as u64) && upto == bytes.len() { Ok(()) } else { Err(Stop::Unwanted) }
        }
    }

    fn hex(bytes: &[u8]) -> &'static str {
        let text: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        text.leak()
    }

    /// A source whose weights are `weights`, installed as they are, and a
    /// shelf that holds its five files.
    fn source(direction: Direction, weights: &[u8], shelf: &mut Shelf) -> Source {
        let source = Source {
            quality: Quality::Accurate,
            direction,
            repository: format!("tests/{}", direction.code()).leak(),
            commit: "0000",
            bytes: weights.len() as u64,
            downloaded: hex(weights),
            installed: Some(hex(weights)),
        };
        for file in store::FILES {
            shelf.files.insert(source.url(file), if file == WEIGHTS { weights.to_vec() } else { file.as_bytes().to_vec() });
        }
        source
    }

    fn models(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("coco-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// Runs an installation to its end and gives what it said.
    fn installation(shelf: &Shelf, sources: &[&Source], models: &Path) -> (Step<()>, Vec<InstallUpdate>) {
        let (updates, stream) = unbounded();
        let outcome = run(shelf, sources, models, &updates);
        drop(updates);
        (outcome, futures_executor::block_on_stream(stream).collect())
    }

    #[test]
    fn a_set_is_downloaded_checked_and_installed() {
        let models = models("whole");
        let mut shelf = Shelf::default();
        let (one, two) = (source(FRENCH, b"french to english", &mut shelf), source(ENGLISH, b"english to french!", &mut shelf));
        assert!(Installed::scan(&models).is_empty());

        let (outcome, said) = installation(&shelf, &[&one, &two], &models);
        assert_eq!(outcome, Ok(()));
        assert_eq!(Installed::scan(&models).sets(), [Quality::Accurate]);
        assert_eq!(said.last(), Some(&InstallUpdate::Downloading { done: 35, total: 35 }));
        assert!(said.contains(&InstallUpdate::Downloading { done: 17, total: 35 }), "the first direction counts for its bytes: {said:?}");
        assert!(said.contains(&InstallUpdate::Checking));

        let folder = store::folder(&models, two.model());
        assert_eq!(fs::read(folder.join(WEIGHTS)).unwrap(), b"english to french!");
        let left: Vec<_> = fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left.len(), store::FILES.len(), "nothing unfinished stays behind: {left:?}");
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn weights_that_are_not_the_expected_ones_are_removed_and_never_installed() {
        let models = models("wrong");
        let mut shelf = Shelf::default();
        let source = source(FRENCH, b"the real weights", &mut shelf);
        shelf.files.insert(source.url(WEIGHTS), b"something else!!".to_vec());

        let (outcome, _) = installation(&shelf, &[&source], &models);
        assert!(matches!(outcome, Err(Stop::Failed(_))), "{outcome:?}");
        let folder = store::folder(&models, source.model());
        assert!(!folder.join(WEIGHTS).exists());
        assert!(!folder.join(format!("{WEIGHTS}.download")).exists());
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn a_download_that_stopped_goes_on_from_where_it_was() {
        let models = models("resumed");
        let mut shelf = Shelf::default();
        let source = source(FRENCH, b"first half, second half", &mut shelf);
        shelf.stop_after = Some(11);

        let (outcome, _) = installation(&shelf, &[&source], &models);
        assert_eq!(outcome, Err(Stop::Unwanted));
        let folder = store::folder(&models, source.model());
        assert_eq!(fs::read(folder.join(format!("{WEIGHTS}.download"))).unwrap(), b"first half,");
        assert!(!folder.join(WEIGHTS).exists());

        shelf.stop_after = None;
        let (outcome, said) = installation(&shelf, &[&source], &models);
        assert_eq!(outcome, Ok(()));
        assert_eq!(fs::read(folder.join(WEIGHTS)).unwrap(), b"first half, second half");
        assert_eq!(said.last(), Some(&InstallUpdate::Downloading { done: 23, total: 23 }));
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn what_is_installed_and_intact_is_not_downloaded_again() {
        let models = models("again");
        let mut shelf = Shelf::default();
        let source = source(FRENCH, b"weights", &mut shelf);
        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        let folder = store::folder(&models, source.model());
        fs::remove_file(folder.join("vocab.json")).unwrap();
        shelf.asked.lock().unwrap().clear();

        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        assert_eq!(*shelf.asked.lock().unwrap(), [source.url("vocab.json")]);

        // Weights that changed on disk are fetched again.
        fs::write(folder.join(WEIGHTS), b"damaged").unwrap();
        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        assert_eq!(fs::read(folder.join(WEIGHTS)).unwrap(), b"weights");
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn weights_in_32_bits_are_installed_in_16() {
        use candle_core::{DType, Device, Tensor};

        let models = models("narrowed");
        fs::create_dir_all(&models).unwrap();
        let table = Tensor::from_vec(vec![1.0f32, -2.5, 0.0, 1024.0], (2, 2), &Device::Cpu).unwrap();
        let published = models.join("published.safetensors");
        candle_core::safetensors::save(&HashMap::from([("model.shared.weight", table)]), &published).unwrap();
        let expected = models.join("expected.safetensors");
        narrow::to_f16(&published, &expected).unwrap();

        let mut shelf = Shelf::default();
        let mut source = source(FRENCH, &fs::read(&published).unwrap(), &mut shelf);
        source.installed = Some(hex(&fs::read(&expected).unwrap()));
        assert_ne!(Some(source.downloaded), source.installed);

        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        let folder = store::folder(&models, source.model());
        let installed = candle_core::safetensors::load(folder.join(WEIGHTS), &Device::Cpu).unwrap();
        assert_eq!(installed["model.shared.weight"].dtype(), DType::F16);
        assert!(!folder.join(format!("{WEIGHTS}.download")).exists(), "the 32-bit file is not kept");
        assert!(!folder.join(RECORD).exists(), "the table knows what was installed");

        // The 16-bit form is not what the table says: nothing is installed.
        source.installed = Some(hex(b"something else"));
        fs::remove_file(folder.join(WEIGHTS)).unwrap();
        assert!(matches!(installation(&shelf, &[&source], &models).0, Err(Stop::Failed(_))));
        assert!(!folder.join(WEIGHTS).exists());
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn weights_that_the_table_cannot_check_once_narrowed_are_checked_against_their_own_record() {
        use candle_core::{Device, Tensor};

        let models = models("recorded");
        fs::create_dir_all(&models).unwrap();
        let table = Tensor::from_vec(vec![0.5f32, 2.0], (1, 2), &Device::Cpu).unwrap();
        let published = models.join("published.safetensors");
        candle_core::safetensors::save(&HashMap::from([("model.shared.weight", table)]), &published).unwrap();

        let mut shelf = Shelf::default();
        let mut source = source(way(b"es", b"en"), &fs::read(&published).unwrap(), &mut shelf);
        source.installed = None;
        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        let folder = store::folder(&models, source.model());
        let narrowed = fs::read(folder.join(WEIGHTS)).unwrap();
        assert_eq!(fs::read_to_string(folder.join(RECORD)).unwrap().trim(), hex(&narrowed));
        assert!(Installed::scan(&models).translates(way(b"es", b"en")));

        // Intact: nothing is fetched. Changed on disk: fetched again.
        shelf.asked.lock().unwrap().clear();
        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        assert_eq!(*shelf.asked.lock().unwrap(), [] as [&str; 0]);
        fs::write(folder.join(WEIGHTS), b"damaged").unwrap();
        assert_eq!(installation(&shelf, &[&source], &models).0, Ok(()));
        assert_eq!(fs::read(folder.join(WEIGHTS)).unwrap(), narrowed);
        fs::remove_dir_all(models).ok();
    }

    #[test]
    fn a_language_is_its_two_small_models_to_english_and_back() {
        let spanish = Language::of(b"es");
        assert!(languages().contains(&spanish));
        assert!(!languages().contains(&Language::FRENCH) && !languages().contains(&Language::ENGLISH), "those are the sets");
        let directions: Vec<String> = models_of(spanish).iter().map(|source| source.direction.code()).collect();
        assert_eq!(directions, ["es-en", "en-es"]);
        assert_eq!(language_download_size(spanish), 624_125_048);
        assert_eq!(language_download_size(Language::of(b"xx")), 0);

        // Every language comes whole, and none comes twice.
        for language in languages() {
            assert_eq!(models_of(language).len(), 2, "{language:?}");
            assert_eq!(languages().iter().filter(|other| **other == language).count(), 1, "{language:?}");
        }
        // French that way is the light set.
        assert_eq!(language_download_size(Language::FRENCH), download_size(Quality::Light));
    }

    #[test]
    fn a_language_that_is_not_in_the_table_fails_at_once() {
        let models = models("unknown");
        let said: Vec<InstallUpdate> = futures_executor::block_on_stream(install_language(models.clone(), Language::of(b"xx"))).collect();
        assert_eq!(said, [InstallUpdate::Failed("There is no model to download for this language.".into())]);
        assert!(!models.exists());
    }

    #[test]
    fn a_gave_up_curl_is_explained_in_plain_words() {
        assert!(reason(Some(6), "curl: (6) Could not resolve host: huggingface.co").starts_with("Cannot reach huggingface.co"));
        assert_eq!(
            reason(Some(22), "curl: (22) The requested URL returned error: 503\n"),
            "The download stopped: The requested URL returned error: 503. Try again."
        );
        assert_eq!(reason(None, ""), "The download stopped. Try again.");
    }

    /// The app and the script must install the same files.
    #[test]
    fn the_table_is_the_one_of_the_script() {
        let script = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/fetch-models.sh")).expect("read fetch-models.sh");
        let rows: Vec<Vec<&str>> = script
            .lines()
            .map(|line| line.split(' ').collect::<Vec<_>>())
            .filter(|fields| fields.len() == 6 && matches!(fields[0], "light" | "accurate"))
            .collect();
        assert_eq!(rows.len(), SOURCES.len());
        for (row, source) in rows.iter().zip(&SOURCES) {
            assert_eq!(store::folder(Path::new(""), source.model()), Path::new(row[0]).join(row[1]));
            // A dash in the script: the SHA-256 once installed is not known.
            assert_eq!(row[2..], [source.repository, source.commit, source.downloaded, source.installed.unwrap_or("-")]);
        }
        assert_eq!(download_size(Quality::Light), 601_606_864);
        assert_eq!(download_size(Quality::Accurate), 922_858_260);
    }
}
