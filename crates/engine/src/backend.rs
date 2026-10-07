//! What the worker needs from a translation model, and the real thing.
//!
//! The worker only loads models and hands them sentences. Keeping that
//! behind two small traits lets the tests drive the worker (queue, cache,
//! unloading) with a model that answers at once and records what it was
//! asked.

use std::path::PathBuf;

use crate::failure::{Failure, Result};
use crate::model::Model;
use crate::store::{self, Hop, Installed};

/// Loads models. It moves to the worker thread; the models it makes never
/// leave it.
pub(crate) trait Backend: Send + 'static {
    type Model: Translate;

    /// The models that can be loaded now. This is asked at every request:
    /// a model may have been installed since the last one.
    fn installed(&self) -> Installed;

    fn load(&self, hop: Hop) -> Result<Self::Model>;
}

pub(crate) trait Translate {
    /// The translation of one sentence, as the model wrote it. `wanted` is
    /// to be asked during the work; when it says no, the model stops and
    /// gives `None`.
    fn translate(&mut self, sentence: &str, wanted: &mut (dyn FnMut() -> bool + Send)) -> Result<Option<String>>;
}

/// The Marian models of a models folder.
pub(crate) struct OnDisk {
    pub models_dir: PathBuf,
}

impl Backend for OnDisk {
    type Model = Model;

    fn installed(&self) -> Installed {
        Installed::scan(&self.models_dir)
    }

    fn load(&self, hop: Hop) -> Result<Model> {
        let folder = store::folder(&self.models_dir, hop);
        // It was there when the route was chosen, a moment ago.
        if !store::is_complete(&folder) {
            let (quality, direction) = (hop.quality.folder(), hop.direction);
            return Err(Failure::new(format!("The {quality} model from {} to {} is not installed.", direction.from.name(), direction.to.name())));
        }
        Model::load(&folder)
    }
}

impl Translate for Model {
    fn translate(&mut self, sentence: &str, wanted: &mut (dyn FnMut() -> bool + Send)) -> Result<Option<String>> {
        Model::translate(self, sentence, wanted)
    }
}
