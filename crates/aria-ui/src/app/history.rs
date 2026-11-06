//! Moving the session to a step of its own history.
//!
//! The engine already owns undo and redo; a history panel adds only two things
//! on top — the names of the steps, and jumping to a step by *position* rather
//! than by direction. The names and the depth are projected beside the store
//! (see [`project_facts`](crate::app::project_facts)) so the panel does not ask
//! the session once per repaint; the jump is here, because it is an edit.

use super::AppState;

impl AppState {
    /// Move the project to the state after `depth` steps.
    ///
    /// One republish for the whole walk rather than one per step: the request
    /// is for a state, not for N undos. The filmstrips follow the walk step by
    /// step and drop only the cells inside the edits it changed back.
    pub fn history_jump(&mut self, depth: usize) -> bool {
        if depth == self.history_depth() {
            return false;
        }
        let walked = match self.host.history_jump(depth) {
            Ok(walked) => walked,
            Err(err) => {
                self.report_error(&err);
                return false;
            }
        };
        for &reverted in walked.iter().flatten() {
            self.filmstrips.revert(reverted);
        }
        self.sync_from_engine();
        self.republish_audio();
        if walked.is_none() {
            self.status_message = Some("History could not go that far".into());
        }
        walked.is_some()
    }
}
