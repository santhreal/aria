//! The keyframe affordances of an inspector row: the stopwatch left of the
//! label, and the keyframe navigator, reset button and time area right of the
//! value.

use super::super::key_strip::key_strip;
use crate::motion::{self, glide};
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use aria_models::ClipParam;
use gpui::motion::{lerp, presets};
use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use super::{DOT, Stopwatch};

/// The stopwatch.
///
/// A click on a constant property animates it with a keyframe at the playhead;
/// a click on an animated one drops every keyframe and keeps the value at the
/// playhead, as one undo step. Alt+click adds a driver, or removes the one the
/// property has. Three states are drawn, and the travel between them is the
/// point: constant, animated, and animated with a keyframe under the playhead.
/// The dot grows into each state rather than switching, so a misclick is
/// visible as it happens.
pub(super) fn stopwatch_dot(
    theme: &Theme,
    key: motion::MotionKey,
    watch: Stopwatch,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let param = watch.param;
    let driven = watch.driver.is_some();
    let animated = watch.animated;
    let pinned = watch.at_playhead;
    // One scalar carries the whole affordance, so the two transitions cannot
    // run at different speeds: 0 constant, 1 animated, 2 pinned.
    let target = if pinned {
        2.0
    } else if animated {
        1.0
    } else {
        0.0
    };
    let at = glide::glide(key.with("watch"), target, presets::CHEVRON, false);
    let on = at.min(1.0);
    let pin = (at - 1.0).clamp(0.0, 1.0);
    let ring = if driven {
        theme.warning
    } else {
        motion::mix(theme.text_faint, theme.accent, on)
    };
    let core = px(lerp(0.0, DOT - 3.0, on) + lerp(0.0, 2.0, pin));
    div()
        .id(key.with("watch").element_id("insp-key"))
        .debug_selector(move || format!("insp-key-{param:?}"))
        .size(px(DOT + 4.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .child(
            div()
                .size(px(DOT))
                .rounded(px(DOT / 2.0))
                .border_1()
                .border_color(ring)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(core).rounded(px(DOT / 2.0)).bg(ring)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                // The row underneath nudges on the same button; a click on the
                // dot is about the curve, not the value.
                cx.stop_propagation();
                let alt = event.modifiers.alt;
                this.state.update(cx, |state, _| {
                    // The press answers the dot that was drawn, not what an
                    // edit in the same frame made of the curve.
                    let row = if alt {
                        Row::Driver(param)
                    } else if animated {
                        Row::Flatten(param)
                    } else {
                        Row::Key {
                            param,
                            keyed: false,
                        }
                    };
                    let wrote = keyframe_rows::apply(state, row);
                    if let Err(err) = wrote {
                        state.report_error(&err);
                    }
                });
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
        .with_tip(crate::tooltip::deferred_builder("prop-watch"))
}

/// Everything right of a row's value: the keyframe navigator (previous
/// keyframe, a keyframe at the playhead, next keyframe) and the reset button,
/// which never narrow, then the property's time area when the surface draws
/// one. The time area is returned apart so the row places it beside the label,
/// where the two share the row's shrink.
pub(super) fn key_controls(
    theme: &Theme,
    key: motion::MotionKey,
    watch: &mut Stopwatch,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> (gpui::AnyElement, Option<gpui::AnyElement>) {
    let param = watch.param;
    let (before, after) = watch.keys_around;
    let keyed = watch.at_playhead;
    let arrow = |lit: bool| {
        if lit {
            theme.text_muted
        } else {
            theme.text_faint.opacity(0.4)
        }
    };
    let (previous_ink, next_ink) = (arrow(before), arrow(after));
    let diamond_ink = if keyed {
        theme.accent
    } else {
        theme.text_muted
    };
    let strip = watch
        .strip
        .take()
        .map(|strip| key_strip(theme, key, strip, cx).into_any_element());
    let controls = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(Theme::SPACE_XXS))
        .child(key_glyph(
            key,
            param,
            "kf-prev",
            "‹",
            previous_ink,
            move || Row::Previous(param),
            cx,
        ))
        .child(key_glyph(
            key,
            param,
            "kf-at",
            if keyed { "◆" } else { "◇" },
            diamond_ink,
            move || Row::Key { param, keyed },
            cx,
        ))
        .child(key_glyph(
            key,
            param,
            "kf-next",
            "›",
            next_ink,
            move || Row::Next(param),
            cx,
        ))
        .child(
            crate::components::icon_button(("prop-reset", key), crate::icons::ROTATE_CCW, theme)
                .size(px(Theme::ICON_HIT_ROW))
                .debug_selector(move || format!("insp-prop-reset-{param:?}"))
                .when(!watch.off_default, |button| button.opacity(0.35))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            if let Err(err) = keyframe_rows::apply(state, Row::Reset(param)) {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
        .into_any_element();
    (controls, strip)
}

/// One glyph of the keyframe navigator. `control` is its tooltip id, and with
/// the property it is the element's debug selector, `insp-kf-prev-Opacity`.
fn key_glyph(
    key: motion::MotionKey,
    param: ClipParam,
    control: &'static str,
    glyph: &'static str,
    ink: gpui::Hsla,
    press: impl Fn() -> Row + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id(key.element_id(control))
        .debug_selector(move || format!("insp-{control}-{param:?}"))
        .with_tip(crate::tooltip::deferred_builder(control))
        .w(px(Theme::SPACE_MD))
        .flex_none()
        .flex()
        .justify_center()
        .text_size(Theme::TEXT_UI)
        .text_color(ink)
        .cursor(CursorStyle::PointingHand)
        .child(glyph)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                // The row underneath scrubs on the same button.
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    if let Err(err) = keyframe_rows::apply(state, press()) {
                        state.report_error(&err);
                    }
                });
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}
