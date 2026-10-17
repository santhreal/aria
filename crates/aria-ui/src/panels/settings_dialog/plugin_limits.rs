//! The WASM limit rows at the top of the Plugins section.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Stateful, Styled, div, px,
};

use crate::app::AppState;
use crate::app::plugin_install::WasmLimit;
use crate::components;
use crate::shell::Shell;
use crate::theme::Theme;

/// One row per WASM limit, from `WasmLimit::ALL`: the value and a pair of
/// steppers. The ranges span three orders of magnitude, so a step halves or
/// doubles.
pub(super) fn limit_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> Vec<gpui::AnyElement> {
    let limits = state.read(cx).wasm_limits();
    WasmLimit::ALL
        .into_iter()
        .map(|(limit, id, label)| {
            let value = limit.value(limits);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(format!(
                            "{label}: {}",
                            limit.describe(value)
                        ))),
                )
                .child(stepper(
                    theme,
                    limit,
                    ("wasm-limit-dec", id),
                    "Halve",
                    value / 2,
                    cx,
                ))
                .child(stepper(
                    theme,
                    limit,
                    ("wasm-limit-inc", id),
                    "Double",
                    value.saturating_mul(2),
                    cx,
                ))
                .into_any_element()
        })
        .collect()
}

/// A button that sets `limit` to `next`.
fn stepper(
    theme: &Theme,
    limit: WasmLimit,
    (key, id): (&'static str, &'static str),
    text: &'static str,
    next: u64,
    cx: &mut gpui::Context<Shell>,
) -> Stateful<gpui::Div> {
    components::ghost_button(crate::motion::MotionKey::new(key, id), text, theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if let Err(err) = state.set_wasm_limit(limit, next) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    )
}
