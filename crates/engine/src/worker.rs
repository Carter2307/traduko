//! The thread that owns the models.
//!
//! Everything slow happens here, one job at a time: reading weights from
//! disk, translating, freeing a model nobody used for a while. The UI only
//! puts jobs in a queue and reads updates from a stream, so it never waits.
//!
//! Only the newest request matters. A request that is still in the queue
//! when a newer one arrives is never started, and the one in progress is
//! dropped at its next output token: its stream just ends. The same happens
//! when the UI drops the stream.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedSender;

use crate::backend::{Backend, Translate};
use crate::cache::{self, SentenceCache};
use crate::failure::{Failure, Result};
use crate::store::{Hop, Installed};
use crate::text::Layout;
use crate::{Direction, Language, Quality, Request, Update};

pub(crate) enum Job {
    Translate(Box<Asked>),
    WarmUp(Quality, Direction),
}

/// A request and where its updates go.
pub(crate) struct Asked {
    pub request: Request,
    pub updates: UnboundedSender<Update>,
    pub at: Instant,
}

/// Runs until every handle to the queue is dropped.
pub(crate) fn run<B: Backend>(backend: B, jobs: Receiver<Job>, idle: Duration) {
    let mut worker = Worker {
        backend,
        inbox: Inbox { jobs, request: None, warm_up: None, closed: false },
        models: Vec::new(),
        cache: SentenceCache::new(cache::CAPACITY),
        idle,
    };
    loop {
        worker.unload_idle();
        match worker.inbox.next(worker.next_unload()) {
            Some(Job::Translate(asked)) => worker.translate(*asked),
            Some(Job::WarmUp(quality, direction)) => worker.warm_up(quality, direction),
            None if worker.inbox.closed => return,
            None => {}
        }
    }
}

/// The jobs that arrived and are not started yet: at most one request (the
/// newest) and one warm-up.
struct Inbox {
    jobs: Receiver<Job>,
    request: Option<Box<Asked>>,
    warm_up: Option<(Quality, Direction)>,
    /// Every `Translator` is gone: nothing will arrive any more.
    closed: bool,
}

impl Inbox {
    /// Takes what is in the queue, without waiting. Replacing an older
    /// request drops it, which ends its stream.
    fn collect(&mut self) {
        loop {
            match self.jobs.try_recv() {
                Ok(job) => self.keep(job),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    return;
                }
            }
        }
    }

    fn keep(&mut self, job: Job) {
        match job {
            Job::Translate(asked) => self.request = Some(asked),
            Job::WarmUp(quality, direction) => self.warm_up = Some((quality, direction)),
        }
    }

    /// The next job, waiting for one until `deadline` (for ever without
    /// one). A request goes before a warm-up: somebody is looking at it.
    fn next(&mut self, deadline: Option<Instant>) -> Option<Job> {
        loop {
            self.collect();
            if let Some(asked) = self.request.take() {
                return Some(Job::Translate(asked));
            }
            if let Some((quality, direction)) = self.warm_up.take() {
                return Some(Job::WarmUp(quality, direction));
            }
            if self.closed {
                return None;
            }
            let arrived = match deadline {
                Some(deadline) => self.jobs.recv_timeout(deadline.saturating_duration_since(Instant::now())),
                None => self.jobs.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match arrived {
                Ok(job) => self.keep(job),
                Err(RecvTimeoutError::Timeout) => return None,
                Err(RecvTimeoutError::Disconnected) => self.closed = true,
            }
        }
    }

    fn has_newer_request(&mut self) -> bool {
        self.collect();
        self.request.is_some()
    }
}

struct Loaded<M> {
    hop: Hop,
    model: M,
    last_used: Instant,
}

struct Worker<B: Backend> {
    backend: B,
    inbox: Inbox,
    /// The models in use: those of the direction that was last asked for
    /// and of the way back. Two most of the time, four when both ways go
    /// through English.
    models: Vec<Loaded<B::Model>>,
    cache: SentenceCache,
    /// A model that nobody used for this long is dropped.
    idle: Duration,
}

