//! The top of the Keys section: the named sets, and the keyboard image.
//!
//! The image is drawn from [`crate::keys::keyboard_image`], with each key
//! coloured by the kinds of command it holds under the modifiers shown. A
//! binding row dragged onto a key binds it there; a key clicked while a row is
//! armed is the same answer as pressing it. A key another binding holds asks
//! before it moves, through the same question a pressed key asks.

use crate::tooltip::WithTip as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::settings_form::SettingsField;
use crate::components;
use crate::keys::BindingRow;
use crate::keys::keyboard_image::{self, Cap, CapKind, CommandKind, Held, Mods};
use crate::motion;
use crate::theme::Theme;

/// One key unit on the image, in pixels. Fifteen of them fit the dialog card.
const UNIT: f32 = 27.0;

/// The height of a cap.
const CAP_HEIGHT: f32 = 26.0;

/// Space between two caps.
const CAP_GAP: f32 = 2.0;

/// A binding row in flight to a key of the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeyDrag {
    pub(super) id: &'static str,
    pub(super) label: SharedString,
}

/// The colour a kind of command gives its key.
fn kind_colour(theme: &Theme, kind: CommandKind) -> gpui::Hsla {
    match kind {
        CommandKind::Editor => theme.accent,
        CommandKind::Timeline => theme.track_video,
        CommandKind::Viewer => theme.track_audio,
        CommandKind::Home => theme.track_generator,
        CommandKind::Plugin => theme.warning,
    }
}

/// The set chips with Import, Export and Delete, then the Save as field.
pub(super) fn keymap_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let (sets, active) = {
        let state = state.read(cx);
        (
            state.keymap_sets(),
            state.active_keymap().map(SharedString::new),
        )
    };
    let mut chips = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .w(px(140.0))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child("Keymap"),
        );
    if sets.is_empty() {
        chips = chips.child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("No saved sets"),
        );
    }
    for (index, name) in sets.iter().enumerate() {
        let id = motion::MotionKey::new("settings-keymap", index);
        let chip = if active.as_ref() == Some(name) {
            components::button(id, name.clone(), theme)
        } else {
            components::ghost_button(id, name.clone(), theme)
        };
        let picked = name.clone();
        chips = chips.child(chip.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, cx| match state.load_keymap(&picked) {
                        Ok(()) => state.persist_settings_later(cx),
                        Err(err) => state.report_error(err),
                    });
                cx.notify();
            }),
        ));
    }
    chips = chips
        .child(
            components::ghost_button("settings-keymap-import", "Import…", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.import_keymap());
                    cx.notify();
                }),
            ),
        )
        .child(
            components::ghost_button("settings-keymap-export", "Export…", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.export_keymap());
                    cx.notify();
                }),
            ),
        );
    if let Some(name) = active.filter(|name| sets.contains(name)) {
        chips = chips.child(
            components::ghost_button("settings-keymap-delete", "Delete set", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, cx| match state.delete_keymap(&name) {
                            Ok(()) => state.persist_settings_later(cx),
                            Err(err) => state.report_error(err),
                        });
                    cx.notify();
                }),
            ),
        );
    }
    vec![
        chips.into_any_element(),
        super::ai::field_row(theme, state, SettingsField::KeymapName, cx),
    ]
}

