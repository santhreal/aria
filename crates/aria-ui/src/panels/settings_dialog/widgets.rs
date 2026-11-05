//! The shapes every settings section is built from.

use gpui::{
    Div, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components::{self, ChipSize};
use crate::theme::Theme;

/// How wide the label column every Settings row starts with is, so the
/// controls after the labels line up down the dialog.
const LABEL_WIDTH: f32 = 140.0;

/// How far a section's rows stand in from its card's outer edge: the card's
/// padding and its 1 px border.
pub(super) const ROW_INSET: f32 = Theme::SPACE_SM + 1.0;

/// The label a Settings row starts with, in the label column.
pub(super) fn row_label(theme: &Theme, label: impl Into<SharedString>) -> Div {
    crate::components::row_label(theme, label, LABEL_WIDTH)
}

/// A row of controls: `label` in the label column, then the controls,
/// `SPACE_XS` apart and wrapping in the rest of the row: a chip per choice
/// with the one in force lit, a switch, or buttons.
///
/// The row is at least one control row high, and the label stands on the
/// center line of the first line of controls however many lines they wrap
/// onto. A line shorter than a control row, a switch's, stands on that center
/// line too.
pub(super) fn chip_row(
    theme: &Theme,
    label: impl Into<SharedString>,
    controls: impl IntoIterator<Item = impl IntoElement>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_SM))
        .child(
            row_label(theme, label)
                .min_h(px(Theme::CONTROL_HEIGHT))
                .flex()
                .items_center(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(Theme::CONTROL_HEIGHT))
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .content_center()
                .gap(px(Theme::SPACE_XS))
                .children(controls),
        )
        .into_any_element()
}

/// One row of mutually exclusive chips, the one in force lit. A press runs
/// `apply` with the chip's choice and reports its error on the error line.
/// `prefix` is the chips' control id.
pub(super) fn choice_row<T: Copy + PartialEq + 'static>(
    theme: &Theme,
    prefix: &'static str,
    label: &'static str,
    choices: &[T],
    name_of: fn(T) -> &'static str,
    current: T,
    cx: &mut gpui::Context<crate::shell::Shell>,
    apply: fn(&mut AppState, T, &mut gpui::Context<AppState>) -> Result<(), String>,
) -> gpui::AnyElement {
    let chips = choices.iter().map(|&choice| {
        let name = name_of(choice);
        let id = crate::motion::MotionKey::new(prefix, name);
        components::chip(id, name, choice == current, ChipSize::Regular, theme).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, cx| {
                    if let Err(err) = apply(state, choice, cx) {
                        state.report_error(err);
                    }
                });
                cx.notify();
            }),
        )
    });
    chip_row(theme, label, chips)
}

/// A row that reads a value: `label` in the label column, then `value` at a
/// chip's inset on the column the controls of the other rows start on,
/// wrapping in the rest of the row with the label level with its first line.
///
/// One line of text, padded by `SPACE_XXS` above and below, fills one
/// `ROW_HEIGHT` row, and the label and every line of the value share that line
/// height, so the label stands on the value's first line however many lines
/// the value takes.
pub(super) fn kv(theme: &Theme, label: &str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .line_height(px(Theme::ROW_HEIGHT - 2.0 * Theme::SPACE_XXS))
        .child(row_label(theme, label.to_string()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px(px(Theme::SPACE_SM))
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

/// A row a click picks or arms: its text at the left and any actions at the
/// right, on the plate every selected row holds while `selected`, or fading to
/// the hover fill under the pointer over `fade`. The plate steps out by its
/// inset on each side, so the row's text stands on the section title's edge
/// and its last action ends on the card's column, as every other settings
/// row's do.
pub(super) fn row_plate(
    id: crate::motion::ControlId,
    theme: &Theme,
    fade: crate::motion::MotionKey,
    selected: bool,
) -> Stateful<Div> {
    div()
        .id(id.element("row-plate"))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_XS))
        .mx(px(-Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(crate::motion::row_fill(fade, selected, theme))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(crate::motion::hover_listener(fade))
}