impl<B: Backend> Worker<B> {
    fn translate(&mut self, asked: Asked) {
        let Asked { request, updates, at } = asked;
        // The receiver may be gone already; that is not an error.
        let send = |update: Update| drop(updates.unbounded_send(update));
        let (quality, direction) = (request.quality, request.direction);

        let layout = Layout::of(&request.text);
        let sentences: Vec<&str> = layout.sentences().collect();
        // A text without a sentence (blanks, a time, a link) is given back
        // as it is, whatever the models on disk.
        let installed = if sentences.is_empty() { Installed::default() } else { self.backend.installed() };
        let route = match installed.route(quality, direction) {
            Some(route) => route,
            None if sentences.is_empty() => Vec::new(),
            None => return send(Update::Failed(no_route(direction))),
        };
        let mut translations: Vec<Option<String>> = sentences.iter().map(|sentence| self.recall(&route, sentence)).collect();

        let all_known = translations.iter().all(Option::is_some);
        if !all_known && route.iter().any(|hop| self.loaded(*hop).is_none()) {
            if !still_wanted(&mut self.inbox, &updates) {
                return;
            }
            send(Update::LoadingModel);
            if let Err(failure) = self.load(&installed, quality, direction) {
                return send(Update::Failed(failure.into_message()));
            }
        }

        for (index, sentence) in sentences.iter().enumerate() {
            if translations[index].is_some() {
                continue;
            }
            // The same sentence earlier in this text is known by now.
            if let Some(known) = self.recall(&route, sentence) {
                translations[index] = Some(known);
                continue;
            }
            match self.translate_sentence(&route, sentence, &updates) {
                Ok(Some(translation)) => translations[index] = Some(translation),
                // Nobody waits for it any more.
                Ok(None) => return,
                Err(failure) => return send(Update::Failed(failure.into_message())),
            }
            // The sentences after this one that were already known count
            // as done with it: the text never shrinks back to less than what
            // is ready.
            let ready: Vec<&str> = translations.iter().map_while(Option::as_deref).collect();
            let text = in_requested_english(layout.render(&ready), &request);
            send(Update::Partial { text, done: ready.len(), total: sentences.len() });
        }

        let all: Vec<&str> = translations.iter().flatten().map(String::as_str).collect();
        let text = match request.text.trim() {
            "" => String::new(),
            _ => in_requested_english(layout.render(&all), &request),
        };
        send(Update::Done { text, elapsed: at.elapsed(), sentences: sentences.len() });
    }

    /// What `sentence` became along `route` the last time, when every model
    /// on the way has seen its part of it.
    fn recall(&mut self, route: &[Hop], sentence: &str) -> Option<String> {
        let mut text = sentence.to_string();
        for hop in route {
            text = self.cache.get(*hop, &text)?;
        }
        Some(text)
    }

    /// Takes a sentence through the models of `route`, one after the other.
    /// `None` when the request was dropped or replaced during the work.
    fn translate_sentence(&mut self, route: &[Hop], sentence: &str, updates: &UnboundedSender<Update>) -> Result<Option<String>> {
        let mut text = sentence.to_string();
        for hop in route {
            // The first half of a way through English is often known:
            // the same sentence was translated to another language.
            text = match self.cache.get(*hop, &text) {
                Some(known) => known,
                None => {
                    let Some(translation) = self.run(*hop, &text, updates)? else { return Ok(None) };
                    self.cache.put(*hop, &text, &translation);
                    translation
                }
            };
        }
        Ok(Some(text))
    }

    /// One sentence through one model.
    fn run(&mut self, hop: Hop, sentence: &str, updates: &UnboundedSender<Update>) -> Result<Option<String>> {
        let Self { models, inbox, .. } = self;
        let Some(loaded) = models.iter_mut().find(|loaded| loaded.hop == hop) else {
            return Err(Failure::new("The model is not loaded."));
        };
        let outcome = guarded(|| loaded.model.translate(sentence, &mut || still_wanted(inbox, updates)));
        loaded.last_used = Instant::now();
        if outcome.is_err() {
            // Whatever state the failure left behind goes with the model.
            models.retain(|loaded| loaded.hop != hop);
        }
        outcome
    }

    fn warm_up(&mut self, quality: Quality, direction: Direction) {
        dialect::warm_up();
        // A failure is reported by the request that follows.
        let _ = self.load(&self.backend.installed(), quality, direction);
    }

    fn loaded(&mut self, hop: Hop) -> Option<&mut Loaded<B::Model>> {
        self.models.iter_mut().find(|loaded| loaded.hop == hop)
    }

