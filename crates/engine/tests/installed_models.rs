//! Tests with the real models, through the public API.
//!
//! They need the models that `scripts/fetch-models.sh` installs (in
//! `$COCO_MODELS_DIR` or `~/Library/Application Support/Coco/models`). When a
//! set is missing, its tests say so and pass: the unit tests cover the rest
//! without any model.
//!
//! The matrix kernels are fifty times slower when the dependencies are not
//! optimised, which is the default of `cargo test`. The tests still pass
//! then, in about a minute; with `--release` they take a few seconds.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use coco_engine::{Direction, EnglishVariant, Installed, Language, Quality, Request, Translator, Update, default_models_dir};

const FR_EN: Direction = Direction::new(Language::FRENCH, Language::ENGLISH);
const EN_FR: Direction = FR_EN.swapped();

/// The tests take turns: each one loads its own models, and all of them at
/// once would hold several gigabytes.
static TURN: Mutex<()> = Mutex::new(());

/// A fresh translator, and the turn of the test that holds it.
struct Session {
    translator: Translator,
    _turn: MutexGuard<'static, ()>,
}

/// A session for `quality`, or `None` with a note when that set is not
/// installed.
fn session(quality: Quality) -> Option<Session> {
    session_when(|installed| installed.sets().contains(&quality), &format!("the {quality:?} models are"))
}

/// A session when the models on disk are `enough`, or `None` with a note
/// that says `what` is missing.
fn session_when(enough: impl Fn(&Installed) -> bool, what: &str) -> Option<Session> {
    let models = default_models_dir();
    if enough(&Translator::installed(&models)) {
        let turn = TURN.lock().unwrap_or_else(PoisonError::into_inner);
        return Some(Session { translator: Translator::start(models), _turn: turn });
    }
    eprintln!("SKIPPED: {what} not in {}. Run scripts/fetch-models.sh.", models.display());
    None
}

fn direction(code: &str) -> Direction {
    Direction::from_code(code).expect("a direction")
}

fn request(text: &str, direction: Direction, quality: Quality) -> Request {
    Request { text: text.to_string(), direction, english: EnglishVariant::American, quality }
}

/// The updates of a request until its stream ends.
fn updates(translator: &Translator, request: Request) -> Vec<Update> {
    futures_executor::block_on_stream(translator.translate(request)).collect()
}

/// The final text of a request, which must succeed.
fn translate(translator: &Translator, request: Request) -> String {
    match updates(translator, request).pop() {
        Some(Update::Done { text, .. }) => text,
        other => panic!("the request did not finish: {other:?}"),
    }
}

fn greets_in_both_directions(quality: Quality) {
    let Some(session) = session(quality) else { return };
    let translator = &session.translator;
    let english = translate(translator, request("Bonjour, comment allez-vous aujourd'hui ?", FR_EN, quality));
    assert_eq!(english, "Hello, how are you today?");
    let french = translate(translator, request("Hello, how are you today?", EN_FR, quality));
    assert_eq!(french, "Bonjour, comment allez-vous aujourd'hui ?");
}

#[test]
fn the_light_models_translate_in_both_directions() {
    greets_in_both_directions(Quality::Light);
}

#[test]
fn the_accurate_models_translate_in_both_directions() {
    greets_in_both_directions(Quality::Accurate);
}

#[test]
fn the_stream_announces_the_load_then_grows_sentence_by_sentence() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    let text = "J'ai emménagé à Lyon il y a trois ans. Au début, je ne connaissais personne. Aujourd'hui, je ne me vois plus vivre ailleurs.";
    let updates = updates(translator, request(text, FR_EN, Quality::Light));

    assert!(matches!(updates.first(), Some(Update::LoadingModel)), "{updates:?}");
    let partials: Vec<(&str, usize, usize)> = updates
        .iter()
        .filter_map(|update| match update {
            Update::Partial { text, done, total } => Some((text.as_str(), *done, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(partials.iter().map(|(_, done, total)| (*done, *total)).collect::<Vec<_>>(), [(1, 3), (2, 3), (3, 3)]);
    assert!(partials[0].0.starts_with("I moved to Lyon"), "{partials:?}");
    assert!(partials.windows(2).all(|pair| pair[1].0.starts_with(pair[0].0)), "each update extends the one before");
    match updates.last() {
        Some(Update::Done { text, sentences, .. }) => {
            assert_eq!((text.as_str(), *sentences), (partials[2].0, 3));
        }
        other => panic!("the request did not finish: {other:?}"),
    }
    assert_eq!(updates.len(), 5, "{updates:?}");
}

#[test]
fn english_comes_out_american_or_british_as_asked() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    let text = "Ma couleur préférée est le gris et j'habite au centre-ville, à côté du théâtre.";
    let ask = |english| translate(translator, Request { english, ..request(text, FR_EN, Quality::Light) });
    let american = ask(EnglishVariant::American);
    let british = ask(EnglishVariant::British);
    assert!(american.contains("color") && american.contains("center") && american.contains("theater"), "{american}");
    assert!(british.contains("colour") && british.contains("centre") && british.contains("theatre"), "{british}");
}

#[test]
fn the_shape_of_the_text_survives_translation() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    let text = "  Liste de courses :\n\n- Acheter du pain\n- Appeler Marie\n\n\t1. Dormir tôt.  Se lever tôt.\n";
    let english = translate(translator, request(text, FR_EN, Quality::Light));
    let shape = |text: &str| -> Vec<String> {
        // What must not change: the blanks and the marker in front of each line.
        let lead = |line: &str| line.chars().take_while(|c| !c.is_alphabetic()).collect::<String>();
        text.split('\n').map(lead).collect()
    };
    assert_eq!(shape(&english), shape(text), "{english:?}");
    assert!(english.contains("bread") && english.ends_with(".\n"), "{english:?}");
    assert!(english.contains(".  "), "the two blanks between the sentences are kept: {english:?}");
}

#[test]
fn odd_input_is_translated_or_copied_but_never_fails() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    let texts = [
        "😀",
        "J'adore ce film 😀 ! On se voit demain 🎉🎉 ?",
        "Écris-moi à jean.dupont@exemple.fr ou va sur https://exemple.fr/contact.",
        "https://exemple.fr/une/page?avec=des&parametres=1",
        "Texte 中文 и русский, mélangés.",
        "</s> <pad> <unk> ne sont que du texte.",
        "\u{0}\u{1}\u{feff}\u{200b} caractères de contrôle \t\r\n",
        "e\u{301}te\u{301} de\u{301}compose\u{301}",
        &"Une phrase assez courte. ".repeat(800),
        &"x".repeat(20_000),
    ];
    for text in texts {
        let updates = updates(translator, request(text, FR_EN, Quality::Light));
        let preview: String = text.chars().take(40).collect();
        assert!(matches!(updates.last(), Some(Update::Done { .. })), "{preview:?} gave {:?}", updates.last());
    }

    // What has no words is given back as it is; emoji follow their sentence.
    let same = |text: &str| translate(translator, request(text, FR_EN, Quality::Light)) == text;
    assert!(same("😀") && same("https://exemple.fr/une/page?avec=des&parametres=1") && same(&"x".repeat(20_000)));
    let with_emoji = translate(translator, request("J'adore ce film 😀 !", FR_EN, Quality::Light));
    assert!(with_emoji.starts_with("I love this") && with_emoji.ends_with("😀"), "{with_emoji}");
}

