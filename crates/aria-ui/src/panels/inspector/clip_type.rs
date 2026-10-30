//! The clip type row above the inspector's tabs: the type a plugin listed for
//! the clip, why it cannot draw when it cannot, and Detach.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::panels::inspector::blocks::widgets::pick_button;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClipType;

/// The row for `clip_type`.
pub(super) fn clip_type_row(
    theme: &Theme,
    clip_type: &UiClipType,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let swatch = clip_type.colour.map_or(theme.text_faint, |colour| {
        theme.label(aria_models::LabelColor::from(colour), false)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_SM))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .flex_none()
                        .size(px(10.))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .bg(swatch),
                )
                .children(clip_type.icon.map(|path| {
                    crate::icons::icon(path)
                        .size(px(14.))
                        .text_color(theme.text_muted)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(clip_type.label.clone()),
                )
                .child(pick_button(
                    "clip-type-detach",
                    "Detach",
                    theme,
                    cx,
                    |state| inspector_pick::apply(state, Pick::DetachClipType),
                )),
        )
        .children(clip_type.missing.clone().map(|reason| {
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.warning)
                .child(reason)
        }))
}
