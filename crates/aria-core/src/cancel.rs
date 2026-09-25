//! Stopping work that is no longer wanted.
//!
//! A scrub asks for a frame at every playhead it passes over. Frames take
//! longer than a scrub tick, so by the time one is composited the playhead has
//! moved on — and the next frame cannot start until it finishes, because a
//! renderer composites one frame at a time. Cancelling is what turns that queue
//! into a hand-off.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A one-way "stop" that long work can be asked to honour.
///
/// Cloning shares the signal: the caller keeps a handle to cancel with, the
/// work keeps one to check. Cancellation is one-way — a token that has been
/// cancelled stays cancelled, so a check cannot race a reset and miss it.
///
/// [`Cancel::never`] carries no signal and allocates nothing, so a path that is
/// never cancelled (an export, a still, a test) pays a null check and no atomic
/// at all.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Option<Arc<AtomicBool>>);

impl Cancel {
    /// A live token: what a caller holds so it can change its mind.
    pub fn new() -> Self {
        Self(Some(Arc::new(AtomicBool::new(false))))
    }

    /// A token nobody can cancel — the default, and what a path that runs to
    /// completion passes.
    pub fn never() -> Self {
        Self(None)
    }

    /// Ask the work holding a clone of this token to stop. Idempotent.
    pub fn cancel(&self) {
        if let Some(flag) = &self.0 {
            // Release pairs with the Acquire in `is_cancelled`: whatever the
            // canceller wrote before asking (a newer playhead, a closed panel)
            // is visible to the worker that observes the flag.
            flag.store(true, Ordering::Release);
        }
    }

    /// Whether the work should stop. Called between units of work, so it is a
    /// load with no lock and no allocation.
    pub fn is_cancelled(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    /// Whether anything could ever cancel this token. A worker can skip its
    /// per-unit checks entirely when nothing can.
    pub fn is_cancellable(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(test)]
#[path = "cancel_tests.rs"]
mod tests;
