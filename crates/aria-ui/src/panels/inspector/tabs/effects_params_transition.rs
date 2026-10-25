//! Inspector parameter rows for the selected clip's transition.

use aria_models::{EffectParamKind, EffectParamSpec, TransitionSpec};
use gpui::{AnyElement, IntoElement, SharedString};

use super::{scalar_readout, vec2_axes};
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::widgets::{
    Chip, Columns, Keys, Readout, chip_row, control_line, pick_button, swatch_row, unkeyed_row,
};
use crate::surfaces::clip_pick::{self, Pick, TransitionSlotEdit};
use crate::theme::Theme;
use crate::types::{UiSlots as _, UiTransition};

/// Build all visible parameter rows for the selected clip's transition.
pub(crate) fn transition_param_rows(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    transition: &UiTransition,
    spec: &TransitionSpec,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (slot, pspec) in spec.params.iter().enumerate() {
        if !transition.is_slot_visible(slot) {
            continue;
        }
        push_transition_slot_rows(&mut out, theme, state, slot, pspec, transition, cx);
    }
    out
}

/// A chip that writes `edit` to transition slot `slot`.
fn transition_chip(
    theme: &Theme,
    key: MotionKey,
    text: impl Into<SharedString>,
    slot: usize,
    edit: TransitionSlotEdit,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    pick_button(key, text, theme, cx, move |state| {
        let edit = edit.clone();
        clip_pick::apply(state, Pick::TransitionParam { slot, edit })
    })
    .into_any_element()
}

fn push_transition_slot_rows(
    out: &mut Vec<AnyElement>,
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    slot: usize,
    pspec: &EffectParamSpec,
    transition: &UiTransition,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    match pspec.kind {
        EffectParamKind::Scalar | EffectParamKind::Angle => {
            let value = transition.scalar(slot).unwrap_or(pspec.default);
            out.push(
                unkeyed_row(
                    theme,
                    state,
                    MotionKey::new("tr-param", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    scalar_readout(value, pspec),
                    Columns::Keyed,
                    move |state, by| {
                        clip_pick::apply(
                            state,
                            Pick::TransitionParam {
                                slot,
                                edit: TransitionSlotEdit::NudgeScalar(by),
                            },
                        )
                    },
                    cx,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Int => {
            let value = transition.int(slot).unwrap_or(pspec.default_int as i32);
            out.push(
                unkeyed_row(
                    theme,
                    state,
                    MotionKey::new("tr-param-int", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    Readout::number(value as f32, 0, 1.0),
                    Columns::Keyed,
                    move |state, by| {
                        clip_pick::apply(
                            state,
                            Pick::TransitionParam {
                                slot,
                                edit: TransitionSlotEdit::NudgeInt(by.round() as i32),
                            },
                        )
                    },
                    cx,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Bool => {
            let value = transition.boolean(slot).unwrap_or(pspec.default_bool);
            out.push(
                control_line(
                    theme,
                    MotionKey::new("tr-param", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    transition_chip(
                        theme,
                        MotionKey::new("tr-bool", slot),
                        if value { "On" } else { "Off" },
                        slot,
                        TransitionSlotEdit::ToggleBool,
                        cx,
                    ),
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Choice => {
            let current = transition
                .choice(slot)
                .unwrap_or_else(|| pspec.default_choice_id().unwrap_or_default());
            let chips: Vec<Chip> = pspec
                .choices
                .iter()
                .map(|opt| {
                    Chip::choice(opt.label.clone(), opt.id == current, |text| {
                        transition_chip(
                            theme,
                            MotionKey::new("tr-choice", slot).with(opt.id.as_str()),
                            text,
                            slot,
                            TransitionSlotEdit::Choice(opt.id.clone()),
                            cx,
                        )
                    })
                })
                .collect();
            let shown = pspec
                .choices
                .iter()
                .find(|opt| opt.id == current)
                .map_or(current, |opt| opt.label.as_str());
            out.push(
                chip_row(
                    theme,
                    MotionKey::new("tr-param", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    chips,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Color => {
            let value = transition
                .color(slot)
                .unwrap_or_else(|| pspec.default_color_u8());
            out.push(swatch_row(
                theme,
                MotionKey::new("tr-slot-col", slot),
                pspec.label.clone().into(),
                Some(value),
                Keys::Unkeyed(Columns::Keyed),
                move |state, rgba| {
                    clip_pick::apply(
                        state,
                        Pick::TransitionParam {
                            slot,
                            edit: TransitionSlotEdit::Color(rgba),
                        },
                    )
                },
                cx,
            ));
        }
        EffectParamKind::Vec2 | EffectParamKind::Size => {
            let value = transition.vec2(slot).unwrap_or(pspec.default_vec2);
            let unit = ((pspec.max - pspec.min) / 100.0).max(0.001);
            for (axis, name) in vec2_axes(pspec.kind) {
                out.push(
                    unkeyed_row(
                        theme,
                        state,
                        MotionKey::new("tr-vec2", slot).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(value[axis], 3, unit),
                        Columns::Keyed,
                        move |state, by| {
                            clip_pick::apply(
                                state,
                                Pick::TransitionParam {
                                    slot,
                                    edit: TransitionSlotEdit::NudgeVec2 { axis, by },
                                },
                            )
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        EffectParamKind::Point3 => {
            let value = transition.point3(slot).unwrap_or(pspec.default_vec3);
            let unit = ((pspec.max - pspec.min) / 100.0).max(0.001);
            for (axis, name) in [(0usize, "x"), (1usize, "y"), (2usize, "z")] {
                out.push(
                    unkeyed_row(
                        theme,
                        state,
                        MotionKey::new("tr-pt3", slot).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(value[axis], 3, unit),
                        Columns::Keyed,
                        move |state, by| {
                            clip_pick::apply(
                                state,
                                Pick::TransitionParam {
                                    slot,
                                    edit: TransitionSlotEdit::NudgePoint3 { axis, by },
                                },
                            )
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        EffectParamKind::Curve
        | EffectParamKind::Gradient
        | EffectParamKind::Text
        | EffectParamKind::Font
        | EffectParamKind::File
        | EffectParamKind::Layer => {}
    }
}
