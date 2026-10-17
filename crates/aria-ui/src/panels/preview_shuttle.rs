//! The shuttle readout: what J and L did, and what the mixer is doing about it.
//!
//! J, K and L moved the transport off 1x and nothing on screen said so — the
//! picture ran at 4x, or backwards, behind a play button that looks exactly the
//! way it looks at 1x. The badge is drawn only while the mixer is not following
//! the transport, states the rate, says the audio is silent there, and takes a
//! click back to 1x so an operator never has to count key presses to get home.
//!
//! Presence is [`AppState::audio_follows`], not a second reading of `shuttle`:
//! the badge is on screen exactly when the mixer has been told to go quiet.

use crate::tooltip::WithTip as _;
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;

/// How a shuttle rate reads on the badge: `8x`, `-8x`, `4.1x`.
pub(super) fn rate_label(shuttle: crate::transport::ShuttleSpeed) -> String {
    shuttle.to_string()
}

/// The badge, or nothing while the mixer is following the transport.
pub(super) fn badge(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let shuttle = {
        let state = state.read(cx);
        if state.audio_follows() {
            return None;
        }
        state.shuttle
    };
    let key = motion::MotionKey::new("shuttle", "preview-shuttle");
    let element = div()
        .id("preview-shuttle")
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(key, theme.warning_muted, theme.warning))
        .text_size(Theme::TEXT_UI)
        .font_family(theme.font_mono.clone())
        .text_color(motion::hover_blend(key, theme.warning, theme.on_solid))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .when_some(crate::tooltip::builder("preview-shuttle"), |el, tip| {
            el.with_tip(tip)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                // Back to 1x, which is also the one speed the mixer can play, so
                // the click that dismisses the badge is the click that restores
                // the sound.
                this.state.update(cx, |state, _| state.set_shuttle(1));
                cx.notify();
            }),
        )
        .child(crate::icons::icon(crate::icons::VOLUME_X).size(px(11.)))
        .child(rate_label(shuttle));
    // Erased here: the element holds the borrow of `cx` it was built with, and a
    // builder chain that carried it would keep that borrow across the caller's
    // own reads of the app.
    Some(motion::fade_in("preview-shuttle-in", element).into_any_element())
}

#[cfg(test)]
#[path = "preview_shuttle_tests.rs"]
mod tests;
