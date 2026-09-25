//! The history list: every step this session took, and where the project sits
//! in it.
//!
//! Undo used to be a keystroke into the dark — the operator could press Ctrl+Z
//! but not see what it would take back. Each row names one applied command;
//! clicking a row moves the project to the state *after* that step, which is why
//! the panel asks the engine for a depth rather than for a count of undos.
//!
//! Undone steps stay in the list, dimmed, until a new edit clears the redo
//! stack. That is exactly what redo can still reach, so the list is the truth
//! about both directions rather than a log of the past.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::dialog_surface;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;

pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // Owned for the length of the frame: the rows below attach listeners, which
    // needs the context mutably, and a borrow of the state would hold it.
    let entries = state.read(cx).history_entries();
    let depth = state.read(cx).history_depth();
    // A job still running will add steps of its own, so the list is provisional
    // until it lands. The loader is the difference between "this is the whole
    // list" and "this is the list so far".
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
    let mut rows: Vec<gpui::AnyElement> =
        vec![row(&theme, 0, "Project opened", true, depth == 0, cx)];
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

    let done = components::ghost_button("history-done", "Done", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(pending)
        .children(rows)
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .pt(px(Theme::SPACE_SM))
                .child(done),
        );
    dialog_surface("history", DialogKind::History, "History", state, cx, body)
}

/// One step. `applied` dims an undone step; `current` marks where the project
/// actually is, which is not the same thing: the newest applied step is the
/// current point, and every older applied step is still applied.
fn row(
    theme: &Theme,
    depth: usize,
    label: &str,
    applied: bool,
    current: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let text = SharedString::from(label.to_string());
    // The hover key carries the depth: one key per row, or every row would fade
    // together with whichever one the pointer is on.
    let key = motion::MotionKey::new("history-row", depth);
    let rest = if current {
        theme.surface_raised
    } else {
        theme.wash(0.0)
    };
    div()
        .id(("history-row", depth))
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
                .text_size(px(13.))
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
                    .text_size(px(11.))
                    .text_color(theme.accent)
                    .child("current"),
            )
        })
        .when(!current, |el| {
            el.child(
                components::ghost_button(
                    motion::MotionKey::new("history-jump", depth),
                    "Go",
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| state.history_jump(depth));
                        cx.notify();
                    }),
                ),
            )
        })
        .into_any_element()
}
