//! Inspector parameter rows for effect instances (v2 parameter kinds, collapsible
//! groups, visibility conditions, display hints, soft ranges, units, and tooltips).

#[path = "effects_params_transition.rs"]
pub(crate) mod transition;

use crate::tooltip::WithTip as _;
use aria_models::{
    ClipId, ClipParam, EffectCurvePoint, EffectGradientStop, EffectParamDisplay, EffectParamKind,
    EffectParamSpec,
};
use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div,
};

use crate::app::AppState;
use crate::app::clip_params::{ColorRow, ParamRow};
use crate::app::key_strip::KeyStrip;
use crate::components;
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::widgets::{
    Chip, Columns, Keys, MIXED, Readout, Stopwatch, Watching, chip_row, color_stopwatch,
    control_line, stepper_row, swatch_row,
};
use crate::surfaces::effects_rack::{self, Gesture, SlotEdit};
use crate::theme::Theme;
use crate::types::{UiEffectInstance, UiSlots as _};
use gpui::prelude::FluentBuilder;

/// Build all visible parameter rows (and collapsible group sections) for one effect instance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn effect_param_rows(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    index: usize,
    effect: &UiEffectInstance,
    sequence_clips: &[(ClipId, SharedString)],
    font_families: &[String],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let Some(spec) = aria_models::effect_spec(&effect.id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let known_groups: Vec<&str> = spec.groups.iter().map(|g| g.id.as_str()).collect();
    let keys = EffectKeys::read(state.read(cx), index);

    for (slot, pspec) in spec.params.iter().enumerate() {
        if !effect.is_slot_visible(slot) {
            continue;
        }
        let in_known_group = pspec
            .group
            .as_deref()
            .is_some_and(|gid| known_groups.contains(&gid));
        if in_known_group {
            continue;
        }
        push_effect_slot_rows(
            &mut out,
            theme,
            state,
            &keys,
            index,
            slot,
            pspec,
            effect,
            sequence_clips,
            font_families,
            cx,
        );
    }

    for group in &spec.groups {
        let visible_slots: Vec<(usize, &EffectParamSpec)> = spec
            .params
            .iter()
            .enumerate()
            .filter(|(slot, pspec)| {
                pspec.group.as_deref() == Some(group.id.as_str()) && effect.is_slot_visible(*slot)
            })
            .collect();
        if visible_slots.is_empty() {
            continue;
        }
        let collapsed = state
            .read(cx)
            .is_effect_group_collapsed(index, &group.id, group.collapsed);
        let group_name = group.id.clone();
        out.push(
            components::disclosure_button(
                MotionKey::new("fx-group", index).with(group.id.as_str()),
                group.label.clone(),
                !collapsed,
                theme,
                cx.reduce_motion(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    let group_name = group_name.clone();
                    this.state.update(cx, |state, _| {
                        if let Err(err) = effects_rack::apply(
                            state,
                            Gesture::ToggleGroup {
                                index,
                                group: group_name,
                            },
                        ) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element(),
        );
        if !collapsed {
            for (slot, pspec) in visible_slots {
                push_effect_slot_rows(
                    &mut out,
                    theme,
                    state,
                    &keys,
                    index,
                    slot,
                    pspec,
                    effect,
                    sequence_clips,
                    font_families,
                    cx,
                );
            }
        }
    }

    out
}

/// What the rows of one effect read off the state once a frame: each animatable
/// slot's value at the playhead with its keyframe state and time area, and the
/// driver draft its stopwatches show. A slot no row is offered for reads the
/// projection and draws no stopwatch.
struct EffectKeys {
    rows: Vec<(ParamRow, Option<KeyStrip>)>,
    colors: Vec<(ColorRow, Option<KeyStrip>)>,
    watching: Watching,
    /// The slots no curve carries that the other selected clips set
    /// differently.
    mixed_slots: Vec<usize>,
}

impl EffectKeys {
    fn read(state: &AppState, index: usize) -> Self {
        Self {
            rows: state
                .effect_param_rows(index)
                .into_iter()
                .map(|row| {
                    let strip = state.key_strip(row.param);
                    (row, strip)
                })
                .collect(),
            colors: state
                .effect_param_colors(index)
                .into_iter()
                .map(|row| {
                    let strip = state.key_strip(row.param);
                    (row, strip)
                })
                .collect(),
            watching: Watching::read(state),
            mixed_slots: state.effect_slots_mixed(index),
        }
    }

    /// Whether the other selected clips read component `component` of slot
    /// `slot` differently at the playhead, so its row prints a dash.
    fn mixed(&self, index: usize, slot: usize, component: Option<u8>) -> bool {
        let param = slot_param(index, slot);
        self.rows
            .iter()
            .map(|(row, _)| row)
            .find(|row| row.param == param && row.component == component)
            .map(|row| row.mixed)
            .or_else(|| {
                self.colors
                    .iter()
                    .find(|(row, _)| row.param == param)
                    .map(|(row, _)| row.mixed)
            })
            .unwrap_or_else(|| self.mixed_slots.contains(&slot))
    }

    /// The value of component `component` of slot `slot` at the playhead, and
    /// the stopwatch its row draws.
    fn number(&self, index: usize, slot: usize, component: Option<u8>) -> Option<(f32, Stopwatch)> {
        let param = slot_param(index, slot);
        self.rows
            .iter()
            .find(|(row, _)| row.param == param && row.component == component)
            .map(|(row, strip)| (row.value, self.watching.stopwatch(row, strip.clone())))
    }

    /// The colour of slot `slot` at the playhead, and the stopwatch its row
    /// draws.
    fn color(&self, index: usize, slot: usize) -> Option<([u8; 4], Stopwatch)> {
        let param = slot_param(index, slot);
        self.colors
            .iter()
            .find(|(row, _)| row.param == param)
            .map(|(row, strip)| (row.value, color_stopwatch(row, strip.clone())))
    }
}

/// The columns of a number row: its stopwatch where the slot can be keyed,
/// the keyed rows' blanks where it cannot.
fn keys_of(keyed: Option<(f32, Stopwatch)>) -> Keys {
    keyed.map_or(Keys::Unkeyed(Columns::Keyed), |(_, watch)| {
        Keys::Watch(watch)
    })
}

/// The property address of slot `slot` on effect `index`.
fn slot_param(index: usize, slot: usize) -> ClipParam {
    ClipParam::Effect {
        effect: index as u32,
        param: slot as u32,
    }
}

/// The axes of a two-component slot: width and height for a size, x and y
/// for any other.
pub(crate) fn vec2_axes(kind: EffectParamKind) -> [(usize, &'static str); 2] {
    if kind == EffectParamKind::Size {
        [(0, "w"), (1, "h")]
    } else {
        [(0, "x"), (1, "y")]
    }
}

/// A chip that writes `edit` to slot `slot` of effect `index`.
fn slot_chip(
    theme: &Theme,
    key: MotionKey,
    text: impl Into<SharedString>,
    (index, slot): (usize, usize),
    edit: SlotEdit,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    components::ghost_button(key, text, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let edit = edit.clone();
                this.state.update(cx, |state, _| {
                    if let Err(err) =
                        effects_rack::apply(state, Gesture::EditSlot { index, slot, edit })
                    {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

pub(super) fn scalar_readout(value: f32, pspec: &EffectParamSpec) -> Readout {
    let min = pspec.soft_min.unwrap_or(pspec.min);
    let max = pspec.soft_max.unwrap_or(pspec.max);
    let unit = ((max - min) / 100.0).max(0.01);
    if pspec.kind == EffectParamKind::Angle || pspec.display == Some(EffectParamDisplay::AngleDial)
    {
        Readout::printed(format!("{value:.1}°"), value, 1, unit)
    } else if pspec.display == Some(EffectParamDisplay::Percentage) {
        Readout::printed(format!("{:.0}%", value * 100.0), value, 2, unit)
    } else if let Some(u) = pspec.unit.as_deref().filter(|u| !u.is_empty()) {
        Readout::printed(format!("{value:.2} {u}"), value, 2, unit)
    } else {
        Readout::number(value, 2, unit)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_effect_slot_rows(
    out: &mut Vec<AnyElement>,
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    keys: &EffectKeys,
    index: usize,
    slot: usize,
    pspec: &EffectParamSpec,
    effect: &UiEffectInstance,
    sequence_clips: &[(ClipId, SharedString)],
    font_families: &[String],
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let tip = pspec.tooltip.clone();
    match pspec.kind {
        // Every number reads at the playhead and keys where an edit lands when
        // the slot is animated, so the value shown is the value written.
        EffectParamKind::Scalar | EffectParamKind::Angle => {
            let keyed = keys.number(index, slot, None);
            let value = keyed.as_ref().map_or_else(
                || effect.scalar(slot).unwrap_or(pspec.default),
                |(value, _)| *value,
            );
            let readout = scalar_readout(value, pspec).dashed(keys.mixed(index, slot, None));
            out.push(
                div()
                    .id(MotionKey::new("fx-param-tip", index)
                        .with(slot)
                        .element_id("fx-param-tip"))
                    .when_some(tip, |el, tip| {
                        el.with_tip(crate::tooltip::text_builder(tip))
                    })
                    .child(stepper_row(
                        theme,
                        state,
                        MotionKey::new(&pspec.name, index),
                        pspec.label.as_str(),
                        readout,
                        None,
                        keys_of(keyed),
                        move |state, by| {
                            effects_rack::apply(state, Gesture::NudgeScalar { index, slot, by })
                        },
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        EffectParamKind::Int => {
            let keyed = keys.number(index, slot, None);
            let value = keyed.as_ref().map_or_else(
                || effect.int(slot).unwrap_or(pspec.default_int as i32),
                |(value, _)| value.round() as i32,
            );
            out.push(
                div()
                    .id(MotionKey::new("fx-param-int", index)
                        .with(slot)
                        .element_id("fx-param-int"))
                    .when_some(tip, |el, tip| {
                        el.with_tip(crate::tooltip::text_builder(tip))
                    })
                    .child(stepper_row(
                        theme,
                        state,
                        MotionKey::new(&pspec.name, index),
                        pspec.label.as_str(),
                        Readout::number(value as f32, 0, 1.0).dashed(keys.mixed(index, slot, None)),
                        None,
                        keys_of(keyed),
                        move |state, by| {
                            effects_rack::apply(
                                state,
                                Gesture::EditSlot {
                                    index,
                                    slot,
                                    edit: SlotEdit::NudgeInt(by.round() as i32),
                                },
                            )
                        },
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        EffectParamKind::Bool => {
            let value = effect.boolean(slot).unwrap_or(pspec.default_bool);
            let shown = if keys.mixed(index, slot, None) {
                MIXED
            } else if value {
                "On"
            } else {
                "Off"
            };
            out.push(
                control_line(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    slot_chip(
                        theme,
                        MotionKey::new("fx-param-bool", index).with(slot),
                        shown,
                        (index, slot),
                        SlotEdit::ToggleBool,
                        cx,
                    ),
                )
                .id(MotionKey::new("fx-param-bool-row", index)
                    .with(slot)
                    .element_id("fx-param-bool-row"))
                .when_some(tip, |el, tip| {
                    el.with_tip(crate::tooltip::text_builder(tip))
                })
                .into_any_element(),
            );
        }
        EffectParamKind::Choice => {
            let current = effect
                .choice(slot)
                .unwrap_or_else(|| pspec.default_choice_id().unwrap_or_default());
            let mixed = keys.mixed(index, slot, None);
            let chips: Vec<Chip> = pspec
                .choices
                .iter()
                .map(|opt| {
                    Chip::choice(opt.label.clone(), opt.id == current && !mixed, |text| {
                        slot_chip(
                            theme,
                            MotionKey::new("fx-choice", index)
                                .with(slot)
                                .with(opt.id.as_str()),
                            text,
                            (index, slot),
                            SlotEdit::Choice(opt.id.clone()),
                            cx,
                        )
                    })
                })
                .collect();
            let shown = if mixed {
                MIXED
            } else {
                pspec
                    .choices
                    .iter()
                    .find(|opt| opt.id == current)
                    .map_or(current, |opt| opt.label.as_str())
            };
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    chips,
                )
                .id(MotionKey::new("fx-choice-row", index)
                    .with(slot)
                    .element_id("fx-choice-row"))
                .when_some(tip, |el, tip| {
                    el.with_tip(crate::tooltip::text_builder(tip))
                })
                .into_any_element(),
            );
        }
        EffectParamKind::Color => {
            let (value, watch) = match keys.color(index, slot) {
                Some((value, watch)) => (value, Keys::Watch(watch)),
                None => (
                    effect
                        .color(slot)
                        .unwrap_or_else(|| pspec.default_color_u8()),
                    Keys::Unkeyed(Columns::Keyed),
                ),
            };
            out.push(swatch_row(
                theme,
                MotionKey::new("fx-slot", index).with(slot),
                pspec.label.clone().into(),
                (!keys.mixed(index, slot, None)).then_some(value),
                watch,
                move |state, rgba| effects_rack::apply(state, Gesture::Color { index, slot, rgba }),
                cx,
            ));
        }
        EffectParamKind::Vec2 | EffectParamKind::Size => {
            let value = effect.vec2(slot).unwrap_or(pspec.default_vec2);
            let unit = ((pspec.max - pspec.min) / 100.0).max(0.001);
            for (axis, name) in vec2_axes(pspec.kind) {
                let keyed = keys.number(index, slot, Some(axis as u8));
                let shown = keyed.as_ref().map_or(value[axis], |(value, _)| *value);
                out.push(
                    stepper_row(
                        theme,
                        state,
                        MotionKey::new(&pspec.name, index).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(shown, 3, unit).dashed(keys.mixed(
                            index,
                            slot,
                            Some(axis as u8),
                        )),
                        None,
                        keys_of(keyed),
                        move |state, by| {
                            effects_rack::apply(
                                state,
                                Gesture::NudgeVec2 {
                                    index,
                                    slot,
                                    axis,
                                    by,
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
            let value = effect.point3(slot).unwrap_or(pspec.default_vec3);
            let unit = ((pspec.max - pspec.min) / 100.0).max(0.001);
            for (axis, name) in [(0usize, "x"), (1usize, "y"), (2usize, "z")] {
                let keyed = keys.number(index, slot, Some(axis as u8));
                let shown = keyed.as_ref().map_or(value[axis], |(value, _)| *value);
                out.push(
                    stepper_row(
                        theme,
                        state,
                        MotionKey::new(&pspec.name, index).with(name),
                        &format!("{} {name}", pspec.label),
                        Readout::number(shown, 3, unit).dashed(keys.mixed(
                            index,
                            slot,
                            Some(axis as u8),
                        )),
                        None,
                        keys_of(keyed),
                        move |state, by| {
                            effects_rack::apply(
                                state,
                                Gesture::EditSlot {
                                    index,
                                    slot,
                                    edit: SlotEdit::NudgePoint3 { axis, by },
                                },
                            )
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        EffectParamKind::Curve => {
            let count = effect
                .curve(slot)
                .map_or(pspec.default_curve.len(), <[EffectCurvePoint]>::len);
            let presets = [
                (
                    "Linear",
                    vec![
                        EffectCurvePoint::new(0.0, 0.0),
                        EffectCurvePoint::new(1.0, 1.0),
                    ],
                ),
                (
                    "S-Curve",
                    vec![
                        EffectCurvePoint::new(0.0, 0.0),
                        EffectCurvePoint::new(0.25, 0.15),
                        EffectCurvePoint::new(0.75, 0.85),
                        EffectCurvePoint::new(1.0, 1.0),
                    ],
                ),
                (
                    "Invert",
                    vec![
                        EffectCurvePoint::new(0.0, 1.0),
                        EffectCurvePoint::new(1.0, 0.0),
                    ],
                ),
            ];
            let chips: Vec<Chip> = presets
                .into_iter()
                .map(|(preset_name, pts)| {
                    Chip::other(slot_chip(
                        theme,
                        MotionKey::new("fx-curve", index)
                            .with(slot)
                            .with(preset_name),
                        preset_name,
                        (index, slot),
                        SlotEdit::Curve(pts),
                        cx,
                    ))
                })
                .collect();
            let shown = if keys.mixed(index, slot, None) {
                SharedString::new_static(MIXED)
            } else {
                format!("{count} pts").into()
            };
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    chips,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Gradient => {
            let count = effect
                .gradient(slot)
                .map_or(pspec.default_gradient.len(), <[EffectGradientStop]>::len);
            let presets = [
                (
                    "B&W",
                    vec![
                        EffectGradientStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
                        EffectGradientStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
                    ],
                ),
                (
                    "Warm",
                    vec![
                        EffectGradientStop::new(0.0, [0.1, 0.02, 0.0, 1.0]),
                        EffectGradientStop::new(1.0, [1.0, 0.85, 0.4, 1.0]),
                    ],
                ),
            ];
            let chips: Vec<Chip> = presets
                .into_iter()
                .map(|(preset_name, stops)| {
                    Chip::other(slot_chip(
                        theme,
                        MotionKey::new("fx-grad", index)
                            .with(slot)
                            .with(preset_name),
                        preset_name,
                        (index, slot),
                        SlotEdit::Gradient(stops),
                        cx,
                    ))
                })
                .collect();
            let shown = if keys.mixed(index, slot, None) {
                SharedString::new_static(MIXED)
            } else {
                format!("{count} stops").into()
            };
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    chips,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Text | EffectParamKind::File => {
            let current = effect.text(slot).unwrap_or(pspec.default_text.as_str());
            let shown = if keys.mixed(index, slot, None) {
                MIXED
            } else {
                current
            };
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    [Chip::other(slot_chip(
                        theme,
                        MotionKey::new("fx-text-clear", index).with(slot),
                        "Clear",
                        (index, slot),
                        SlotEdit::Text(String::new()),
                        cx,
                    ))],
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Font => {
            let current = effect.text(slot).unwrap_or(pspec.default_text.as_str());
            let mixed = keys.mixed(index, slot, None);
            let chips: Vec<Chip> = font_families
                .iter()
                .take(4)
                .map(|family| {
                    Chip::choice(family.clone(), family == current && !mixed, |label| {
                        slot_chip(
                            theme,
                            MotionKey::new("fx-font", index)
                                .with(slot)
                                .with(family.as_str()),
                            label,
                            (index, slot),
                            SlotEdit::Text(family.clone()),
                            cx,
                        )
                    })
                })
                .collect();
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    if mixed { MIXED } else { current },
                    chips,
                )
                .into_any_element(),
            );
        }
        EffectParamKind::Layer => {
            // Clips that point at different layers mark no chip.
            let current = (!keys.mixed(index, slot, None)).then(|| effect.layer(slot).flatten());
            let shown = match current {
                None => SharedString::new_static(MIXED),
                Some(None) => SharedString::new_static("None"),
                Some(Some(raw)) => sequence_clips
                    .iter()
                    .find(|(clip, _)| clip.raw() == raw)
                    .map_or_else(
                        || SharedString::new_static("Missing clip"),
                        |(_, name)| name.clone(),
                    ),
            };
            let mut chips: Vec<Chip> = Vec::with_capacity(sequence_clips.len() + 2);
            chips.push(Chip::choice("None", current == Some(None), |label| {
                slot_chip(
                    theme,
                    MotionKey::new("fx-layer-none", index).with(slot),
                    label,
                    (index, slot),
                    SlotEdit::Layer(None),
                    cx,
                )
            }));
            for (target_clip, name) in sequence_clips {
                let target = *target_clip;
                let on = current == Some(Some(target.raw()));
                chips.push(Chip::choice(name.clone(), on, |label| {
                    slot_chip(
                        theme,
                        MotionKey::new("fx-layer", index)
                            .with(slot)
                            .with(target.raw()),
                        label,
                        (index, slot),
                        SlotEdit::Layer(Some(target)),
                        cx,
                    )
                }));
            }
            // A layer the render refuses says why under the chips.
            if let Some(message) = state.read(cx).effect_layer_refusal(index, slot) {
                chips.push(Chip::other(
                    div()
                        .w_full()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.danger)
                        .child(message),
                ));
            }
            out.push(
                chip_row(
                    theme,
                    MotionKey::new(&pspec.name, index),
                    pspec.label.as_str(),
                    Columns::Keyed,
                    shown,
                    chips,
                )
                .into_any_element(),
            );
        }
    }
}
