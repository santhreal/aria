//! Telling a seek from a scrub, so the frame can cross-fade on one and not the
//! other.
//!
//! A new frame deserves a fade when it replaces one the operator had been
//! looking at: click a tick on the ruler, press a transport key, jump to a
//! marker. It must not get one while the playhead is being dragged, because a
//! fade that restarts every frame never finishes, and a frame that never
//! reaches full opacity during a scrub is worse than one that never fades at
//! all.
//!
//! The two cases differ only in their spacing, and that is all this reads: if
//! the previous change arrived less than one fade ago, the changes are a run
//! and the frame is left alone. Nothing in the app has to record that a drag
//! is happening, which is the point — the ruler, the transport, the keyboard
//! and the pool monitor all seek through different paths and none of them
//! would agree on a flag.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::motion;
use crate::motion::ControlId;

/// What the last change to one keyed value looked like.
struct Run {
    /// The value as of the last render that saw it change.
    value: i64,
    /// When that was.
    at: Instant,
    /// Whether that change was part of a run: another change had landed less
    /// than a fade earlier.
    in_run: bool,
}

thread_local! {
    /// One entry per frame surface. There are two — the sequence stage and the
    /// pool monitor — so the map is bounded by the surfaces that exist rather
    /// than by anything the project holds, and never needs retiring.
    static RUNS: RefCell<HashMap<&'static str, Run>> = RefCell::new(HashMap::new());
}

/// The identity to fade the frame under.
///
/// Distinct per discrete seek, so the fade restarts and the new frame arrives
/// rather than appearing; constant for the whole of a scrub and for playback,
/// so those run at full opacity.
pub(super) fn frame_id(key: &'static str, value: i64) -> ControlId {
    let now = Instant::now();
    let in_run = RUNS.with(|runs| {
        let mut runs = runs.borrow_mut();
        match runs.get_mut(key) {
            Some(run) => {
                if run.value != value {
                    run.in_run = now.duration_since(run.at) < motion::FADE_QUICK.total();
                    run.value = value;
                    run.at = now;
                }
                run.in_run
            }
            None => {
                // The first frame of the session is an arrival, not a run.
                runs.insert(
                    key,
                    Run {
                        value,
                        at: now,
                        in_run: false,
                    },
                );
                false
            }
        }
    });
    if in_run {
        ControlId::Fixed(key)
    } else {
        // A tick that repeats after a hash collision would only share a fade
        // clock with the position it collided with, which is what a run already
        // does on purpose.
        ControlId::Key(motion::MotionKey::new(key, value))
    }
}
