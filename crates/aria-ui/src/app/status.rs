//! Ageing the status line.
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
