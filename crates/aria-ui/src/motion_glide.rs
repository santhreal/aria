//! Eased geometry for the values the chrome would otherwise step through.
//!
//! [`gpui::Animation`] runs one fixed transition from the frame an element first
//! appears, which is the wrong shape for a number that keeps moving: the
//! playhead, a lane's height, a pane's width, a clip's position after a ripple.
//! Those chase a target instead. This is the store [`crate::motion`] already
//! keeps for hover fades, over `f32` rather than colour, and every transition
//! here runs on a catalog [`MotionSpec`] — no duration or curve is invented in
//! this file.
//!
//! One store, and the shell drives it. The driver is an element, so it belongs
//! to the window that mounts it rather than to this store; a second one would
//! advance the frame counter twice and evict the values the first had just
//! drawn, which is why `crate::shell` mounts exactly one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::motion::{self, MotionSpec};

/// The playhead follows within one exit beat on the catalog's decelerating
/// curve: short enough that a scrub stays under the pointer, long enough that a
/// frame step reads as movement rather than a jump.
pub(crate) const PLAYHEAD: MotionSpec = MotionSpec::new(motion::FLICK, motion::EASE_OUT);

/// Lane height: a collapse, an expand, or a resize that was released.
pub(crate) const LANE: MotionSpec = motion::COLLAPSE;

/// A clip's own position and length, so a ripple, a roll or a move slides the
/// cards it displaced instead of teleporting them.
pub(crate) const EDIT: MotionSpec = motion::RESIZE;

/// A pane changing size: a splitter that was released, or a rail collapsing.
/// A collapse is a resize to zero, so it is the same motion rather than a
/// second one that happens to look like it.
pub(crate) const PANE: MotionSpec = motion::RESIZE;

/// The horizontal offset, when the view moved on the editor's initiative: a fit,
/// or playback that ran past the edge. Slower than the playhead's own beat
/// because the whole picture travels, and a jump that size reads as a cut.
pub(crate) const SCROLL: MotionSpec = motion::SCROLL_GLIDE;

/// The horizontal offset after a pan was released. The same length as [`SCROLL`]
/// on the one curve that overshoots: the offset carries the momentum of the
/// release, passes where it was going by under a tenth of the throw, and comes
/// back. Nothing else in the app overshoots, which is what makes a flick read as
/// weight rather than as a bug.
pub(crate) const FLING: MotionSpec = motion::FLING;

/// The horizontal scale, when the editor chose it: a fit, a keyed step, a menu
/// row. Shorter than [`SCROLL`] because the drawn scale is not the committed one
/// while this runs, and a click during that window lands on the tick that is
/// under the pointer on screen rather than the one the model would compute.
pub(crate) const ZOOM: MotionSpec = motion::RESIZE;

/// A binary state a card can leave again: selected, locked. The same beat as a
/// hover, because the ring and the fill under it are two parts of one response
/// to the same click and would read as two events at two rates.
pub(crate) const STATE: MotionSpec = motion::HOVER_FADE;

/// A whole route arriving: Home, or the editor. The dialog beat, because a route
/// change is the largest thing the window does that is not a boot, and anything
/// slower keeps the operator waiting for a screen they have already asked for.
pub(crate) const ROUTE: MotionSpec = motion::DIALOG_IN;

/// A progress fill catching up with a report. The geometry beat, because that is
/// what a bar is: a width that changed. Short, because a report that arrived
/// while the last one was still travelling must not queue behind it.
pub(crate) const PROGRESS: MotionSpec = motion::RESIZE;

/// How long a wait must last before a loader is worth showing, spent by
/// [`since`] rather than as a transition: only the span is read, never the
/// curve. One settle, because a wait an operator did not notice must not be
/// announced, and one they did must not be silent.
pub(crate) const WAIT: MotionSpec = motion::OPEN;

#[derive(Clone, Copy)]
struct Eased {
    origin: f32,
    target: f32,
    started: Instant,
    spec: MotionSpec,
    seen: u64,
}

impl Eased {
    fn duration(&self) -> std::time::Duration {
        self.spec.total().mul_f32(motion::speed_scale())
    }

    fn value(&self, now: Instant) -> f32 {
        let duration = self.duration();
        let elapsed = now.saturating_duration_since(self.started);
        if duration.is_zero() || elapsed >= duration {
            return self.target;
        }
        let raw = elapsed.as_secs_f32() / duration.as_secs_f32();
        motion::lerp(self.origin, self.target, self.spec.progress(raw))
    }

