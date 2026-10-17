//! Eased geometry for the values the chrome would otherwise step through.
//!
//! [`gpui::Animation`] runs one fixed transition from the frame an element first
//! appears, which is the wrong shape for a number that keeps moving: the
//! playhead, a lane's height, a clip's position after a ripple.
//! Those chase a target instead, in the shared glide store, a
//! [`gpui::motion::TweenStore`] sampled at the open motion frame. Every
//! transition here runs on a catalog [`MotionSpec`].
//!
//! [`super::end_frame`] advances the store once per draw: it drops the keys no
//! element read and requests the next frame while a value is still moving.

use gpui::motion::{Easing, MotionSpec, presets};

use super::{MotionKey, with_frame};

/// The playhead follows within one exit beat, [`presets::MENU_OUT`], on the
/// decelerating curve: short enough that a scrub stays under the pointer, long
/// enough that a frame step reads as movement rather than a jump.
pub(crate) const PLAYHEAD: MotionSpec =
    MotionSpec::new(presets::MENU_OUT.duration_ms, Easing::EaseOut);

/// Lane height: a collapse, an expand, or a resize that was released.
pub(crate) const LANE: MotionSpec = presets::COLLAPSE;

/// A clip's own position and length, so a ripple, a roll or a move slides the
/// cards it displaced instead of teleporting them.
pub(crate) const EDIT: MotionSpec = presets::RESIZE;

/// A list the editor scrolled to show a new row: the assistant transcript
/// following its newest reply. Slow, because the rows travel through the line
/// the reader is on, and a jump that size reads as a cut.
pub(crate) const SCROLL: MotionSpec = presets::SCROLL_GLIDE;

/// The horizontal offset after a pan was released. The same length as [`SCROLL`]
/// on the one curve that overshoots: the offset carries the momentum of the
/// release, passes where it was going by under a tenth of the throw, and comes
/// back. Nothing else in the app overshoots, so a flick reads as weight rather
/// than as a bug.
pub(crate) const FLING: MotionSpec = super::FLING;

/// The horizontal scale and scroll offset, when the editor chose them: a fit, a
/// keyed step, a menu row, playback that ran past the edge. Both ride this one
/// spec, so a tick held in place across the change (the playhead under `=`)
/// stays put while they travel; two specs drift it. Short, because the drawn
/// scale is not the committed one while this runs, and a click during that
/// window lands on the tick that is under the pointer on screen rather than the
/// one the model would compute.
pub(crate) const ZOOM: MotionSpec = presets::RESIZE;

/// A binary state a card can leave again: selected, locked. The same beat as a
/// hover, because the ring and the fill under it are two parts of one response
/// to the same click and would read as two events at two rates.
pub(crate) const STATE: MotionSpec = presets::HOVER_FADE;

/// A whole route arriving: Home, or the editor. The dialog beat, because a route
/// change is the largest thing the window does that is not a boot, and anything
/// slower keeps the operator waiting for a screen they have already asked for.
pub(crate) const ROUTE: MotionSpec = presets::DIALOG_IN;

/// A progress fill catching up with a report. The geometry beat, because that is
/// what a bar is: a width that changed. Short, because a report that arrived
/// while the last one was still travelling must not queue behind it.
pub(crate) const PROGRESS: MotionSpec = presets::RESIZE;

/// How long a wait must last before a loader is shown, spent by [`since`]
/// rather than as a transition: only the span is read, never the curve. One
/// [`super::OPEN`] stage, because a wait an operator did not notice must not be
/// announced, and one they did must not be silent.
pub(crate) const WAIT: MotionSpec = super::OPEN;

/// The value `key` shows in the open frame while it travels to `target`.
///
/// The first read of a key is not a change: a clip that scrolls into view
/// draws where it is rather than flying in from nowhere.
///
/// `instant` is the caller's override for a value that must not lag the hand
/// moving it: a splitter under the pointer, a scroll the operator is driving.
/// Reduced motion is read from the frame's policy, so a new call site cannot
/// forget it and leave one pane sliding for an operator who turned motion off.
/// Playback holds the value at its target the same way
/// ([`super::hold_for_playback`]).
pub(crate) fn glide(key: MotionKey, target: f32, spec: MotionSpec, instant: bool) -> f32 {
    with_frame(|chrome, frame| {
        let policy = chrome.policy(frame.policy());
        chrome
            .glide
            .glide(key, target, spec, instant, policy, frame.now())
    })
}

/// How long `key` has been read, as a fraction of `spec` from 0 at the first
/// read to 1, in the open frame.
///
/// The inverse of [`glide`]'s first-read rule: a value that arrives already at
/// its target is right for geometry and wrong for an age. A surface waits on
/// this before it shows a loader. Reduced motion does not shorten a wait; the
/// duration scale applies.
pub(crate) fn since(key: MotionKey, spec: MotionSpec) -> f32 {
    with_frame(|chrome, frame| chrome.glide.since(key, spec, frame.policy(), frame.now()))
}

#[cfg(test)]
#[path = "motion_glide_tests.rs"]
mod tests;
