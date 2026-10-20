//! Inspector parameter rows for the selected clip's transition.

use aria_models::{EffectParamKind, EffectParamSpec, TransitionSpec};
use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Styled, div, px,
};

use super::{param_line, scalar_readout};
use crate::components;
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::widgets::{Readout, audio_row, color_name, swatch};
use crate::surfaces::clip_pick::{self, Pick, TransitionSlotEdit};
use crate::theme::Theme;
use crate::types::UiTransition;

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
            let readout = scalar_readout(value, pspec);
            out.push(
                audio_row(
                    theme,
                    state,
                    MotionKey::new("tr-param", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    readout,
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
                audio_row(
                    theme,
                    state,
                    MotionKey::new("tr-param-int", slot).with(pspec.name.as_str()),
                    pspec.label.as_str(),
                    Readout::number(value as f32, 0, 1.0),
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
            let value = transition.bool_val(slot).unwrap_or(pspec.default_bool);
            let btn_label = if value { "✓ On" } else { "○ Off" };
            out.push(
                param_line(theme, pspec.label.clone())
                    .child(
                        components::ghost_button(MotionKey::new("tr-bool", slot), btn_label, theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.state.update(cx, |state, _| {
                                        if let Err(err) = clip_pick::apply(
                                            state,
                                            Pick::TransitionParam {
                                                slot,
                                                edit: TransitionSlotEdit::ToggleBool,
                                            },
                                        ) {
                                            state.report_error(&err);
                                        }
                                    });
                                    cx.notify();
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        }
        EffectParamKind::Choice => {
            let current = transition
                .choice(slot)
                .unwrap_or_else(|| pspec.default_choice_id().unwrap_or_default());
            let chips: Vec<AnyElement> = pspec
                .choices
                .iter()
                .map(|opt| {
                    let opt_val = opt.id.clone();
                    let text = if opt.id == current {
                        format!("• {}", opt.label)
                    } else {
                        opt.label.clone()
                    };
                    components::ghost_button(
                        MotionKey::new("tr-choice", slot).with(opt.id.as_str()),
                        text,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            let choice = opt_val.clone();
                            this.state.update(cx, |state, _| {
                                if let Err(err) = clip_pick::apply(
                                    state,
                                    Pick::TransitionParam {
                                        slot,
                                        edit: TransitionSlotEdit::Choice(choice),
                                    },
                                ) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                })
                .collect();
            out.push(
                param_line(theme, pspec.label.clone())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(Theme::SPACE_XS))
                            .children(chips),
                    )
                    .into_any_element(),
            );
        }
        EffectParamKind::Color => {
            let value = transition
                .color(slot)
                .unwrap_or_else(|| pspec.default_color_u8());
            let swatches: Vec<_> = crate::app::clip_effects::PARAM_COLORS
                .iter()
                .map(|(name, _, rgba)| {
                    let rgba = *rgba;
                    let picked = rgba == value;
                    div()
                        .id(MotionKey::new("tr-slot-col", slot)
                            .with(name)
                            .element_id("tr-slot-col"))
                        .w(px(16.))
                        .h(px(16.))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .bg(swatch(rgba))
                        .border_1()
                        .border_color(if picked { theme.accent } else { theme.border })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| {
                                    if let Err(err) = clip_pick::apply(
                                        state,
                                        Pick::TransitionParam {
                                            slot,
                                            edit: TransitionSlotEdit::Color(rgba),
                                        },
                                    ) {
                                        state.report_error(&err);
                                    }
                                });
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                })
                .collect();
            out.push(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(Theme::SPACE_XS))
                    .py(px(Theme::SPACE_XS))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(Theme::SPACE_XS))
                            .items_center()
                            .child(
                                div()
                                    .text_size(Theme::TEXT_UI)
                                    .text_color(theme.text_muted)
                                    .child(pspec.label.clone()),
                            )
                            .child(
                                div()
                                    .text_size(Theme::TEXT_UI)
                                    .text_color(theme.text_faint)
                                    .child(color_name(value)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(Theme::SPACE_XS))
                            .children(swatches),
                    )
                    .into_any_element(),
            );
        }
        EffectParamKind::Vec2 | EffectParamKind::Size => {
            let value = transition.vec2(slot).unwrap_or(pspec.default_vec2);
            let unit = ((pspec.max - pspec.min) / 100.0).max(0.001);
            for (axis, name) in [(0usize, "x"), (1usize, "y")] {
                out.push(
                    audio_row(
                        theme,
                        state,
                        MotionKey::new("tr-vec2", slot).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(value[axis], 3, unit),
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
                    audio_row(
                        theme,
                        state,
                        MotionKey::new("tr-pt3", slot).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(value[axis], 3, unit),
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
