//! The WASM limit rows at the top of the Plugins section.

use gpui::Entity;

use crate::app::AppState;
use crate::app::plugin_install::WasmLimit;
use crate::shell::Shell;
use crate::theme::Theme;

use super::value_row::doubling_row;

/// One row per WASM limit, from `WasmLimit::ALL`: the value and a pair of
/// steppers.
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
            doubling_row(
                theme,
                ("wasm-limit-dec", id),
                ("wasm-limit-inc", id),
                label,
                (limit.describe(value), value),
                move |state, next| state.set_wasm_limit(limit, next),
                cx,
            )
        })
        .collect()
}
