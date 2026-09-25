//! The workspace list.
//!
//! Generated from [`AppState::workspace_rows`], which is the same list the Window
//! menu draws, so the two cannot offer different arrangements. Each row says what
//! it would do to the panes before it is clicked, because an arrangement is
//! easier to recognise by its numbers than by its name.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
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
        "rail hidden".to_string()
    } else {
        format!("rail {:.0}", row.panes.sidebar_width)
    };
    let inspector = if row.panes.inspector_collapsed {
        "inspector hidden".to_string()
    } else {
        format!("inspector {:.0}", row.panes.inspector_width)
    };
    let scopes = if row.panes.scopes_open {
        " · scopes"
    } else {
        ""
    };
    format!(
        "{rail} · {inspector} · timeline {:.0}{scopes}",
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
    let element_id = gpui::ElementId::from(("settings-workspace", index));
    let fade = motion::MotionKey::new("settings-workspace", &element_id);
    let numbers = measurements(&row);
    let tone = if row.active {
        theme.accent
    } else {
        theme.text_faint
    };
    let mut actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .font_family(theme.font_mono.clone())
                .text_size(px(11.))
                .text_color(tone)
                .child(if row.active {
                    "in use".to_string()
                } else {
                    numbers.clone()
                }),
        );
    // Forgetting is offered on the row it would empty, not on the hint line: the
    // slot is the row, and a button somewhere else would ask the operator to
    // remember which one it meant.
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
    div()
        .id(element_id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            fade,
            if row.active {
                theme.glass_selected_bg()
            } else {
                theme.wash(0.0)
            },
            theme.glass_hover(),
        ))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(fade))
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text)
                        .child(row.name),
                )
                .child(
                    div()
                        .text_size(px(10.))
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
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child("Click a workspace to arrange the panes that way"),
        )
        .child(
            components::ghost_button("settings-workspace-save", label, theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, cx| state.save_workspace(cx));
                    cx.notify();
                }),
            ),
        )
}
