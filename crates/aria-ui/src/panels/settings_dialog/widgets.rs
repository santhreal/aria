//! The two shapes every settings section is built from.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::theme::Theme;

/// One titled group of rows.
///
/// A card rather than a heading and a gap: a dozen sections stacked on one flat
/// surface leave only blank space to show which rows belong to which title.
/// Opaque and unblurred on purpose: the dialog it sits in is already frosted,
/// and a second backdrop blur per section would cost a GPU pass each to make
/// the text behind it muddier.
pub(super) fn section(theme: &Theme, title: &str, rows: Vec<gpui::AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface_raised)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_faint)
                .child(title.to_string()),
        )
        .children(rows)
}

pub(super) fn kv(theme: &Theme, label: &str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_MD))
        .min_h(px(Theme::ROW_HEIGHT))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(value.into()),
        )
}
