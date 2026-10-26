//! The shapes every settings section is built from.

use gpui::{Div, IntoElement, ParentElement, SharedString, Stateful, Styled, div, px};

use crate::theme::Theme;

/// How wide the label column every Settings row starts with is, so the
/// controls after the labels line up down the dialog.
const LABEL_WIDTH: f32 = 140.0;

/// How far a section's rows stand in from its card's outer edge: the card's
/// padding and its 1 px border.
pub(super) const ROW_INSET: f32 = Theme::SPACE_SM + 1.0;

/// The label a Settings row starts with, in the label column.
pub(super) fn row_label(theme: &Theme, label: impl Into<SharedString>) -> Div {
    div()
        .w(px(LABEL_WIDTH))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(label.into())
}

/// A row of choices: `label` in the label column, then one chip per choice
/// with the one in force lit, `SPACE_XS` apart.
pub(super) fn chip_row(
    theme: &Theme,
    label: &'static str,
    chips: impl IntoIterator<Item = Stateful<Div>>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(row_label(theme, label))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(Theme::SPACE_XS))
                .children(chips),
        )
        .into_any_element()
}

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

/// A horizontal hairline separating distinct choice groups inside one section card.
pub(super) fn group_divider(theme: &Theme) -> gpui::AnyElement {
    div()
        .h(px(1.0))
        .my(px(Theme::SPACE_XXS))
        .bg(theme.hairline)
        .into_any_element()
}