    /// Reads the models of `direction` that are not in memory yet.
    fn load(&mut self, installed: &Installed, quality: Quality, direction: Direction) -> Result<()> {
        let Some(route) = installed.route(quality, direction) else { return Err(Failure::new(no_route(direction))) };
        // The user changed the languages or the quality setting: the models
        // of the earlier choice will not be asked again, and their memory is
        // needed now. Those of the way back stay, for a swap.
        let back = installed.route(quality, direction.swapped()).unwrap_or_default();
        let before = self.models.len();
        self.models.retain(|loaded| route.contains(&loaded.hop) || back.contains(&loaded.hop));
        if self.models.len() < before {
            give_memory_back();
        }
        for hop in route {
            if self.loaded(hop).is_none() {
                let model = guarded(|| self.backend.load(hop))?;
                self.models.push(Loaded { hop, model, last_used: Instant::now() });
            }
        }
        Ok(())
    }

    fn unload_idle(&mut self) {
        let (idle, before) = (self.idle, self.models.len());
        self.models.retain(|loaded| loaded.last_used.elapsed() < idle);
        if self.models.len() < before {
            give_memory_back();
        }
    }

    /// When the next model becomes idle, if any is loaded.
    fn next_unload(&self) -> Option<Instant> {
        self.models.iter().map(|loaded| loaded.last_used + self.idle).min()
    }
}

/// What the user reads when the models on disk cannot serve a direction.
fn no_route(direction: Direction) -> String {
    format!("No model is installed to translate {} to {}.", direction.from.name(), direction.to.name())
}

/// Asks the allocator to return freed pages to the system. Without this,
/// macOS keeps most of a dropped model's pages in the process (measured:
/// 162 MB before and after dropping the light model), and the app that
/// runs all day would stay as heavy as its last translation.
fn give_memory_back() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means every zone and a goal of 0 means as much
        // as possible; the call only touches memory that is already free.
        unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    }
}

/// False when the UI dropped the stream of a request or sent a newer one.
fn still_wanted(inbox: &mut Inbox, updates: &UnboundedSender<Update>) -> bool {
    !updates.is_closed() && !inbox.has_newer_request()
}

/// English output gets the spelling the user asked for. This is done on the
/// whole text, after the cache: the cache holds what the model wrote, so
/// switching between American and British translates nothing again.
fn in_requested_english(text: String, request: &Request) -> String {
    if request.direction.to == Language::ENGLISH {
        dialect::convert(&text, request.english, &dialect::Options::default())
    } else {
        text
    }
}

