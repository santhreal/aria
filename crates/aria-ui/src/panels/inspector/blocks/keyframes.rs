//! Keyframes block: one row per animated property of the selected clip, that
//! is, per property that holds a keyframe or a driver.
//!
//! Rows come from `AppState::key_params`, the projected property list, which
//! drops the properties this clip's lane rejects, so no row offers a command
//! the model would reject. Each row already holds its keys, so the block reads
//! how many the property has, which one is under the playhead and which curve
//! leaves it with no second model query per row. A constant property is keyed
//! from its own row's stopwatch or diamond, or from its timeline property row,
//! so it has no row here.

use gpui::prelude::FluentBuilder;
use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};
use crate::components::section_label;
use crate::motion::MotionKey;
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;

use super::widgets::{Columns, choice_chip, label_note, pick_button, under_labels};

pub fn keyframes_block(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let (params, playhead) = {
        let read = state.read(cx);
        (read.key_params(), read.playhead.value)
    };

    let rows: Vec<_> = params
        .iter()
        .filter_map(|row| {
            let param = row.param();
            let count = row.marks.len();
            let driven = state.read(cx).selected_driver(param).is_some();
            if count == 0 && !driven {
                return None;
            }
            let id = row.id.clone();
            let here = row.keyed_at(playhead);
            let preset_here = row.preset_at(playhead);

            let easing_chips = easing_presets().iter().map(|preset| {
                let preset_id = preset.id.as_str();
                choice_chip(
                    ("keyframe-easing", MotionKey::new(preset_id, &id)),
                    preset.label.as_str(),
                    preset_here == Some(preset_id),
                    theme,
                    cx,
                    move |state| {
                        keyframe_rows::apply(
                            state,
                            Row::Easing {
                                param,
                                preset: preset_id,
                            },
                        )
                    },
                )
                .into_any_element()
            });
            let piecewise_chips = PIECEWISE_PRESETS.iter().map(|(preset, label)| {
                let preset = *preset;
                pick_button(
                    ("keyframe-piecewise", MotionKey::new(label, &id)),
                    *label,
                    theme,
                    cx,
                    move |state| keyframe_rows::apply(state, Row::Piecewise { param, preset }),
                )
                .into_any_element()
            });
            let presets: Vec<_> = if here {
                easing_chips.chain(piecewise_chips).collect()
            } else {
                Vec::new()
            };

            let keyed = count > 0;
            let controls = [
                keyed.then(|| {
                    pick_button(
                        MotionKey::new("kf-prev", &id),
                        "‹",
                        theme,
                        cx,
                        move |state| keyframe_rows::apply(state, Row::Previous(param)),
                    )
                }),
                keyed.then(|| {
                    pick_button(
                        MotionKey::new("kf-next", &id),
                        "›",
                        theme,
                        cx,
                        move |state| keyframe_rows::apply(state, Row::Next(param)),
                    )
                }),
                Some(pick_button(
                    MotionKey::new("kf-toggle", &id),
                    if here { "◆" } else { "◇" },
                    theme,
                    cx,
                    move |state| keyframe_rows::apply(state, Row::Key { param, keyed: here }),
                )),
                Some(pick_button(
                    MotionKey::new("kf-driver", &id),
                    if driven { "ƒ Remove" } else { "ƒ Driver" },
                    theme,
                    cx,
                    move |state| keyframe_rows::apply(state, Row::Driver(param)),
                )),
                keyed.then(|| {
                    pick_button(
                        MotionKey::new("kf-flatten", &id),
                        "Flatten",
                        theme,
                        cx,
                        move |state| keyframe_rows::apply(state, Row::Flatten(param)),
                    )
                }),
            ]
            .into_iter()
            .flatten()
            .map(IntoElement::into_any_element);

            let name = format!(
                "{} · {}",
                row.label,
                aria_core::count::counted(count, "key", "keys")
            );
            Some(
                div()
                    .debug_selector(|| format!("kf-row-{id}"))
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .child(under_labels(
                        Columns::Keyed,
                        [div()
                            .debug_selector(|| format!("kf-label-{id}"))
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.text_muted)
                            .child(name)
                            .into_any_element()],
                    ))
                    .child(under_labels(Columns::Keyed, controls))
                    .when(!presets.is_empty(), |el| {
                        el.child(under_labels(Columns::Keyed, presets))
                    })
                    .into_any_element(),
            )
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(section_label(theme, "Keyframes"))
        .when(rows.is_empty(), |el| {
            el.child(label_note(Columns::Keyed, theme.text_faint, "None"))
        })
        .children(rows)
}
