//! What Traduko translated, kept on this Mac to be looked at later: one line
//! of JSON for each translation, in `history.jsonl` beside the settings.
//!
//! The panel translates while the user types, so a text goes through many
//! translations before it is whole. The panel hands over the ones that
//! stayed on screen; here, a text that goes on growing keeps one line.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::ops::Range;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use traduko_engine::Language;

const FILE: &str = "history.jsonl";

/// One translation, as the user saw it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// When it was finished, in seconds since 1970.
    pub at: u64,
    pub from: Language,
    pub to: Language,
    /// Traduko read `from` off the text; false when the user chose it.
    pub detected: bool,
    /// The English that was asked for. It only counts when `to` is English.
    pub british: bool,
    /// The model that was asked for, as in the settings.
    pub accurate: bool,
    pub source: String,
    pub translation: String,
    /// How long the translation took, in milliseconds.
    pub ms: u64,
    /// The user copied the translation.
    pub copied: bool,
}

impl Entry {
    /// True when this is `earlier` with more typed after it: the same
    /// languages and model, and a text that starts the same.
    fn carries_on(&self, earlier: &Entry) -> bool {
        (self.from, self.to, self.british, self.accurate) == (earlier.from, earlier.to, earlier.british, earlier.accurate)
            && self.source.starts_with(&earlier.source)
    }
}

/// Now, as an [`Entry`] counts time.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub struct History {
    file: PathBuf,
    /// The entry this run wrote last, and where its line is in the file.
    last: Option<(Range<u64>, Entry)>,
}

impl History {
    pub fn new(dir: &Path) -> Self {
        Self { file: dir.join(FILE), last: None }
    }

    /// Adds `entry` at the end of the file, or puts it in place of the last
    /// one when it carries on with the same text.
    pub fn record(&mut self, entry: Entry) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        // Everything the user typed: nobody else on this Mac reads it.
        let mut file = OpenOptions::new().create(true).append(true).mode(0o600).open(&self.file)?;
        let end = file.metadata()?.len();
        let start = match &self.last {
            // Still the last line of the file: nothing was written after it.
            Some((at, last)) if at.end == end && entry.carries_on(last) => at.start,
            _ => end,
        };
        file.set_len(start)?;
        file.write_all(&line)?;
        self.last = Some((start..start + line.len() as u64, entry));
        Ok(())
    }

    /// The text was cleared: the next one is another text, even one that
    /// starts the same.
    pub fn next_text(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn entry(source: &str, translation: &str) -> Entry {
        Entry {
            at: 1_791_370_000,
            from: Language::FRENCH,
            to: Language::ENGLISH,
            detected: true,
            british: false,
            accurate: true,
            source: source.to_string(),
            translation: translation.to_string(),
            ms: 212,
            copied: false,
        }
    }

    /// A folder of its own for one test, and the history in it.
    fn history(name: &str) -> (PathBuf, History) {
        let dir = std::env::temp_dir().join(format!("traduko-history-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let history = History::new(&dir);
        (dir, history)
    }

    fn lines(dir: &Path) -> Vec<Entry> {
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
        text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    #[test]
    fn a_translation_is_one_line_of_json() {
        let (dir, mut history) = history("line");
        history.record(entry("Bonjour", "Hello")).unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert_eq!(
            text,
            "{\"at\":1791370000,\"from\":\"fr\",\"to\":\"en\",\"detected\":true,\"british\":false,\"accurate\":true,\
             \"source\":\"Bonjour\",\"translation\":\"Hello\",\"ms\":212,\"copied\":false}\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_text_with_several_lines_stays_on_one() {
        let (dir, mut history) = history("newlines");
        history.record(entry("Bonjour.\nMerci.", "Hello.\nThank you.")).unwrap();
        history.record(entry("Au revoir", "Goodbye")).unwrap();
        assert_eq!(lines(&dir), [entry("Bonjour.\nMerci.", "Hello.\nThank you."), entry("Au revoir", "Goodbye")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_text_that_grows_keeps_one_line() {
        let (dir, mut history) = history("grows");
        history.record(entry("Au revoir", "Goodbye")).unwrap();
        history.record(entry("Bonjour", "Hello")).unwrap();
        history.record(entry("Bonjour tout", "Hello all")).unwrap();
        history.record(entry("Bonjour tout le monde", "Hello everyone")).unwrap();
        assert_eq!(lines(&dir), [entry("Au revoir", "Goodbye"), entry("Bonjour tout le monde", "Hello everyone")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_text_that_was_cut_short_gets_its_own_line() {
        let (dir, mut history) = history("shorter");
        history.record(entry("Bonjour tout le monde", "Hello everyone")).unwrap();
        history.record(entry("Bonjour", "Hello")).unwrap();
        assert_eq!(lines(&dir).len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_same_text_to_another_language_gets_its_own_line() {
        let (dir, mut history) = history("language");
        history.record(entry("Bonjour", "Hello")).unwrap();
        history.record(Entry { to: Language::from_code("de").unwrap(), ..entry("Bonjour", "Hallo") }).unwrap();
        history.record(Entry { accurate: false, ..entry("Bonjour", "Hello") }).unwrap();
        assert_eq!(lines(&dir).len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_copy_marks_the_line_of_its_translation() {
        let (dir, mut history) = history("copied");
        history.record(entry("Bonjour", "Hello")).unwrap();
        history.record(Entry { copied: true, ..entry("Bonjour", "Hello") }).unwrap();
        assert_eq!(lines(&dir), [Entry { copied: true, ..entry("Bonjour", "Hello") }]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_text_typed_again_after_a_clear_is_counted_again() {
        let (dir, mut history) = history("cleared");
        history.record(entry("Bonjour", "Hello")).unwrap();
        history.next_text();
        history.record(entry("Bonjour", "Hello")).unwrap();
        assert_eq!(lines(&dir).len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_lines_of_earlier_runs_stay() {
        let (dir, mut history) = history("runs");
        history.record(entry("Bonjour", "Hello")).unwrap();
        // The next run knows nothing of the last line: it only adds.
        let mut next_run = History::new(&dir);
        next_run.record(entry("Bonjour tout le monde", "Hello everyone")).unwrap();
        assert_eq!(lines(&dir), [entry("Bonjour", "Hello"), entry("Bonjour tout le monde", "Hello everyone")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_line_added_by_someone_else_is_not_written_over() {
        let (dir, mut history) = history("someone-else");
        history.record(entry("Bonjour", "Hello")).unwrap();
        let mut file = OpenOptions::new().append(true).open(dir.join(FILE)).unwrap();
        file.write_all(b"{\"note\":\"mine\"}\n").unwrap();
        history.record(entry("Bonjour tout le monde", "Hello everyone")).unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert_eq!(text.lines().nth(1), Some("{\"note\":\"mine\"}"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_is_for_its_owner_only() {
        let (dir, mut history) = history("owner");
        history.record(entry("Bonjour", "Hello")).unwrap();
        let mode = std::fs::metadata(dir.join(FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
