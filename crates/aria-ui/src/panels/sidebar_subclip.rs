//! Subclip creation and bin integration for the media sidebar.

use gpui::{AnyElement, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, div};

use crate::app::AppState;
use crate::components;
use crate::surfaces::sidebar_pick::{self, Pick};
use crate::theme::Theme;

/// Press one row of the registry, reporting a refusal where the operator is
/// looking rather than swallowing it.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = sidebar_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

/// UI control to create a subclip from the current source monitor in/out marks.
pub(super) fn subclip_button(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let can_subclip =
        state.read(cx).source_view.is_some() && state.read(cx).source_range().is_some();
    if !can_subclip {
        return div().into_any_element();
    }
    components::chip(
        "create-subclip",
        "Make Subclip",
        false,
        components::ChipSize::Compact,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| press(state, Pick::Subclip));
            cx.notify();
        }),
    )
    .into_any_element()
}
