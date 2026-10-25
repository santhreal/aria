//! Keyframes block: one row per animatable property of the selected clip.
//!
//! Rows come from `AppState::key_params`, the projected property list, which
//! drops the properties this clip's lane refuses, so a row can never offer a
//! command the model would reject. Each row already carries its keys, so the
//! strip reads how many the property has, which one sits under the playhead and
//! which curve leaves it without asking the model again per row.

use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::{Columns, ghosts_under_labels, pick_button, picked, under_labels};

use crate::components::section_label;

pub fn keyframes_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let _ = clip;
    let state = gpui::Entity::clone(state);
    let (params, playhead) = {
        let read = state.read(cx);
        (read.key_params(), read.playhead.value)
    };

    let rows: Vec<_> = params
        .iter()
        .map(|row| {
            let param = row.param();
            let id = row.id.clone();
            let label = row.label.clone();
            let count = row.marks.len();
            let here = row.keyed_at(playhead);
            let preset_here = row.preset_at(playhead);
            let driven = state.read(cx).selected_driver(param).is_some();

            let easing_chips: Vec<_> = easing_presets()
                .iter()
                .map(|preset| {
                    let preset_id = preset.id.as_str();
                    let selected = preset_here == Some(preset_id);
                    pick_button(
                        ("keyframe-easing", preset_id),
                        picked(preset.label.as_str(), selected),
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
                    .id(crate::motion::MotionKey::new(preset_id, &id).element_id("kf-preset"))
                    .into_any_element()
                })
                .collect();

            let piecewise_chips: Vec<_> = PIECEWISE_PRESETS
                .iter()
                .map(|(preset, label)| {
                    let preset = *preset;
                    pick_button(
                        (
                            "keyframe-piecewise",
                            crate::motion::MotionKey::new(label, &id),
                        ),
                        *label,
                        theme,
                        cx,
                        move |state| keyframe_rows::apply(state, Row::Piecewise { param, preset }),
                    )
                    .into_any_element()
                })
                .collect();

            div()
                .id(id.clone())
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(under_labels(
                    Columns::Keyed,
                    [div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text_faint)
                                .child(format!(
                                    "{label} · {}",
                                    aria_core::count::counted(count, "key", "keys")
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(Theme::SPACE_XS))
                                .when(count > 0, |el| {
                                    el.child(
                                        pick_button("kf-prev", "‹", theme, cx, move |state| {
                                            keyframe_rows::apply(state, Row::Previous(param))
                                        })
                                        .id(crate::motion::MotionKey::new("kf-prev", &id)
                                            .element_id("kf-prev")),
                                    )
                                    .child(
                                        pick_button("kf-next", "›", theme, cx, move |state| {
                                            keyframe_rows::apply(state, Row::Next(param))
                                        })
                                        .id(crate::motion::MotionKey::new("kf-next", &id)
                                            .element_id("kf-next")),
                                    )
                                })
                                .child(
                                    pick_button(
                                        "kf-toggle",
                                        if here { "◆" } else { "◇" },
                                        theme,
                                        cx,
                                        move |state| {
                                            keyframe_rows::apply(
                                                state,
                                                Row::Key { param, keyed: here },
                                            )
                                        },
                                    )
                                    .id(crate::motion::MotionKey::new("kf-toggle", &id)
                                        .element_id("kf-toggle")),
                                )
                                .child(
                                    pick_button(
                                        "kf-driver",
                                        if driven { "ƒ Remove" } else { "ƒ Driver" },
                                        theme,
                                        cx,
                                        move |state| {
                                            keyframe_rows::apply(state, Row::Driver(param))
                                        },
                                    )
                                    .id(crate::motion::MotionKey::new("kf-driver", &id)
                                        .element_id("kf-driver")),
                                )
                                .when(count > 0, |el| {
                                    el.child(
                                        pick_button(
                                            "kf-flatten",
                                            "Flatten",
                                            theme,
                                            cx,
                                            move |state| {
                                                keyframe_rows::apply(state, Row::Flatten(param))
                                            },
                                        )
                                        .id(crate::motion::MotionKey::new("kf-flatten", &id)
                                            .element_id("kf-flatten")),
                                    )
                                }),
                        )
                        .into_any_element()],
                ))
                .when(here, |el| {
                    el.child(ghosts_under_labels(
                        Columns::Keyed,
                        easing_chips.into_iter().chain(piecewise_chips),
                    ))
                })
                .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Keyframes"))
        .children(rows)
}
