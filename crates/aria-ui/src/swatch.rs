//! A swatch: a square of one color that picks that color.
//!
//! The label palette, the Edit Marker dialog and the Markers panel's color filter
//! draw their colors with it, so a color pick has one size, one ring and one press
//! wherever it appears.

use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, Div, Hsla, InteractiveElement, Stateful, StatefulInteractiveElement, Styled, div,
    px,
};

use crate::motion::ControlId;
use crate::theme::Theme;

/// The side of a swatch.
pub(crate) const SIDE: f32 = 16.0;

/// The square a swatch fills: [`SIDE`] on a side, rounded as a control is, and
/// pressable. A swatch that draws something other than a fill, as the label
/// palette's clear does, starts from this.
pub(crate) fn frame(id: impl Into<ControlId>) -> Stateful<Div> {
    div()
        .id(id.into().element("swatch"))
        .flex_none()
        .w(px(SIDE))
        .h(px(SIDE))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
}

/// A swatch of `fill`, ringed in the text color while `chosen`, so a palette
/// shows what is set as well as what can be set. The caller adds the press and
/// the tip that names the color.
pub(crate) fn swatch(
    id: impl Into<ControlId>,
    fill: Hsla,
    chosen: bool,
    theme: &Theme,
) -> Stateful<Div> {
    frame(id)
        .bg(fill)
        .when(chosen, |el| el.border_2().border_color(theme.text))
}
