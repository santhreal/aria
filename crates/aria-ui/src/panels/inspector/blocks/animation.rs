//! Animation block of the Look tab: the In, Out, and Combo slots.
//!
//! Presets come from `AppState::animation_choices`, which filters the catalog by
//! slot and hides text-only presets on non-text clips, so a chip can never offer
//! a preset the command layer refuses. Knob rows are hidden when the applied
//! preset does not expose that knob.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, px,
};

use aria_models::AnimationSlot;

use crate::app::clip_anim::{ANIM_KNOBS, SLOTS, anim_knob, anim_knob_available};
use crate::components;
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{Chip, Columns, Readout, chip_row, unkeyed_row};

use crate::components::section_label;

/// The label of `slot`'s row.
pub(crate) fn slot_label(slot: AnimationSlot) -> &'static str {
    match slot {
        AnimationSlot::In => "In",
        AnimationSlot::Out => "Out",
        AnimationSlot::Combo => "Loop",
    }
}

fn clear_id(slot: AnimationSlot) -> &'static str {
    match slot {
        AnimationSlot::In => "anim-clear-in",
        AnimationSlot::Out => "anim-clear-out",
        AnimationSlot::Combo => "anim-clear-combo",
    }
}

pub fn animation_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let slots: Vec<_> = SLOTS
        .iter()
        .map(|slot| {
            let slot = *slot;
            let applied = match slot {
                AnimationSlot::In => clip.animation_in.clone(),
                AnimationSlot::Out => clip.animation_out.clone(),
                AnimationSlot::Combo => clip.animation_combo.clone(),
            };
            let choices = state.read(cx).animation_choices(slot);
            // The applied animation's name as its chip prints it.
            let mut current = applied.as_ref().map_or_else(
                || SharedString::new_static("None"),
                |anim| SharedString::new_static(anim.label),
            );
            let mut chips: Vec<_> = choices
                .into_iter()
                .map(|choice| {
                    let id = choice.id;
                    let on = applied.as_ref().is_some_and(|anim| anim.id == id);
                    // An id with no sampler is still offered — the project may
                    // already reference it — but it is named for what it is.
                    let label = if choice.rendered {
                        SharedString::new_static(choice.label)
                    } else {
                        SharedString::from(format!("{} (no renderer)", choice.label))
                    };
                    if on {
                        current = label.clone();
                    }
                    Chip::choice(label, on, |label| {
                        components::ghost_button(("anim-preset", id), label, theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.state.update(cx, |state, _| {
                                        if let Err(err) =
                                            clip_pick::apply(state, Pick::Animation(id))
                                        {
                                            state.report_error(&err);
                                        }
                                    });
                                    cx.notify();
                                }),
                            )
                            .into_any_element()
                    })
                })
                .collect();
            if applied.is_some() {
                chips.push(Chip::other(
                    components::ghost_button(("anim-clear", clear_id(slot)), "Clear", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| {
                                    if let Err(err) =
                                        clip_pick::apply(state, Pick::ClearAnimation(slot))
                                    {
                                        state.report_error(&err);
                                    }
                                });
                                cx.notify();
                            }),
                        ),
                ));
            }

            // Each slot's knobs are keys of their own: In's speed and Out's
            // speed are two rows, and a drag on one is no drag on the other.
            let knobs: Vec<_> = applied
                .as_ref()
                .map(|anim| {
                    ANIM_KNOBS
                        .iter()
                        .filter(|(id, ..)| anim_knob_available(anim, id))
                        .map(|(id, label, ..)| {
                            let knob = *id;
                            let value = anim_knob(anim, knob);
                            unkeyed_row(
                                theme,
                                &state,
                                MotionKey::new("insp.row", knob).with(clear_id(slot)),
                                *label,
                                Readout::number(value, 2, 0.01),
                                Columns::Keyed,
                                move |state, by| {
                                    clip_pick::apply(state, Pick::AnimationKnob { slot, knob, by })
                                },
                                cx,
                            )
                            .into_any_element()
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(chip_row(
                    theme,
                    MotionKey::new("insp.row", clear_id(slot)),
                    slot_label(slot),
                    Columns::Keyed,
                    current,
                    chips,
                ))
                .children(knobs)
                .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Animation"))
        .children(slots)
}
