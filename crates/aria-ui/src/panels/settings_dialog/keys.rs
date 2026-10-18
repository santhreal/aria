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
//!
//! The Settings filter searches the list: a filter that is not part of the
//! section's title lists the rows whose name or chord contains it. A chord
//! another binding holds is a question on the row, answered by Move or Keep, or
//! by Enter or Esc from the keyboard.

use gpui::{
    AppContext, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::keymaps::KeyMove;
use crate::components;
use crate::keys::BindingRow;
use crate::motion;
use crate::panels::sidebar::DragGhost;
use crate::theme::Theme;

use super::keyboard::KeyDrag;

/// The Keys section under the Settings filter, or nothing when the filter
/// matches neither the section's title nor any binding.
///
/// The sets and the keyboard image, the reset, every listed binding, then any
/// refusal. The image and the refusals are drawn only when the whole list is.
pub(super) fn section(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<Vec<gpui::AnyElement>> {
    let (listing, conflicts, armed, pending) = {
        let state = state.read(cx);
        let listing = state.listed_binding_rows()?;
        let conflicts: Vec<(String, String)> = if listing.whole {
            state
                .binding_conflicts()
                .into_iter()
                .map(|conflict| {
                    (
                        format!("{} was refused", state.binding_name(conflict.refused)),
                        format!(
                            "{} is held by {}",
                            conflict.chord.label(),
                            state.binding_name(conflict.held_by)
                        ),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let pending = state.pending_key_move().map(|pending| {
            (
                pending,
                SharedString::from(state.binding_name(pending.holder)),
            )
        });
        (listing, conflicts, state.rebinding(), pending)
    };
    let mut out: Vec<gpui::AnyElement> =
        Vec::with_capacity(listing.rows.len() + conflicts.len() + 4);
    if listing.whole {
        out.extend(super::keyboard::keymap_rows(theme, state, cx));
        let rows: Vec<BindingRow> = listing.rows.iter().map(|(row, _)| *row).collect();
        out.push(super::keyboard::keyboard(theme, state, &rows, cx));
    }
    out.push(reset_row(theme, cx).into_any_element());
    for (index, (row, label)) in listing.rows.into_iter().enumerate() {
        let is_armed = armed == Some(row.binding.id);
        let asked = pending
            .as_ref()
            .filter(|(pending, _)| pending.id == row.binding.id)
            .cloned();
        out.push(binding_row(theme, row, label, is_armed, asked, index, cx));
    }
    for (refused, held) in conflicts {
        out.push(super::widgets::kv(theme, &refused, held).into_any_element());
    }
    Some(out)
}

/// One binding: what it does, what it answers to, and how to change it.
fn binding_row(
    theme: &Theme,
    row: BindingRow,
    label: String,
    armed: bool,
    asked: Option<(KeyMove, SharedString)>,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = row.binding.id;
    let label = SharedString::from(label);
    let element_id = gpui::ElementId::from(("settings-key", index));
    if let Some((pending, holder)) = asked {
        let question = motion::ControlId::Key(motion::MotionKey::new("settings-key", index));
        return question_row(theme, pending, holder, label, question, cx).into_any_element();
    }
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
                .text_size(Theme::TEXT_UI)
                .text_color(tone)
                .child(shown),
        );
    // Clear is drawn only where there is a chord to clear, and Revert only where
    // the chord in force is not the shipped one: no button on a row does nothing.
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
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_faint)
                        .child(row.binding.route.label()),
                ),
        )
        .child(actions)
        // Dropped on a key of the keyboard image, the row binds there.
        .on_drag(KeyDrag { id, label }, |drag, _offset, _window, cx| {
            let label = drag.label.clone();
            cx.new(|_| DragGhost { label })
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    // A second click on the armed row disarms it, with the
                    // pointer that armed it.
                    if state.rebinding() == Some(id) {
                        state.cancel_rebind();
                    } else {
                        state.begin_rebind(id);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

/// A row whose chord another binding holds: what holds it, and Move or Keep.
fn question_row(
    theme: &Theme,
    pending: KeyMove,
    holder: SharedString,
    label: SharedString,
    id: motion::ControlId,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id(id.element("settings-key-question"))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.glass_selected_bg())
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.warning)
                        .child(format!("{} is held by {holder}", pending.chord.label())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::button("settings-key-move", "Move", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |state, cx| {
                                state.move_key();
                                state.persist_settings_later(cx);
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button("settings-key-keep", "Keep", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |state, _| state.keep_key());
                            cx.notify();
                        }),
                    ),
                ),
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
                .text_size(Theme::TEXT_UI)
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