#[test]
fn a_sentence_too_long_for_the_model_is_translated_in_pieces() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    // About 330 tokens, where the model is given 256 at most.
    let long_sentence = "le chat regarde la pluie tomber sur le jardin et ".repeat(30);

    // The first request loads the model; the second one shows the speed.
    translate(translator, request("Le chat mange.", FR_EN, Quality::Light));
    let started = Instant::now();
    translate(translator, request("Le chat dort.", FR_EN, Quality::Light));
    let three_words = started.elapsed();
    if three_words > Duration::from_millis(200) {
        eprintln!("SKIPPED: this build takes {three_words:?} for three words, so 300 would take a minute. Use --release.");
        return;
    }
    let english = translate(translator, request(&long_sentence, FR_EN, Quality::Light));
    let cats = english.matches("cat").count();
    assert!(cats >= 25, "{cats} cats of 30 in {english:?}");
}

#[test]
fn a_newer_request_stops_the_one_in_progress() {
    let Some(session) = session(Quality::Light) else { return };
    let translator = &session.translator;
    let many_sentences: String = (1..=60).map(|n| format!("Voici la phrase numéro {n} de ce long texte. ")).collect();
    let first = translator.translate(request(&many_sentences, FR_EN, Quality::Light));
    // Let it start, then ask for something else.
    std::thread::sleep(Duration::from_millis(300));
    let second = translate(translator, request("Merci beaucoup.", FR_EN, Quality::Light));
    assert_eq!(second, "Thank you very much.");

    let first: Vec<Update> = futures_executor::block_on_stream(first).collect();
    assert!(!first.iter().any(|update| matches!(update, Update::Done { .. })), "the first request was replaced");
    assert!(first.len() < 60, "it stopped early, after {} updates", first.len());
}

#[test]
fn another_language_is_translated_to_english_and_back() {
    let languages = ["es-en", "en-es", "de-en", "en-de"].map(direction);
    let Some(session) = session_when(|installed| languages.iter().all(|way| installed.translates(*way)), "Spanish and German (--lang es,de) are") else {
        return;
    };
    let translator = &session.translator;
    let light = |text: &str, way: &str| translate(translator, request(text, direction(way), Quality::Light));
    assert_eq!(light("¿Dónde está la estación de tren?", "es-en"), "Where's the train station?");
    assert_eq!(light("Where is the train station?", "en-es"), "¿Dónde está la estación de tren?");
    assert_eq!(light("Wo ist der Bahnhof?", "de-en"), "Where's the station?");
    assert_eq!(light("Where is the train station?", "en-de"), "Wo ist der Bahnhof?");

    // The large models have no Spanish: asked for them, the small one answers.
    let asked_of_the_large = translate(translator, request("Muchas gracias por tu ayuda.", direction("es-en"), Quality::Accurate));
    assert_eq!(asked_of_the_large, light("Muchas gracias por tu ayuda.", "es-en"));
}

#[test]
fn two_languages_without_a_model_of_their_own_are_translated_through_english() {
    let needed = ["fr-en", "en-de", "es-en", "en-fr"].map(direction);
    let Some(session) = session_when(|installed| needed.iter().all(|way| installed.translates(*way)), "French, Spanish and German are") else {
        return;
    };
    let translator = &session.translator;
    let text = "Le train part à huit heures. Nous arriverons avant midi.";
    let updates = updates(translator, request(text, direction("fr-de"), Quality::Light));
    let Some(Update::Done { text: german, sentences, .. }) = updates.last() else { panic!("the request did not finish: {updates:?}") };
    assert_eq!(*sentences, 2);
    assert!(german.contains("Zug") && german.contains("acht Uhr") && german.contains("Mittag"), "{german}");

    let french = translate(translator, request("Muchas gracias por tu ayuda.", direction("es-fr"), Quality::Light));
    assert!(french.contains("Merci") && french.contains("aide"), "{french}");
}
