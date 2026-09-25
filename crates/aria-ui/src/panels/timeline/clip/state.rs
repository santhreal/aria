//! The clip card's chased states: selection and lock.
//!
//! Split out of [`super`] so the card's builder chain stays one screen and so
//! the rule these two share has one place to live: a state a clip can leave
//! again is a chased value, never a one-shot. A one-shot cannot be retargeted,
//! and both of these change under the operator's hand — a marquee sweeping a
//! lane changes selection on consecutive frames, and Lock is a menu row that can
//! be hit twice. Queued one-shots would show that as a flicker; a chased value
//! shows it as a reversal from wherever the fade had got to.
//!
//! Both are opacity, not colour, because the ring and the badge are drawn over
//! whatever the card already shows: fading the alpha leaves the fill and the
//! filmstrip underneath untouched.
//!
//! The elements those values drive live here too. A card's chrome is what its
//! state means, and splitting the two would let the fade and the thing it fades
//! drift apart.

use super::*;

/// How far the selection ring has arrived, in `0.0..=1.0`.
///
/// Keyed per clip, so two cards can be mid-flight in opposite directions — which
/// is exactly what a click on one clip while another was selected produces.
pub(crate) fn selection_t(id: u64, selected: bool, reduced: bool) -> f32 {
    state_t("clip-ring", id, selected, reduced)
}

/// How far the lock badge has arrived, in `0.0..=1.0`.
pub(crate) fn lock_t(id: u64, locked: bool, reduced: bool) -> f32 {
    state_t("clip-lock", id, locked, reduced)
}

/// A binary state as a value that travels. `reduced` is passed through as the
/// glide store's `instant`, and the store also reads the preference itself, so
/// this cannot be the call site that forgets it.
fn state_t(kind: &'static str, id: u64, on: bool, reduced: bool) -> f32 {
    let target = if on { 1.0 } else { 0.0 };
    glide::glide(
        motion::MotionKey::new(kind, id),
        target,
        glide::STATE,
        reduced,
    )
    .clamp(0.0, 1.0)
}

/// Whether a fade has arrived far enough to be worth building an element for.
///
/// A card that has never been selected has to build nothing, or every clip in
/// the sequence would carry an invisible ring; the threshold is below one step
/// of 8-bit alpha, so nothing visible is skipped.
pub(crate) fn showing(t: f32) -> bool {
    t > 1.0 / 255.0
}

/// The ring drawn over a selected card, at `t` of its way in or out.
///
/// A border rather than an outline on the card itself, because the card clips
/// its children and a border there would move the filmstrip and the waveform in
/// by a pixel on the frame it appeared.
pub(crate) fn ring(theme: &Theme, t: f32) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .rounded(px(Theme::CONTROL_RADIUS))
        .border_1()
        .border_color(theme.accent.opacity(t))
}

/// The inset hairline the theme gives a selected card, at `t` of its way in or
/// out. Scaled rather than replaced, so the token still owns the colour.
pub(crate) fn selected_shadows(t: f32) -> Vec<gpui::BoxShadow> {
    theme::glass_selected_shadows()
        .into_iter()
        .map(|shadow| gpui::BoxShadow {
            color: shadow.color.opacity(t),
            ..shadow
        })
        .collect()
}

/// The badge on a locked card, at `t` of its way in or out.
///
/// Locked used to be readable only by trying to drag: the cursor changed and the
/// trim handles were absent. The badge takes the corner those handles occupy,
/// which is free precisely when it is drawn.
pub(crate) fn lock_badge(theme: &Theme, t: f32) -> impl IntoElement {
    div()
        .absolute()
        .right(px(Theme::SPACE_XS))
        .top(px(2.))
        .text_color(theme.text_faint.opacity(t))
        .child(crate::icons::icon(crate::icons::LOCK).size(px(10.)))
}

#[cfg(test)]
mod tests;
