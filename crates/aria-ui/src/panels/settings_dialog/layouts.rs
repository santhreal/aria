//! The workspace list.
//!
//! Generated from [`AppState::workspace_rows`], which is the same list the Window
//! menu draws, so the two cannot offer different arrangements. Under its name,
//! each row shows the pane sizes it applies, since an arrangement is easier to
//! recognize by its numbers than by its name; the row on screen reads "In use".

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::app::layouts::WorkspaceRow;
use crate::components;
use crate::motion;
use crate::theme::Theme;

/// The hint and save controls, then one row per workspace.
pub(super) fn workspace_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows = state.read(cx).workspace_rows();
    let saved = state.read(cx).ui_settings.workspace.is_some();
    let mut out: Vec<gpui::AnyElement> = Vec::with_capacity(rows.len() + 1);
    out.push(save_row(theme, saved, cx).into_any_element());
    for (index, row) in rows.into_iter().enumerate() {
        out.push(workspace_row(theme, row, index, cx).into_any_element());
    }
    out
}

/// What one workspace measures, in the order the panes appear on screen.
fn measurements(row: &WorkspaceRow) -> String {
    let rail = if row.panes.sidebar_collapsed {
        "Rail hidden".to_string()
    } else {
        format!("Rail {:.0}", row.panes.sidebar_width)
    };
    let inspector = if row.panes.inspector_collapsed {
        "Inspector hidden".to_string()
    } else {
        format!("Inspector {:.0}", row.panes.inspector_width)
    };
    let scopes = if row.panes.scopes_open {
        " · Scopes"
    } else {
        ""
    };
    format!(
        "{rail} · {inspector} · Timeline {:.0}{scopes}",
        row.panes.timeline_height
    )
}

/// One workspace: its name, what it measures, and whether it is what is on screen.
fn workspace_row(
    theme: &Theme,
    row: WorkspaceRow,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let id = row.id;
    let control = motion::ControlId::from(("settings-workspace", index));
    let fade = control.motion("row");
    let numbers = match row.plugin {
        Some(plugin) => format!("{plugin} \u{b7} {}", measurements(&row)),
        None => measurements(&row),
    };
    // The measurements are the line under the name; the right edge holds only
    // the row's state, so a row never reads the same numbers twice.
    let mut actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .when(row.active, |actions| {
            actions.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.accent)
                    .child("In use"),
            )
        });
    // Forgetting is offered on the row it would empty, not on the hint line: the
    // slot is the row, and a button somewhere else would leave it unclear
    // which one it meant.
    if row.custom {
        actions = actions.child(
            components::ghost_button(
                motion::MotionKey::new("settings-workspace-forget", index),
                "Forget",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state
                        .update(cx, |state, cx| state.forget_workspace(cx));
                    cx.notify();
                }),
            ),
        );
    }
    super::widgets::row_plate(control, theme, fade, row.active)
        .py(px(Theme::SPACE_XS))
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(row.name),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_faint)
                        .child(numbers),
                ),
        )
        .child(actions)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, cx| {
                    state.apply_workspace(id, cx);
                });
                cx.notify();
            }),
        )
}

/// Keep the arrangement that is on screen. The label says which of the two things
/// it does: there is one slot, so saving again replaces it.
fn save_row(
    theme: &Theme,
    saved: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let label = if saved {
        "Replace Custom"
    } else {
        "Save as Custom"
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child("Click a workspace to arrange the panes that way"),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::ghost_button("settings-workspace-reset", "Reset", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, cx| {
                                    state.reset_workspace_layout(cx);
                                });
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    components::ghost_button("settings-workspace-save", label, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, cx| state.save_workspace(cx));
                                cx.notify();
                            }),
                        ),
                ),
        )
}
