//! The progress bar, determinate and not.
//!
//! A job that reports a fraction and a job that cannot are the same wait to the
//! operator, so they get one element with two fills. The determinate fill is a
//! chased value: a render reports every few hundred frames, so the raw fraction
//! arrives in visible jumps, and a bar that jumps reads as a bar that was stuck.
//! The indeterminate fill shuttles at the loader clock's rate, and stops when the
//! element stops being drawn — nothing here schedules a frame on its own.
//!
//! Split from [`super`] because a bar is not a loader: it says how far, not that
//! something is happening, and the two are drawn together.

use super::*;

// The glide store, for the determinate fill.
use crate::motion::glide;

/// The height of the bar. Thin enough to sit under a row of text without
/// becoming the row.
const BAR_H: f32 = 3.0;

/// How much of the track the indeterminate segment covers.
const SHUTTLE: f32 = 0.3;

/// A bar for `fraction`: filled to it when it is known, shuttling when it is not.
///
/// `id` must be unique per bar — it is both the chased value's identity and the
/// indeterminate phase offset, so two bars sharing a key travel as one.
///
/// Empty until the wait has lasted a settle, on the same clock every other loader
/// waits on. The track is drawn from the first frame and the fill is not, so a
/// job that finishes quickly costs one hairline appearing and disappearing rather
/// than a bar that filled and vanished, and the row's height never changes.
pub fn progress_bar(
    id: motion::ControlId,
    fraction: Option<f32>,
    theme: &Theme,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let track = div()
        .relative()
        .w_full()
        .h(px(BAR_H))
        .rounded(px(BAR_H / 2.0))
        .overflow_hidden()
        .bg(theme.wash(0.08));
    if !worth_showing(id) {
        return track;
    }
    match fraction {
        Some(fraction) => track.child(determinate(id, fraction, theme)),
        None => track.child(indeterminate(id, theme, view, cx)),
    }
}

/// A row that says what is happening and how far it has got.
///
/// The same leader as [`super::pending_row`], because a wait with a number is
/// still a wait: only the bar under the label is new.
pub fn progress_row(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    fraction: Option<f32>,
    theme: &Theme,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    // Erased before the row is built: the bar's opaque type captures the borrow
    // of `cx` it was made with, and the row needs that borrow too.
    let bar = progress_bar(id, fraction, theme, view, cx).into_any_element();
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(pending_row(id, label, theme, view, cx))
        .child(bar)
}

/// The fill for a known fraction, at the width the chased value has reached.
fn determinate(id: motion::ControlId, fraction: f32, theme: &Theme) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .w(gpui::relative(eased_width(id, fraction)))
        .rounded(px(BAR_H / 2.0))
        .bg(theme.accent)
}

/// How wide the determinate fill is drawn, in `0.0..=1.0` of its track.
///
/// Clamped on the way in and on the way out. On the way in because a fraction is
/// a number a job computes, and a job that counted frames it had not written
/// would draw past the track; on the way out because the chased value is only
/// bounded by the two ends of its own leg, and the leg it was on when the clamp
/// changed could have started outside.
fn eased_width(id: motion::ControlId, fraction: f32) -> f32 {
    glide::glide(
        id.motion("progress"),
        fraction.clamp(0.0, 1.0),
        glide::PROGRESS,
        false,
    )
    .clamp(0.0, 1.0)
}

/// The fill for a wait that cannot say how far it has got.
///
/// A shuttle rather than a sweep that wraps: the cosine eases at both ends, so
/// the segment slows before it turns, and there is no frame where it jumps from
/// one edge back to the other. Its rate is the loader clock's, so every
/// indeterminate thing in the window moves at one rate.
fn indeterminate(
    id: motion::ControlId,
    theme: &Theme,
    view: EntityId,
    cx: &mut App,
) -> impl IntoElement {
    let phase = motion::pulse_delta(&ZERON_PULSE, view, cx) + id.phase();
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(gpui::relative(shuttle_left(phase)))
        .w(gpui::relative(SHUTTLE))
        .rounded(px(BAR_H / 2.0))
        .bg(theme.accent)
}

/// Where the indeterminate segment's left edge sits at `phase`.
///
/// `phase` is unbounded — a key offset is added to the clock — so the wave is
/// what keeps this inside the track rather than a clamp.
fn shuttle_left(phase: f32) -> f32 {
    (1.0 - SHUTTLE) * loaders::pulse_wave(phase)
}

#[cfg(test)]
#[path = "loaders_progress_tests.rs"]
mod tests;
