//! The status line: how long it stays up, and how a line is written.
//!
//! A status is a receipt for something that just happened ("Copied", "Export
//! started"), so it stops being true a few seconds later and must not sit on the
//! strip for the rest of the session. An error is not a receipt: it stays until
//! an action succeeds, which is the only thing that makes it stale.
//!
//! Ageing is done by asking, not by every writer remembering to stamp a time:
//! the strip has forty-odd writers across the crate, and one that forgot the
//! stamp would be a message that never clears. Instead this module notices the
//! text changed, starts the clock then, and clears it when the clock runs out —
//! so a new writer needs no ceremony at all.
//!
//! A line on the strip starts with a capital and ends with a period only when
//! it holds more than one sentence. An error from below the editor is written
//! as Rust errors are, in lower case so it reads inside another sentence, and
//! `as_sentence` starts it with a capital when it is posted on its own.

use std::time::{Duration, Instant};

use super::AppState;

/// How long a status stays on the strip after it is posted.
pub const STATUS_TTL: Duration = Duration::from_secs(4);

impl AppState {
    /// Expire the status line if it has been up for [`STATUS_TTL`]. Returns
    /// whether anything changed, so the caller repaints only when it did.
    ///
    /// `now` is a parameter rather than read here so the rule is testable
    /// without sleeping.
    pub fn age_status(&mut self, now: Instant) -> bool {
        let Some(message) = self.status_message.as_deref() else {
            self.status_seen = None;
            return false;
        };
        match &self.status_seen {
            // Same text still up: clear it once its time is spent.
            Some((seen, at)) if seen == message => {
                if now.duration_since(*at) < STATUS_TTL {
                    return false;
                }
                self.status_message = None;
                self.status_seen = None;
                true
            }
            // New text (or the first one): the clock starts now.
            _ => {
                self.status_seen = Some((message.to_owned(), now));
                false
            }
        }
    }

    /// Forget the error, if any. Called when an action succeeds: that is what
    /// makes a previous failure stale.
    pub(crate) fn clear_error(&mut self) {
        self.last_error = None;
    }
}

/// Symbols written in lower case: the effects badge and the expression driver
/// mark. Each is drawn as it is, as Premiere draws its `fx` badge.
pub(crate) const LOWER_CASE_MARKS: [&str; 2] = ["fx", "ƒ"];

/// The first word of `line`, when it is written in lower case letters alone.
///
/// A word with a capital, a digit or a mark in it is a unit, a version, an id
/// or a path (`dB`, `v0.1.0`, `com.example.fx`, `/data`), and is written as it
/// is, as is a symbol of [`LOWER_CASE_MARKS`].
pub(crate) fn lowercase_start(line: &str) -> Option<&str> {
    let word = line
        .split_whitespace()
        .next()?
        .trim_end_matches([',', ';', ':', '.']);
    let lower = !word.is_empty() && word.chars().all(char::is_lowercase);
    (lower && !LOWER_CASE_MARKS.contains(&word)).then_some(word)
}

/// `line` started with a capital, when its first word is written in lower
/// case letters alone.
pub(crate) fn as_sentence(mut line: String) -> String {
    let Some(first) = lowercase_start(&line).and_then(|word| word.chars().next()) else {
        return line;
    };
    // The first word starts at the first character that is not whitespace.
    let at = line.len() - line.trim_start().len();
    line.replace_range(at..at + first.len_utf8(), &first.to_uppercase().to_string());
    line
}

#[cfg(test)]
#[path = "status_lines_tests.rs"]
mod lines_tests;
