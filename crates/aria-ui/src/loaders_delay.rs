//! When a loader is allowed to appear.
//!
//! A wait that resolves in forty milliseconds and a wait that never resolves
//! both used to show a spinner on the first frame, so the fast one flashed: the
//! operator saw something start and stop with no time to read it, which reads as
//! a glitch rather than as work. Every loader now asks first.
//!
//! The age comes from the eased-geometry store's [`glide::since`], which already
//! holds a start time per key, forgets a key nothing draws, and drives the frames
//! while the value is still climbing. A wait that ends before the span is up
//! therefore leaves no trace at all — including no entry to evict.

use super::*;

use crate::motion::glide;

/// Whether a wait named `id` has lasted long enough to deserve a loader.
///
/// Must be called on every frame the wait is live: the age is kept only while it
/// is asked for, so a caller that asks intermittently restarts the span. That is
/// the same contract every other keyed value in the app has.
pub fn worth_showing(id: motion::ControlId) -> bool {
    glide::since(id.motion("wait"), glide::WAIT) >= 1.0
}

#[cfg(test)]
#[path = "loaders_delay_tests.rs"]
mod tests;