    fn settled(&self, now: Instant) -> bool {
        self.origin == self.target || now.saturating_duration_since(self.started) >= self.duration()
    }
}

#[derive(Default)]
struct Eases {
    entries: HashMap<motion::MotionKey, Eased>,
    frame: u64,
}

thread_local! {
    static EASES: RefCell<Eases> = RefCell::new(Eases::default());
}

/// The value `key` shows this frame while it travels to `target`.
///
/// The first sighting of a key is not a change: a clip that scrolls into view
/// draws where it is rather than flying in from nowhere.
///
/// `instant` is the caller's override for a value that must not lag the hand
/// moving it — a splitter under the pointer, a scroll the operator is driving.
/// Reduced motion is not the caller's business: it is read here, once, so a new
/// call site cannot forget it and leave one pane sliding for an operator who
/// turned motion off.
pub(crate) fn glide(key: motion::MotionKey, target: f32, spec: MotionSpec, instant: bool) -> f32 {
    glide_at(key, target, spec, instant, Instant::now())
}

/// [`glide`] against a caller's clock.
///
/// Every read of this store is a function of `now`, so `now` is a parameter and
/// the wall clock is read in exactly one place per frame. Two calls a
/// microsecond apart are two different frames to `Instant::now()`, which is how
/// a retarget test ends up asserting that a rising value did not rise: with the
/// instant passed in, a frame is a frame.
pub(crate) fn glide_at(
    key: motion::MotionKey,
    target: f32,
    spec: MotionSpec,
    instant: bool,
    now: Instant,
) -> f32 {
    let instant = instant || motion::reduced_motion_now();
    EASES.with(|store| {
        let mut store = store.borrow_mut();
        let frame = store.frame;
        if let Some(entry) = store.entries.get_mut(&key) {
            entry.seen = frame;
            if instant {
                entry.origin = target;
                entry.target = target;
                return target;
            }
            if entry.target != target {
                let current = entry.value(now);
                entry.origin = current;
                entry.target = target;
                entry.started = now;
                entry.spec = spec;
            }
            return entry.value(now);
        }
        store.entries.insert(
            key,
            Eased {
                origin: target,
                target,
                started: now,
                spec,
                seen: frame,
            },
        );
        target
    })
}

/// How long `key` has been asked for, as a fraction of `spec`, starting at 0 on
/// the first sighting and stopping at 1.
///
/// The inverse of [`glide`]'s first-sighting rule, and for the same reason: a
/// value that arrives already at its target is right for geometry and wrong for
/// an age. This is how a surface waits before it shows a loader — the store
/// already holds a start time per key, evicts a key nothing draws, and drives
/// the frames while the value is still moving, and none of that is worth a
/// second copy of.
pub(crate) fn since(key: motion::MotionKey, spec: MotionSpec) -> f32 {
    since_at(key, spec, Instant::now())
}

/// [`since`] against a caller's clock, for the same reason as [`glide_at`].
pub(crate) fn since_at(key: motion::MotionKey, spec: MotionSpec, now: Instant) -> f32 {
    EASES.with(|store| {
        let mut store = store.borrow_mut();
        let frame = store.frame;
        if let Some(entry) = store.entries.get_mut(&key) {
            entry.seen = frame;
            return entry.value(now);
        }
        store.entries.insert(
            key,
            Eased {
                origin: 0.0,
                target: 1.0,
                started: now,
                spec,
                seen: frame,
            },
        );
        0.0
    })
}

/// Close the frame: drop the keys nothing drew, and report whether anything is
/// still moving.
pub(crate) fn tick() -> bool {
    tick_at(Instant::now())
}

/// [`tick`] against a caller's clock, for the same reason as [`glide_at`].
pub(crate) fn tick_at(now: Instant) -> bool {
    EASES.with(|store| {
        let mut store = store.borrow_mut();
        store.frame += 1;
        let frame = store.frame;
        let mut active = false;
        store.entries.retain(|_, entry| {
            if entry.seen + 1 < frame {
                return false;
            }
            if !entry.settled(now) {
                active = true;
            }
            true
        });
        active
    })
}

#[cfg(test)]
#[path = "motion_glide_tests.rs"]
mod tests;
