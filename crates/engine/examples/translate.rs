//! Translates from the command line, through the same API as the app.
//!
//!     cargo run --release -p traduko-engine --example translate -- \
//!         [--quality light|accurate] [--english american|british] \
//!         [--dir FROM-TO] [--models DIR] [--linger SECONDS] \
//!         [--testset FILE | "text"]
//!
//! `--dir` takes two language codes, like `fr-en` (the default) or `es-de`:
//! any direction that the models on disk can serve, through English when
//! it has no model of its own.
//!
//! With a text, it prints each update as it arrives, then the translation.
//! With `--testset` (a JSON file of lists named after their direction, like
//! `fr_to_en` and `en_to_fr`) it translates every entry and prints how long
//! each one took; `--dir` then keeps one of the lists. `--linger` waits
//! before leaving and
//! prints the memory of the process before and after: with a short
//! `TRADUKO_IDLE_SECS`, that shows a model being unloaded.

#[global_allocator]
static ALLOCATOR: traduko_engine::ReturnsLargeBlocks = traduko_engine::ReturnsLargeBlocks;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use traduko_engine::{Direction, EnglishVariant, Language, Quality, Request, Translator, Update, default_models_dir};

struct Options {
    quality: Quality,
    english: EnglishVariant,
    direction: Option<Direction>,
    models: PathBuf,
    testset: Option<PathBuf>,
    linger: Option<Duration>,
    text: Option<String>,
}

const USAGE: &str = "usage: translate [--quality light|accurate] [--english american|british] \
[--dir FROM-TO] [--models DIR] [--linger SECONDS] [--testset FILE | \"text\"]";

const FRENCH_TO_ENGLISH: Direction = Direction::new(Language::FRENCH, Language::ENGLISH);

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("translate: {problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let installed = Translator::installed(&options.models);
    if installed.is_empty() {
        eprintln!("translate: there is no model in {}. Run scripts/fetch-models.sh.", options.models.display());
        return ExitCode::FAILURE;
    }

    let translator = Translator::start(options.models.clone());
    let outcome = match (&options.testset, &options.text) {
        (Some(path), _) => run_testset(&translator, &options, path),
        (None, Some(text)) => run_text(&translator, &options, text),
        (None, None) => Err(format!("nothing to translate\n{USAGE}")),
    };
    if let Err(problem) = outcome {
        eprintln!("translate: {problem}");
        return ExitCode::FAILURE;
    }
    if let Some(linger) = options.linger {
        eprintln!("memory now: {}", resident_memory());
        std::thread::sleep(linger);
        eprintln!("memory after {} s: {}", linger.as_secs(), resident_memory());
    }
    ExitCode::SUCCESS
}

fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        quality: Quality::Light,
        english: EnglishVariant::American,
        direction: None,
        models: default_models_dir(),
        testset: None,
        linger: None,
        text: None,
    };
    while let Some(argument) = arguments.next() {
        let mut value = || arguments.next().ok_or(format!("{argument} needs a value"));
        match argument.as_str() {
            "--quality" => {
                options.quality = match value()?.as_str() {
                    "light" => Quality::Light,
                    "accurate" => Quality::Accurate,
                    other => return Err(format!("unknown quality {other}")),
                }
            }
            "--english" => {
                options.english = match value()?.as_str() {
                    "american" => EnglishVariant::American,
                    "british" => EnglishVariant::British,
                    other => return Err(format!("unknown English {other}")),
                }
            }
            "--dir" => {
                let code = value()?;
                options.direction = Some(Direction::from_code(&code).ok_or(format!("{code} is not a direction like fr-en"))?)
            }
            "--models" => options.models = PathBuf::from(value()?),
            "--testset" => options.testset = Some(PathBuf::from(value()?)),
            "--linger" => {
                let seconds: u64 = value()?.parse().map_err(|_| "--linger takes a number of seconds")?;
                options.linger = Some(Duration::from_secs(seconds));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => options.text = Some(argument),
        }
    }
    Ok(options)
}

