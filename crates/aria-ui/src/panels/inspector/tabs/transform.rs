//! Transform tab: position, scale, rotation, anchor, opacity.
//!
//! Every row comes from the property catalog, so the block keeps no list of the
//! properties it owns and every one of them is written the keyframe-aware way. A
//! hand-listed block wrote the whole transform on each nudge, which replaced an
//! animated property with the constant the pointer left it at, and it silently
//! lacked the anchor point the catalog has always carried.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::components;
use crate::theme::Theme;
use crate::types::UiClip;

use crate::panels::inspector::blocks::widgets::{param_rows, section_label};

pub fn transform_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let rows = param_rows(theme, "transform", state, cx);
    let reset = components::icon_button("inspector-reset", crate::icons::ROTATE_CCW, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.reset_selected_transform() {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Transform"))
        .children(rows)
        .child(div().pt(px(Theme::SPACE_SM)).child(reset))
        .child(crate::panels::inspector::blocks::framing::framing_block(
            theme, clip, cx,
        ))
        .child(crate::panels::inspector::blocks::keyframes::keyframes_block(theme, clip, state, cx))
        .child(
            crate::panels::inspector::blocks::curve_editor::curve_editor_block(
                theme, clip, state, cx,
            ),
        )
}
