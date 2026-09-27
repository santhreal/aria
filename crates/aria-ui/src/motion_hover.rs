//! Hover fades: the toggle behind every hovered control, its listener, and the
//! colour blend it feeds.
//!
//! A hover is not a one-shot transition: the pointer can leave halfway. So it
//! is a toggle in the shared hover store rather than a [`gpui::Animation`],
//! keyed by [`MotionKey`] and eased by [`presets::HOVER_FADE`], so every strip
//! in the app fades at one rate and a control that stopped being drawn stops
//! costing anything.

use gpui::motion::{FrameInstant, MotionPolicy, mix_premultiplied, presets};
use gpui::{App, Hsla, Rgba, Window};

use super::{CHROME, MotionKey, with_frame};

/// Turns the hover toggle of `key` on or off at `now` under `policy`, fading
/// from its current value.
pub fn set_hover(key: MotionKey, hovered: bool, policy: MotionPolicy, now: FrameInstant) {
    CHROME.with(|chrome| {
        chrome
            .borrow_mut()
            .hover
            .set_toggle(key, hovered, presets::HOVER_FADE, policy, now)
    });
}

/// The hover listener of the control keyed by `key`: sets its toggle under the
/// app's motion policy and redraws the window.
pub fn hover_listener(key: MotionKey) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |hovered, window, cx| {
        set_hover(key, *hovered, cx.motion_policy(), cx.frame_instant());
        window.refresh();
    }
}

/// How far the hover fade of `key` has come in the open frame: 0 at rest, 1
/// fully hovered.
pub fn hover_t(key: MotionKey) -> f32 {
    with_frame(|chrome, frame| chrome.hover.toggle_value(&key, frame.policy(), frame.now()))
}

/// Blends two colours through [`mix_premultiplied`], so a fade out of a
/// transparent colour does not travel through that colour's hue. `t <= 0`
/// returns `from` and `t >= 1` returns `to` unchanged.
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    if t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let (start, end) = (Rgba::from(from), Rgba::from(to));
    let [r, g, b, a] = mix_premultiplied(
        [start.r, start.g, start.b, start.a],
        [end.r, end.g, end.b, end.a],
        t,
    );
    Hsla::from(Rgba { r, g, b, a })
}

/// `rest` blended toward `hover` by the hover fade of `key`.
pub fn hover_blend(key: MotionKey, rest: Hsla, hover: Hsla) -> Hsla {
    mix(rest, hover, hover_t(key))
}
