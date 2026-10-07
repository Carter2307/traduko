//! Offline translation between the languages whose models are installed.
//!
//! The UI talks to a [`Translator`]: a handle to one worker thread that owns
//! the models. A request returns a stream of [`Update`]s; a newer request, or
//! dropping the stream, cancels the one in progress.
//!
//! Inside, a text goes through these steps:
//!
//! 1. `text` takes it apart: sentences to translate, and the blanks, line
//!    breaks and list markers between them, which are copied.
//! 2. `cache` answers for the sentences seen before.
//! 3. `model` translates the others one by one: `spm` turns a sentence into
//!    token ids, `marian` is the network, `search` picks the output tokens.
//!    A model knows one direction. `store` finds the models on disk and
//!    the way from one language to another: one model, or two through
//!    English.
//! 4. `worker` puts the output back together, gives English the requested
//!    spelling, and sends it.
//!
//! The models themselves come from the Hub: [`install`] downloads a set and
//! checks it, for an app that was not set up by `scripts/fetch-models.sh`.
//! [`remove_unused`] takes off the disk the ones that no translation uses.

mod backend;
mod cache;
mod failure;
mod install;
mod language;
mod marian;
mod memory;
mod model;
mod narrow;
mod remove;
mod search;
mod spm;
mod store;
mod text;
mod weights;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, unbounded};

pub use dialect::EnglishVariant;
pub use install::{InstallUpdate, download_size, install, install_language, language_download_size, languages};
pub use language::{Direction, Language};
pub use memory::ReturnsLargeBlocks;
pub use narrow::to_f16;
pub use remove::remove_unused;
pub use store::Installed;

use backend::{Backend, OnDisk};
use worker::{Asked, Job};

/// Which model does the work, when the direction has both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    /// The small model: less memory, a little less accurate.
    Light,
    /// The larger model.
    Accurate,
}

impl Quality {
    /// The name of its folder of models, and of its set in the script.
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Quality::Light => "light",
            Quality::Accurate => "accurate",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub text: String,
    pub direction: Direction,
    /// Spelling and vocabulary of English output. Ignored when the text is
    /// translated to another language.
    pub english: EnglishVariant,
    /// The model to prefer. A direction that only the other quality has is
    /// translated by that one.
    pub quality: Quality,
}

#[derive(Clone, Debug)]
pub enum Update {
    /// The weights are being read from disk first.
    LoadingModel,
    /// One more sentence went through the model; `text` is the translation
    /// so far, from the start of the text to the first sentence that is not
    /// ready. Sentences that were translated before (while typing, most of
    /// the text) need no model: they are counted in `done` with the next
    /// update instead of getting one each.
    Partial { text: String, done: usize, total: usize },
    /// The whole translation. `elapsed` runs from the call to
    /// [`Translator::translate`]; `sentences` is how many were translated or
    /// recalled.
    Done { text: String, elapsed: Duration, sentences: usize },
    /// The request could not be served; the text says why, for the user.
    Failed(String),
}

/// How long a model stays in memory without being used.
const IDLE: Duration = Duration::from_secs(600);

/// Overrides [`IDLE`] with a number of seconds, to watch a model being
/// unloaded without waiting ten minutes.
const IDLE_VARIABLE: &str = "COCO_IDLE_SECS";

#[derive(Clone)]
pub struct Translator {
    jobs: mpsc::Sender<Job>,
}

impl Translator {
    /// Starts the worker thread. Nothing is loaded until the first request.
    pub fn start(models_dir: PathBuf) -> Self {
        let seconds = std::env::var(IDLE_VARIABLE).ok().and_then(|value| value.trim().parse().ok());
        Self::with_backend(OnDisk { models_dir }, seconds.map_or(IDLE, Duration::from_secs))
    }

    pub(crate) fn with_backend(backend: impl Backend, idle: Duration) -> Self {
        let (jobs, queue) = mpsc::channel();
        // If the thread cannot start, the queue is dropped with the closure
        // and every request fails with a message: see `translate`.
        let _ = std::thread::Builder::new().name("coco-translator".into()).spawn(move || worker::run(backend, queue, idle));
        Self { jobs }
    }

    /// Never blocks. The stream ends after `Done` or `Failed`, or without
    /// either when a newer request replaced this one.
    pub fn translate(&self, request: Request) -> UnboundedReceiver<Update> {
        let (updates, stream) = unbounded();
        let asked = Box::new(Asked { request, updates, at: Instant::now() });
        if let Err(mpsc::SendError(Job::Translate(asked))) = self.jobs.send(Job::Translate(asked)) {
            let _ = asked.updates.unbounded_send(Update::Failed("The translator is not running.".into()));
        }
        stream
    }

    /// Loads the models of a direction ahead of the first request.
    pub fn warm_up(&self, direction: Direction, quality: Quality) {
        // Without a worker there is nothing to warm up, and the next
        // request says so.
        let _ = self.jobs.send(Job::WarmUp(quality, direction));
    }

    /// The models found on disk.
    pub fn installed(models_dir: &Path) -> Installed {
        Installed::scan(models_dir)
    }
}

/// Where the app keeps its models: `$COCO_MODELS_DIR`, or
/// `~/Library/Application Support/Coco/models`.
pub fn default_models_dir() -> PathBuf {
    let from_home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Coco/models");
    std::env::var_os("COCO_MODELS_DIR").map_or_else(from_home, PathBuf::from)
}
