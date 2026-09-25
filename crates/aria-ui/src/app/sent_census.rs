//! What a driven control sent, as opposed to what it changed.
//!
//! A reach census that reads history entries can only see an edit. Saving,
//! opening, exporting, applying a template and renaming a project are commands
//! an operator asks for that write no history, so a surface census built on
//! history alone reports every one of them as unreached.
//!
//! `AppState::apply_command` emits `CommandProposed` before the engine sees the
//! command, so one subscription observes every command any surface dispatches,
//! including the ones that are refused. That is the right signal for "does a
//! control exist that asks for this": the ask is the command, not its outcome.

use std::sync::{Arc, Mutex};

use aria_plugin::{CommandProposed, Registration};

use super::AppState;

/// Every command dispatched while this is alive.
///
/// Holds its own registration, so the subscription ends with the watch rather
/// than living as long as the state's context.
pub(super) struct Watch {
    seen: Arc<Mutex<Vec<&'static str>>>,
    _reg: Registration,
}

impl Watch {
    /// Start watching one state.
    pub(super) fn on(state: &AppState) -> Self {
        let seen: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let reg = state
            .ctx
            .on_waterfall::<CommandProposed, _>(move |proposed| {
                sink.lock()
                    .expect("the recorder outlives every dispatch")
                    .push(proposed.command.label());
            });
        Self { seen, _reg: reg }
    }

    /// Take what has been sent so far, emptying the record.
    pub(super) fn take(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.seen.lock().expect("the recorder is not poisoned"))
    }
}
