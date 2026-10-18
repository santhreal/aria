//! Hover fades: the toggle behind every hovered control, its listener, and the
//! colour blend it feeds.
//!
//! A hover is not a one-shot transition: the pointer can leave halfway. So it
//! is a toggle in the shared hover store rather than a [`gpui::Animation`],
//! keyed by [`MotionKey`] and eased by [`presets::HOVER_FADE`], so every strip
//! in the app fades at one rate and a control that stopped being drawn stops
//! costing anything.

use gpui::motion::{FrameInstant, MotionPolicy, lerp, mix_premultiplied, presets};
use gpui::{
    App, Hsla, InteractiveElement, ParentElement, Rgba, StatefulInteractiveElement, Styled, Window,
    div, px,
};

use super::{CHROME, MotionKey, with_frame};

/// Turns the hover toggle of `key` on or off at `now` under `policy`, fading
/// from its current value.
pub fn set_hover(key: MotionKey, hovered: bool, policy: MotionPolicy, now: FrameInstant) {
    CHROME.with(|chrome| {
        let chrome = &mut *chrome.borrow_mut();
        let policy = chrome.policy(policy);
        let before = chrome.hover.toggle_value(&key, policy, now);
        let target = if hovered { 1.0 } else { 0.0 };
        if !policy.reduced() && before != target {
            chrome.hover_started.insert(key, now);
        } else {
            chrome.hover_started.remove(&key);
        }
        chrome
            .hover
            .set_toggle(key, hovered, presets::HOVER_FADE, policy, now)
    });
}

/// The hover listener of the control keyed by `key`: sets its toggle under the
/// app's motion policy and redraws the cached panel body or window chrome that
/// holds the control.
pub fn hover_listener(key: MotionKey) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let view = super::building_view().or_else(super::chrome_view);
    move |hovered, window, cx| {
        set_hover(key, *hovered, cx.motion_policy(), cx.frame_instant());
        match view {
            Some(view) => cx.notify(view),
            None => window.refresh(),
        }
    }
}

/// A control that rises as its hover fade comes in.
///
/// gpui decides hover from the bounds an element is painted at, so a control
/// that carried its own hover listener would take its hover region up with it:
/// with the pointer resting within the rise of its lower edge, the risen
/// control is no longer hovered, falls back under the pointer, is hovered
/// again, and the window draws a frame for every flip for as long as the
/// pointer rests there. Here the listener is on an empty child laid over the
/// control's rest bounds, which do not move.
pub trait RiseOnHover: ParentElement + Styled + Sized {
    /// Raises `self` by up to `rise` pixels as `lit` (its hover fade) goes from
    /// 0 to 1, and calls `listener` when the pointer enters or leaves the bounds
    /// `self` has at rest.
    fn rise_on_hover(
        self,
        rise: f32,
        lit: f32,
        listener: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        let lift = lerp(0.0, rise, lit);
        self.relative().top(px(-lift)).child(
            div()
                .id("hover-rest")
                .absolute()
                .left_0()
                .right_0()
                .top(px(lift))
                .bottom(px(-lift))
                .on_hover(listener),
        )
    }
}

impl<E: ParentElement + Styled> RiseOnHover for E {}

/// How far the hover fade of `key` has come in the open frame: 0 at rest, 1
/// fully hovered.
pub fn hover_t(key: MotionKey) -> f32 {
    with_frame(|chrome, frame| {
        let policy = chrome.policy(frame.policy());
        let t = chrome.hover.toggle_value(&key, policy, frame.now());
        let started = chrome.hover_started.get(&key).copied() == Some(frame.now());
        if (t > 0.0 && t < 1.0) || (!policy.reduced() && started) {
            chrome.views.moving();
        }
        t
    })
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
