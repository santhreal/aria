//! A card: a titled group of rows on the raised surface, outlined in the
//! border color. A Settings section is one card and an AI tools registry
//! group is another.
//!
//! A card rather than a heading and a gap: a dozen groups stacked on one flat
//! surface leave only blank space to show which rows belong to which title.
//! The card is opaque and unblurred: the Settings dialog is frosted already,
//! and a backdrop blur per card costs a GPU pass each and muddies the text
//! behind it.

use gpui::{Div, ParentElement, SharedString, Styled, div, px};

use crate::components::section_label;
use crate::theme::Theme;

/// A card under `title`, drawn as [`section_label`] draws a heading. The
/// caller adds the rows with `.children`.
pub(crate) fn card(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface_raised)
        .border_1()
        .border_color(theme.border)
        .child(section_label(theme, title).pt_0())
}