/// The modifiers shown, the image, and the legend.
pub(super) fn keyboard(
    theme: &Theme,
    state: &Entity<AppState>,
    rows: &[BindingRow],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let mods = state.read(cx).key_image_mods();
    let held = keyboard_image::held_by_key(rows, mods);
    let mut image = div().flex().flex_col().gap(px(CAP_GAP));
    let mut index = 0usize;
    for (row_index, row) in keyboard_image::rows().enumerate() {
        if row_index == keyboard_image::MAIN.len() {
            image = image.child(div().h(px(Theme::SPACE_XS)));
        }
        let mut line = div().flex().flex_row().gap(px(CAP_GAP));
        for cap in row {
            let holds = match cap.kind {
                CapKind::Key(name) => held.get(name).map(Vec::as_slice).unwrap_or_default(),
                _ => &[],
            };
            line = line.child(cap_element(theme, state, *cap, holds, mods, index, cx));
            index += 1;
        }
        image = image.child(line);
    }
    let mut legend = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(Theme::SPACE_SM));
    for kind in CommandKind::ALL {
        legend = legend.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .w(px(10.0))
                        .h(px(4.0))
                        .rounded(px(1.0))
                        .bg(kind_colour(theme, kind)),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_muted)
                        .child(kind.label()),
                ),
        );
    }
    legend = legend.child(
        div()
            .text_size(Theme::TEXT_SMALL)
            .text_color(theme.text_faint)
            .child("Drag a row onto a key to bind it"),
    );
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(image)
        .child(legend)
        .into_any_element()
}

/// One cap of the image.
fn cap_element(
    theme: &Theme,
    state: &Entity<AppState>,
    cap: Cap,
    holds: &[Held],
    mods: Mods,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let width = px(cap.width * UNIT - CAP_GAP);
    let base = div()
        .id(gpui::ElementId::from(("settings-kbd-cap", index)))
        .w(width)
        .h(px(CAP_HEIGHT))
        .flex_none()
        .rounded(px(Theme::SPACE_XS))
        .overflow_hidden();
    match cap.kind {
        CapKind::Gap => base.into_any_element(),
        CapKind::Inert => base
            .border_1()
            .border_color(theme.hairline)
            .flex()
            .items_center()
            .justify_center()
            .text_size(Theme::TEXT_MICRO)
            .text_color(theme.text_faint)
            .child(cap.label)
            .into_any_element(),
        CapKind::Modifier(modifier) => {
            let down = mods.held(modifier);
            base.border_1()
                .border_color(if down { theme.accent } else { theme.border })
                .bg(if down {
                    theme.accent
                } else {
                    theme.surface_raised
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(Theme::TEXT_MICRO)
                .text_color(if down {
                    theme.on_accent
                } else {
                    theme.text_muted
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .child(cap.label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.toggle_key_image_modifier(modifier));
                        cx.notify();
                    }),
                )
                .into_any_element()
        }
        CapKind::Key(name) => {
            let accent = theme.accent;
            let mut bands = div().flex().flex_row().h(px(3.0)).w_full();
            for kind in keyboard_image::kinds(holds) {
                bands = bands.child(div().flex_1().h_full().bg(kind_colour(theme, kind)));
            }
            let tip_state = state.clone();
            base.border_1()
                .border_color(if holds.is_empty() {
                    theme.hairline
                } else {
                    theme.border_strong
                })
                .bg(theme.surface_raised)
                .flex()
                .flex_col()
                .justify_between()
                .cursor(gpui::CursorStyle::PointingHand)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(Theme::TEXT_MICRO)
                        .text_color(if holds.is_empty() {
                            theme.text_faint
                        } else {
                            theme.text
                        })
                        .child(cap.label),
                )
                .child(bands)
                // Resolved when the pointer rests, not per frame.
                .with_tip(move |window, cx| {
                    let tip = tip_state.read(cx).key_cap_tip(name);
                    crate::tooltip::text_builder(tip)(window, cx)
                })
                .drag_over::<KeyDrag>(move |style, _, _, _| style.border_color(accent))
                .on_drop(cx.listener(move |this, drag: &KeyDrag, _, cx| {
                    this.state.update(cx, |state, cx| {
                        state.drop_binding_on_key(drag.id, name);
                        state.persist_settings_later(cx);
                    });
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            // A key clicked while a row waits for one is that
                            // row's answer, the same as pressing it.
                            if let Some(id) = state.rebinding() {
                                state.drop_binding_on_key(id, name);
                                state.persist_settings_later(cx);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element()
        }
    }
}