/// Runs model code so that a panic inside it becomes a failed request
/// instead of the end of the worker thread.
fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(Failure::new("The translator hit an unexpected error.")))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};

    use futures_channel::mpsc::UnboundedReceiver;

    use super::*;
    use crate::{EnglishVariant, Translator};
    use Quality::{Accurate, Light};

    const FR_EN: Direction = Direction::new(Language::FRENCH, Language::ENGLISH);
    const EN_FR: Direction = FR_EN.swapped();

    fn direction(code: &str) -> Direction {
        Direction::from_code(code).expect("a direction")
    }

    fn hop(quality: Quality, code: &str) -> Hop {
        Hop { quality, direction: direction(code) }
    }

    #[derive(Debug, PartialEq)]
    enum Event {
        Loaded(Quality, Direction),
        Unloaded(Quality, Direction),
        Translating(String),
    }

    /// A back end whose models put the sentence between brackets, and tell
    /// the test what they are doing.
    struct Fake {
        /// What it says is on disk.
        installed: Installed,
        events: Sender<Event>,
        /// When set, a model waits for a permit before each sentence.
        permits: Option<Arc<Mutex<Receiver<()>>>>,
    }

    struct FakeModel {
        hop: Hop,
        events: Sender<Event>,
        permits: Option<Arc<Mutex<Receiver<()>>>>,
    }

    impl Backend for Fake {
        type Model = FakeModel;

        fn installed(&self) -> Installed {
            self.installed.clone()
        }

        fn load(&self, hop: Hop) -> Result<FakeModel> {
            // On disk, but it cannot be read.
            if hop == (Hop { quality: Accurate, direction: EN_FR }) {
                return Err(Failure::damaged("config.json", "it is empty"));
            }
            self.events.send(Event::Loaded(hop.quality, hop.direction)).ok();
            Ok(FakeModel { hop, events: self.events.clone(), permits: self.permits.clone() })
        }
    }

    impl Translate for FakeModel {
        fn translate(&mut self, sentence: &str, wanted: &mut (dyn FnMut() -> bool + Send)) -> Result<Option<String>> {
            if !wanted() {
                return Ok(None);
            }
            self.events.send(Event::Translating(sentence.to_string())).ok();
            if let Some(permits) = &self.permits {
                permits.lock().expect("the permits").recv().ok();
            }
            match sentence {
                "Broken." => Err(Failure::new("The model stopped with an error: broken.")),
                "Panic." => panic!("a bug in the model"),
                // Like the real model, it looks up from its work now and then.
                _ => Ok(wanted().then(|| format!("[{sentence}]"))),
            }
        }
    }

    impl Drop for FakeModel {
        fn drop(&mut self) {
            self.events.send(Event::Unloaded(self.hop.quality, self.hop.direction)).ok();
        }
    }

    struct Harness {
        translator: Translator,
        events: Receiver<Event>,
        permits: Sender<()>,
    }

    const LONG: Duration = Duration::from_secs(600);

    /// French and English in both qualities, and German in the small one.
    const ON_DISK: [(Quality, &str); 6] =
        [(Light, "fr-en"), (Light, "en-fr"), (Accurate, "fr-en"), (Accurate, "en-fr"), (Light, "de-en"), (Light, "en-de")];

    fn start(idle: Duration, held: bool) -> Harness {
        start_with(&ON_DISK, idle, held)
    }

    fn start_with(on_disk: &[(Quality, &str)], idle: Duration, held: bool) -> Harness {
        let (events, seen) = channel();
        let (permits, taken) = channel();
        let installed = Installed::of(on_disk.iter().map(|(quality, code)| hop(*quality, code)).collect());
        let fake = Fake { installed, events, permits: held.then(|| Arc::new(Mutex::new(taken))) };
        Harness { translator: Translator::with_backend(fake, idle), events: seen, permits }
    }

    impl Harness {
        fn ask(&self, text: &str, direction: Direction, quality: Quality) -> UnboundedReceiver<Update> {
            let english = EnglishVariant::American;
            self.translator.translate(Request { text: text.to_string(), direction, english, quality })
        }

        fn next_event(&self) -> Event {
            self.events.recv_timeout(Duration::from_secs(10)).expect("the model did nothing for ten seconds")
        }

        /// The sentences that went through a model so far.
        fn translated(&self) -> Vec<String> {
            let sentence = |event| match event {
                Event::Translating(sentence) => Some(sentence),
                _ => None,
            };
            self.events.try_iter().filter_map(sentence).collect()
        }
    }

    /// Every update of a stream until it ends, in a form that is easy to
    /// compare.
    fn updates(stream: UnboundedReceiver<Update>) -> Vec<String> {
        let describe = |update| match update {
            Update::LoadingModel => "loading".to_string(),
            Update::Partial { text, done, total } => format!("{done}/{total} {text:?}"),
            Update::Done { text, sentences, .. } => format!("done {sentences} {text:?}"),
            Update::Failed(message) => format!("failed {message}"),
        };
        futures_executor::block_on_stream(stream).map(describe).collect()
    }

    #[test]
    fn each_sentence_gives_an_update_and_the_layout_is_kept() {
        let harness = start(LONG, false);
        let stream = harness.ask("  - Un. Deux.\n\nTrois\n", EN_FR, Light);
        assert_eq!(updates(stream), [
            "loading",
            r#"1/3 "  - [Un.]""#,
            r#"2/3 "  - [Un.] [Deux.]""#,
            r#"3/3 "  - [Un.] [Deux.]\n\n[Trois]\n""#,
            r#"done 3 "  - [Un.] [Deux.]\n\n[Trois]\n""#,
        ]);
    }

    #[test]
    fn empty_input_is_done_at_once_and_loads_nothing() {
        let harness = start(LONG, false);
        assert_eq!(updates(harness.ask("", FR_EN, Light)), [r#"done 0 """#]);
        assert_eq!(updates(harness.ask(" \n\t ", FR_EN, Light)), [r#"done 0 """#]);
        // Nothing to translate either, but something to give back.
        assert_eq!(updates(harness.ask("12:30\n", FR_EN, Light)), [r#"done 0 "12:30\n""#]);
        // Even in a direction that has no model.
        assert_eq!(updates(harness.ask("12:30\n", direction("fr-es"), Light)), [r#"done 0 "12:30\n""#]);
        assert!(harness.events.try_recv().is_err(), "no model was needed");
    }

    #[test]
    fn only_the_sentence_that_changed_is_translated_again() {
        let harness = start(LONG, false);
        updates(harness.ask("Un. Deux. Trois.", EN_FR, Light));
        assert_eq!(harness.translated(), ["Un.", "Deux.", "Trois."]);

        // The known sentences around the new one come with it, in one update.
        let stream = harness.ask("Un. Autre. Trois.", EN_FR, Light);
        assert_eq!(updates(stream), [r#"3/3 "[Un.] [Autre.] [Trois.]""#, r#"done 3 "[Un.] [Autre.] [Trois.]""#]);
        assert_eq!(harness.translated(), ["Autre."]);

        // A text that is known entirely is answered without the model.
        assert_eq!(updates(harness.ask("Trois. Un.", EN_FR, Light)), [r#"done 2 "[Trois.] [Un.]""#]);
        assert_eq!(harness.translated(), [] as [&str; 0]);

        // A sentence that comes several times in a text is translated once.
        updates(harness.ask("Encore. Encore. Fin. Encore.", EN_FR, Light));
        assert_eq!(harness.translated(), ["Encore.", "Fin."]);
    }

    #[test]
    fn a_newer_request_replaces_the_one_in_progress() {
        let harness = start(LONG, true);
        let first = harness.ask("Un. Deux. Trois.", EN_FR, Light);
        assert_eq!(harness.next_event(), Event::Loaded(Light, EN_FR));
        assert_eq!(harness.next_event(), Event::Translating("Un.".to_string()));

        // Two more arrive while the first sentence is in the model.
        let second = harness.ask("Jamais.", EN_FR, Light);
        let third = harness.ask("Quatre.", EN_FR, Light);
        (0..2).for_each(|_| harness.permits.send(()).expect("a waiting model"));

        // The first one stops in the middle of its sentence; the second one
        // is never started; neither gets `Done`.
        assert_eq!(updates(first), ["loading"]);
        assert_eq!(updates(second), [] as [&str; 0]);
        assert_eq!(updates(third), [r#"1/1 "[Quatre.]""#, r#"done 1 "[Quatre.]""#]);
        assert_eq!(harness.translated(), ["Quatre."]);

        // The sentence that was cut short was not remembered as translated.
        let again = harness.ask("Un.", EN_FR, Light);
        harness.permits.send(()).expect("a waiting model");
        assert_eq!(updates(again), [r#"1/1 "[Un.]""#, r#"done 1 "[Un.]""#]);
        assert_eq!(harness.translated(), ["Un."]);
    }

    #[test]
    fn dropping_the_stream_stops_the_work() {
        let harness = start(LONG, true);
        let stream = harness.ask("Un. Deux. Trois.", EN_FR, Light);
        assert_eq!(harness.next_event(), Event::Loaded(Light, EN_FR));
        assert_eq!(harness.next_event(), Event::Translating("Un.".to_string()));
        drop(stream);
        harness.permits.send(()).expect("a waiting model");

        // The worker is free for the next request: "Un." was given up and
        // "Deux." never reached the model.
        let next = harness.ask("Quatre.", EN_FR, Light);
        harness.permits.send(()).expect("a waiting model");
        assert_eq!(updates(next), [r#"1/1 "[Quatre.]""#, r#"done 1 "[Quatre.]""#]);
        assert_eq!(harness.translated(), ["Quatre."]);
    }

    #[test]
    fn english_spelling_is_applied_to_the_whole_text_and_not_stored() {
        let harness = start(LONG, false);
        let ask = |english| {
            let text = "My favourite colour. The theater is in the centre.".to_string();
            updates(harness.translator.translate(Request { text, direction: FR_EN, english, quality: Light }))
        };

        let american = ask(EnglishVariant::American);
        assert_eq!(american[1], r#"1/2 "[My favorite color.]""#);
        assert_eq!(american[3], r#"done 2 "[My favorite color.] [The theater is in the center.]""#);

        // The same request in British English: nothing goes through the
        // model again, and the stored text was not the American one.
        harness.translated();
        let british = ask(EnglishVariant::British);
        assert_eq!(british, [r#"done 2 "[My favourite colour.] [The theatre is in the centre.]""#]);
        assert_eq!(harness.translated(), [] as [&str; 0]);
    }

    #[test]
    fn english_spelling_is_for_english_output_from_any_language() {
        let harness = start(LONG, false);
        let ask = |direction, english| {
            let text = "My favourite colour.".to_string();
            updates(harness.translator.translate(Request { text, direction, english, quality: Light })).pop()
        };
        assert_eq!(ask(direction("de-en"), EnglishVariant::American).as_deref(), Some(r#"done 1 "[My favorite color.]""#));
        // Another language is left as the model wrote it, and so is the
        // English in the middle of a translation that goes through it.
        assert_eq!(ask(EN_FR, EnglishVariant::American).as_deref(), Some(r#"done 1 "[My favourite colour.]""#));
        assert_eq!(ask(direction("fr-de"), EnglishVariant::American).as_deref(), Some(r#"done 1 "[[My favourite colour.]]""#));
    }

    #[test]
    fn the_model_is_announced_only_when_it_is_read_from_disk() {
        let harness = start(LONG, false);
        assert_eq!(updates(harness.ask("Un.", EN_FR, Light))[0], "loading");
        assert_eq!(updates(harness.ask("Deux.", EN_FR, Light)), [r#"1/1 "[Deux.]""#, r#"done 1 "[Deux.]""#]);
        // The other direction is another model.
        assert_eq!(updates(harness.ask("Trois.", FR_EN, Light))[0], "loading");
    }

    #[test]
    fn warming_up_loads_the_model_before_the_first_request() {
        let harness = start(LONG, false);
        harness.translator.warm_up(FR_EN, Light);
        assert_eq!(harness.next_event(), Event::Loaded(Light, FR_EN));
        harness.translator.warm_up(FR_EN, Light);
        assert_eq!(updates(harness.ask("Un.", FR_EN, Light)), [r#"1/1 "[Un.]""#, r#"done 1 "[Un.]""#]);
        let events: Vec<Event> = harness.events.try_iter().collect();
        assert_eq!(events, [Event::Translating("Un.".to_string())], "the second warm-up loaded nothing");
    }

    #[test]
    fn an_idle_model_is_unloaded_and_read_again_when_needed() {
        let harness = start(Duration::from_millis(50), false);
        updates(harness.ask("Un.", EN_FR, Light));
        assert_eq!(harness.next_event(), Event::Loaded(Light, EN_FR));
        assert_eq!(harness.next_event(), Event::Translating("Un.".to_string()));
        assert_eq!(harness.next_event(), Event::Unloaded(Light, EN_FR));

        // What is in the cache needs no model; a new sentence does.
        assert_eq!(updates(harness.ask("Un.", EN_FR, Light)), [r#"done 1 "[Un.]""#]);
        assert_eq!(updates(harness.ask("Deux.", EN_FR, Light))[0], "loading");
    }

    #[test]
    fn changing_the_quality_unloads_the_models_of_the_other_one() {
        let harness = start(LONG, false);
        updates(harness.ask("Un.", EN_FR, Light));
        updates(harness.ask("Un.", FR_EN, Light));
        updates(harness.ask("Un.", FR_EN, Accurate));
        let events: Vec<Event> = harness.events.try_iter().collect();
        assert_eq!(events[4..], [
            Event::Unloaded(Light, EN_FR),
            Event::Unloaded(Light, FR_EN),
            Event::Loaded(Accurate, FR_EN),
            Event::Translating("Un.".to_string()),
        ]);
    }

    #[test]
    fn two_languages_without_a_model_of_their_own_go_through_english() {
        let harness = start(LONG, false);
        let french_to_german = direction("fr-de");
        let stream = harness.ask("Un. Deux.", french_to_german, Light);
        assert_eq!(updates(stream), ["loading", r#"1/2 "[[Un.]]""#, r#"2/2 "[[Un.]] [[Deux.]]""#, r#"done 2 "[[Un.]] [[Deux.]]""#]);
        let events: Vec<Event> = harness.events.try_iter().collect();
        assert_eq!(events, [
            Event::Loaded(Light, FR_EN),
            Event::Loaded(Light, direction("en-de")),
            Event::Translating("Un.".to_string()),
            Event::Translating("[Un.]".to_string()),
            Event::Translating("Deux.".to_string()),
            Event::Translating("[Deux.]".to_string()),
        ]);

        // Each half was remembered on its own: the English one answers a
        // request for English, and only the new sentence is worked on.
        assert_eq!(updates(harness.ask("Deux. Un.", FR_EN, Light)), [r#"done 2 "[Deux.] [Un.]""#]);
        assert_eq!(updates(harness.ask("Un. Trois.", french_to_german, Light))[1..], [r#"done 2 "[[Un.]] [[Trois.]]""#]);
        assert_eq!(harness.translated(), ["Trois.", "[Trois.]"]);
    }

    #[test]
    fn the_quality_that_has_the_direction_is_the_one_that_translates() {
        let harness = start_with(&[(Accurate, "fr-en"), (Light, "en-fr"), (Light, "en-de")], LONG, false);
        // Asked of the large models, which have neither English to French
        // nor German.
        assert_eq!(updates(harness.ask("Un.", EN_FR, Accurate)), ["loading", r#"1/1 "[Un.]""#, r#"done 1 "[Un.]""#]);
        assert_eq!(harness.next_event(), Event::Loaded(Light, EN_FR));
        updates(harness.ask("Deux.", direction("fr-de"), Accurate));
        let loaded: Vec<Event> = harness.events.try_iter().filter(|event| matches!(event, Event::Loaded(..))).collect();
        assert_eq!(loaded, [Event::Loaded(Accurate, FR_EN), Event::Loaded(Light, direction("en-de"))]);
    }

    #[test]
    fn the_models_of_the_way_back_stay_and_the_others_go() {
        let harness = start(LONG, false);
        updates(harness.ask("Un.", direction("fr-de"), Light));
        // French to English: its own model is in memory, and English to
        // French is the way back. German is not asked any more.
        updates(harness.ask("Deux.", EN_FR, Light));
        let events: Vec<Event> = harness.events.try_iter().collect();
        assert_eq!(events[4..], [
            Event::Unloaded(Light, direction("en-de")),
            Event::Loaded(Light, EN_FR),
            Event::Translating("Deux.".to_string()),
        ]);
        // A swap loads nothing and unloads nothing.
        assert_eq!(updates(harness.ask("Trois.", FR_EN, Light)), [r#"1/1 "[Trois.]""#, r#"done 1 "[Trois.]""#]);
    }

    #[test]
    fn a_direction_without_a_model_fails_the_request_with_a_message() {
        let harness = start(LONG, false);
        let stream = harness.ask("Un.", direction("fr-es"), Light);
        assert_eq!(updates(stream), ["failed No model is installed to translate French to Spanish."]);
        assert!(harness.events.try_recv().is_err(), "nothing was loaded");
    }

    #[test]
    fn a_model_that_cannot_be_loaded_fails_the_request_with_a_message() {
        let harness = start(LONG, false);
        let stream = harness.ask("Un.", EN_FR, Accurate);
        assert_eq!(updates(stream), ["loading", "failed The model file config.json is damaged: it is empty."]);
    }

    #[test]
    fn a_failure_or_a_panic_in_the_model_fails_the_request_and_nothing_else() {
        let harness = start(LONG, false);
        let stream = harness.ask("Un. Broken. Trois.", EN_FR, Light);
        assert_eq!(updates(stream)[1..], [r#"1/3 "[Un.]""#, "failed The model stopped with an error: broken."]);
        let stream = harness.ask("Panic.", EN_FR, Light);
        assert_eq!(updates(stream), ["loading", "failed The translator hit an unexpected error."]);
        // The thread is still there, with a fresh model.
        let stream = harness.ask("Quatre.", EN_FR, Light);
        assert_eq!(updates(stream), ["loading", r#"1/1 "[Quatre.]""#, r#"done 1 "[Quatre.]""#]);
    }

    #[test]
    fn the_worker_stops_when_the_last_handle_is_dropped() {
        let harness = start(LONG, false);
        updates(harness.ask("Un.", EN_FR, Light));
        let Harness { translator, events, .. } = harness;
        let other_handle = translator.clone();
        drop(translator);
        drop(other_handle);
        let unloaded = |event| event == Event::Unloaded(Light, EN_FR);
        assert!(events.iter().any(unloaded), "the model goes with the thread");
    }
}