/// One request, from the call to the last update.
struct Run {
    text: String,
    sentences: usize,
    /// As the engine reports it.
    elapsed: Duration,
    /// When each update arrived, with a short description.
    timeline: Vec<(Duration, String)>,
}

fn translate(translator: &Translator, options: &Options, direction: Direction, text: &str) -> Result<Run, String> {
    let started = Instant::now();
    let request = Request { text: text.to_string(), direction, english: options.english, quality: options.quality };
    let mut timeline = Vec::new();
    for update in futures_executor::block_on_stream(translator.translate(request)) {
        let at = started.elapsed();
        match update {
            Update::LoadingModel => timeline.push((at, "loading the model".to_string())),
            Update::Partial { done, total, .. } => timeline.push((at, format!("sentence {done} of {total}"))),
            Update::Done { text, elapsed, sentences } => return Ok(Run { text, sentences, elapsed, timeline }),
            Update::Failed(message) => return Err(message),
        }
    }
    Err("the request was dropped".to_string())
}

fn run_text(translator: &Translator, options: &Options, text: &str) -> Result<(), String> {
    let direction = options.direction.unwrap_or(FRENCH_TO_ENGLISH);
    let run = translate(translator, options, direction, text)?;
    for (at, what) in &run.timeline {
        eprintln!("{:>8.1} ms  {what}", milliseconds(*at));
    }
    eprintln!("{:>8.1} ms  done: {} sentence(s), {} words in", milliseconds(run.elapsed), run.sentences, text.split_whitespace().count());
    println!("{}", run.text);
    Ok(())
}

fn run_testset(translator: &Translator, options: &Options, path: &PathBuf) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let lists: std::collections::BTreeMap<String, Vec<String>> =
        serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;

    for (key, entries) in &lists {
        // "fr_to_en" is the list of French to English.
        let Some(direction) = Direction::from_code(&key.replace("_to_", "-")) else { continue };
        if options.direction.is_some_and(|only| only != direction) {
            continue;
        }
        let name = &direction.code();

        // The first request reads the weights; a second one of the same
        // size tells how much of its time that was. Any two words do: they
        // only have to be new to the model.
        let (cold, warm) = if direction.from == Language::FRENCH { ("Salut.", "Bonsoir.") } else { ("Goodbye.", "Welcome.") };
        let cold = translate(translator, options, direction, cold)?.elapsed;
        let warm = translate(translator, options, direction, warm)?.elapsed;
        println!("# {name} {:?}: first request {:.0} ms, the next one {:.0} ms: about {:.0} ms to load", options.quality, milliseconds(cold), milliseconds(warm), milliseconds(cold.saturating_sub(warm)));

        let mut single_sentences = Vec::new();
        for (index, source) in entries.iter().enumerate() {
            let run = translate(translator, options, direction, source)?;
            println!("[{name} {:02}] {:>7.1} ms  {source}", index + 1, milliseconds(run.elapsed));
            println!("{:>22}{}", "", run.text);
            if run.sentences == 1 {
                single_sentences.push(run.elapsed);
            }
        }
        single_sentences.sort();
        if let (Some(worst), Some(median)) = (single_sentences.last(), single_sentences.get(single_sentences.len() / 2)) {
            println!("# {name} {:?}: {} single sentences, median {:.1} ms, worst {:.1} ms", options.quality, single_sentences.len(), milliseconds(*median), milliseconds(*worst));
        }
    }
    Ok(())
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// The resident memory of this process, as `ps` reports it.
fn resident_memory() -> String {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid]).output();
    let kilobytes = output.ok().and_then(|output| String::from_utf8(output.stdout).ok()).and_then(|text| text.trim().parse::<f64>().ok());
    kilobytes.map_or("unknown".to_string(), |kilobytes| format!("{:.0} MB", kilobytes / 1024.0))
}
