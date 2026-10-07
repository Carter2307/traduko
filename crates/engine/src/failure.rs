//! What went wrong, in words that can be shown to the user.

use std::fmt;

/// A model that could not be loaded or run. The text is short and plain
/// because it ends up in [`crate::Update::Failed`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure(String);

pub(crate) type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// A file of the model folder that is there but cannot be used.
    pub fn damaged(file: &str, why: impl fmt::Display) -> Self {
        Self(format!("The model file {file} is damaged: {why}."))
    }

    pub fn into_message(self) -> String {
        self.0
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// candle reports shapes and backtraces over several lines; the first line
/// says what happened, and that is all a person can use.
impl From<candle_core::Error> for Failure {
    fn from(error: candle_core::Error) -> Self {
        let text = error.to_string();
        let first_line = text.lines().next().unwrap_or("unknown error");
        let short: String = first_line.chars().take(120).collect();
        Self(format!("The model stopped with an error: {short}."))
    }
}
