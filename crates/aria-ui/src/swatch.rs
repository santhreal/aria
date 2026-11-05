//! A swatch: a square of one color that picks that color.
//!
//! The label palette, the Edit Marker dialog, the Markers panel's color filter
//! and an Effect Controls color row draw their colors with it, so a color pick
//! has one size, one ring and one press wherever it appears.

use gpui::{
    CursorStyle, Div, Hsla, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::motion::ControlId;
use crate::theme::Theme;

/// The side of a swatch.
pub(crate) const SIDE: f32 = 16.0;

/// The square a swatch fills: [`SIDE`] on a side, rounded as a control is, and
/// pressable.
fn frame(id: impl Into<ControlId>) -> Stateful<Div> {
    div()
        .id(id.into().element("swatch"))
        .flex_none()
        .w(px(SIDE))
        .h(px(SIDE))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
}

/// A swatch of `fill`, ringed as [`ring`] rings it. The caller adds the press
/// and the tip that names the color.
pub(crate) fn swatch(
    id: impl Into<ControlId>,
    fill: Hsla,
    chosen: bool,
    theme: &Theme,
) -> Stateful<Div> {
    ring(frame(id).bg(fill), chosen, theme)
}

/// The swatch that picks no color: an outline with a cross centered in it, so
/// it reads as an absence rather than one more color, ringed as [`ring`] rings
/// it. The cross is the muted tone of a live control, not the faint one that
/// marks a disabled control.
pub(crate) fn clear(id: impl Into<ControlId>, chosen: bool, theme: &Theme) -> Stateful<Div> {
    let square = frame(id).flex().items_center().justify_center();
    ring(square, chosen, theme)
        .text_color(theme.text_muted)
        .child(crate::icons::icon(crate::icons::CLOSE).size(Theme::TEXT_SMALL))
}

/// `swatch` ringed in the text color while `chosen`, so a palette shows what
/// is set as well as what can be set, and in the strong border color at rest,
/// so a black swatch and the clear keep their edges on a dark surface. Inside
/// the chosen ring runs a line of the surface color, so the ring stands apart
/// from a fill as light as itself.
fn ring(swatch: Stateful<Div>, chosen: bool, theme: &Theme) -> Stateful<Div> {
    if chosen {
        let line = div()
            .absolute()
            .inset_0()
            .border_1()
            .border_color(theme.surface);
        swatch
            .relative()
            .border_2()
            .border_color(theme.text)
            .child(line)
    } else {
        swatch.border_1().border_color(theme.border_strong)
    }
}
