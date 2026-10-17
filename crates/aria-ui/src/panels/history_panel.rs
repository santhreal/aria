//! The History dock panel: every step this session took, and where the project
//! sits in it.
//!
//! Each row names one applied command. Clicking a row moves the project to the
//! state *after* that step, which is why the panel asks the engine for a depth
//! rather than for a count of undos.
//!
//! Undone steps stay in the list, dimmed, until a new edit clears the redo
//! stack. That is what redo can still reach, so the list covers both
//! directions rather than only the past.

use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;

/// The surface name of the panel.
const SURFACE: &str = "history-panel";

/// Window > History.
pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    // Owned for the length of the frame: the rows below attach listeners, which
    // needs the context mutably, and a borrow of the state would hold it.
    let entries = state.read(cx).history_entries();
    let depth = state.read(cx).history_depth();
    // A job still running will add steps of its own, so the list is provisional
    // until it lands. The loader separates "this is the whole list" from "this
    // is the list so far".
    let rebuilding = state.read(cx).running_job_count();
    let pending = (rebuilding > 0).then(|| {
        crate::loaders::pending_row(
            "history-rebuild",
            format!("{rebuilding} job(s) still writing history"),
            &theme,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    });

    // The opening state is a row of its own: without it there is no way to get
    // back to "before the first edit" by clicking.
    let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len() + 1);
    rows.push(row(&theme, 0, "Project opened", true, depth == 0, cx));
    for (index, entry) in entries.iter().enumerate() {
        rows.push(row(
            &theme,
            index + 1,
            &entry.label,
            entry.applied,
            depth == index + 1,
            cx,
        ));
    }

    div()
        .id(SURFACE)
        .debug_selector(|| SURFACE.into())
        .size_full()
        .overflow_y_scroll()
        .p(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(pending)
        .children(rows)
        .into_any_element()
}

/// One step. `applied` dims an undone step; `current` marks where the project
/// is, which is not the same thing: the newest applied step is the current
/// point, and every older applied step is still applied.
///
/// The whole row is the target. The current row takes no click, because moving
/// to the state the project is already in is no move.
fn row(
    theme: &Theme,
    depth: usize,
    label: &str,
    applied: bool,
    current: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let text = SharedString::from(label.to_string());
    // The hover key holds the depth: one key per row, or every row would fade
    // together with whichever one the pointer is on.
    let key = motion::MotionKey::new("history-row", depth);
    let rest = if current {
        theme.surface_raised
    } else {
        theme.wash(0.0)
    };
    div()
        .id(("history-row", depth))
        .debug_selector(move || format!("history-row-{depth}"))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(key, rest, theme.glass_hover()))
        .on_hover(motion::hover_listener(key))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(if applied {
                    theme.text
                } else {
                    theme.text_faint
                })
                .child(text),
        )
        .when(current, |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.accent)
                    .child("current"),
            )
        })
        .when(!current, |el| {
            el.cursor(gpui::CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        // Repainted whether or not the jump landed: a refusal
                        // writes the status line.
                        this.state.update(cx, |state, _| state.history_jump(depth));
                        cx.notify();
                    }),
                )
        })
        .into_any_element()
}
