//! Rows that read a value and act on it: the label in the label column, the
//! value on the controls column, and the row's actions at its end.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, px,
};

use super::widgets::chip_row;
use crate::app::AppState;
use crate::components;
use crate::shell::Shell;
use crate::theme::Theme;

/// `label`, then `value` at a chip's inset on the controls column, filling
/// the row up to `actions` at its end.
///
/// The value wraps; its first line, the label, and the actions share one
/// center line however many lines the value takes. The actions stand in one
/// control row at the top of the value, so a switch shorter than a control
/// row stands on that center line too.
pub(super) fn value_row(
    theme: &Theme,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    actions: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::AnyElement {
    let line = Theme::ROW_HEIGHT - 2.0 * Theme::SPACE_XXS;
    let value = div()
        .flex_1()
        .min_w_0()
        .px(px(Theme::SPACE_SM))
        .py(px((Theme::CONTROL_HEIGHT - line) / 2.0))
        .line_height(px(line))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .child(value.into());
    let actions = div()
        .self_start()
        .flex_none()
        .h(px(Theme::CONTROL_HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .children(actions);
    chip_row(
        theme,
        label,
        [value.into_any_element(), actions.into_any_element()],
    )
}

/// A number set without a text field: `label`, the value as `shown`, then
/// Halve and Double at the row's end. The settings set this way span orders
/// of magnitude, so a step halves or doubles rather than adding a constant.
/// `halve` and `double` are the buttons' control ids, each with the row it
/// is drawn on. `set` clamps and stores the stepped value; its error goes to
/// the error line.
pub(super) fn doubling_row(
    theme: &Theme,
    halve: (&'static str, &'static str),
    double: (&'static str, &'static str),
    label: &'static str,
    (shown, value): (String, u64),
    set: impl Fn(&mut AppState, u64) -> Result<(), String> + Copy + 'static,
    cx: &mut gpui::Context<Shell>,
) -> gpui::AnyElement {
    let step = |(key, row): (&'static str, &'static str), text: &'static str, next: u64| {
        components::ghost_button(crate::motion::MotionKey::new(key, row), text, theme)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = set(state, next) {
                            state.report_error(err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
    };
    let halve = step(halve, "Halve", value / 2);
    let double = step(double, "Double", value.saturating_mul(2));
    value_row(theme, label, shown, [halve, double])
}
