//! The rebinding surface.
//!
//! Every binding the registry holds, in registration order, with the chord it
//! answers to now. Clicking a row arms it, and the next keystroke is its chord:
//! the only spelling that cannot be wrong, because what the capture sees is what
//! dispatch will see.
//!
//! The list is generated from [`AppState::binding_rows`], so a chord contributed
//! by a plugin is rebindable without an edit here, and the refusals the registry
//! recorded are listed after it — a key that silently does nothing is the thing
//! this registry exists to make visible.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::keys::{self, BindingRow};
use crate::motion;
use crate::theme::Theme;

/// The reset, every binding, then any refusal.
pub(super) fn binding_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows = state.read(cx).binding_rows();
    let conflicts = state.read(cx).binding_conflicts();
    let armed = state.read(cx).rebinding();
    let mut out: Vec<gpui::AnyElement> = Vec::with_capacity(rows.len() + conflicts.len() + 1);
    out.push(reset_row(theme, cx).into_any_element());
    for (index, row) in rows.into_iter().enumerate() {
        let is_armed = armed == Some(row.binding.id);
        out.push(binding_row(theme, row, is_armed, index, cx).into_any_element());
    }
    for conflict in conflicts {
        out.push(
            super::widgets::kv(
                theme,
                &format!("{} was refused", keys::binding_label(conflict.refused)),
                format!(
                    "{} is held by {}",
                    conflict.chord.label(),
                    keys::binding_label(conflict.held_by)
                ),
            )
            .into_any_element(),
        );
    }
    out
}

/// One binding: what it does, what it answers to, and how to change it.
fn binding_row(
    theme: &Theme,
    row: BindingRow,
    armed: bool,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let id = row.binding.id;
    let element_id = gpui::ElementId::from(("settings-key", index));
    let fade = motion::MotionKey::new("settings-key", &element_id);
    // Three states, and each one says what to do next: waiting for a keystroke,
    // holding a chord, or holding none because it was cleared.
    let (shown, tone) = if armed {
        ("Press a key · Esc to cancel".to_string(), theme.accent)
    } else {
        match row.chord {
            Some(chord) => (chord.label(), theme.text),
            None => ("unbound".to_string(), theme.text_faint),
        }
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
                .child(shown),
        );
    // Clearing is only offered where there is something to clear, and reverting
    // only where the shipped chord is not already the one in force: a row of
    // buttons that do nothing teaches the operator to stop reading them.
    if row.chord.is_some() {
        actions = actions.child(
            components::ghost_button(
                motion::MotionKey::new("settings-key-clear", index),
                "Clear",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, cx| {
                        state.clear_binding(id);
                        state.persist_settings_later(cx);
                    });
                    cx.notify();
                }),
            ),
        );
    }
    if row.overridden {
        actions = actions.child(
            components::ghost_button(
                motion::MotionKey::new("settings-key-revert", index),
                "Revert",
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, cx| {
                        state.revert_binding(id);
                        state.persist_settings_later(cx);
                    });
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
            if armed {
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
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child(keys::binding_label(id)),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme.text_faint)
                        .child(row.binding.route.label()),
                ),
        )
        .child(actions)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    // A second click on the armed row is how the operator backs
                    // out with the pointer they armed it with.
                    if state.rebinding() == Some(id) {
                        state.cancel_rebind();
                    } else {
                        state.begin_rebind(id);
                    }
                });
                cx.notify();
            }),
        )
}

/// Put every binding back to the chord it shipped with.
fn reset_row(theme: &Theme, cx: &mut gpui::Context<crate::shell::Shell>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme.text_muted)
                .child("Click a row, then press the key you want"),
        )
        .child(
            components::ghost_button("settings-keys-reset", "Reset all", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, cx| {
                        state.reset_bindings();
                        state.persist_settings_later(cx);
                    });
                    cx.notify();
                }),
            ),
        )
}
