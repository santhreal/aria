//! Keyframes block: one row per animatable property of the selected clip.
//!
//! Rows come from `AppState::key_params`, the projected property list, which
//! drops the properties this clip's lane refuses, so a row can never offer a
//! command the model would reject. Each row already carries its keys, so the
//! strip reads how many the property has, which one sits under the playhead and
//! which curve leaves it without asking the model again per row.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::section_label;

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

            let easing_chips: Vec<_> = easing_presets()
                .iter()
                .map(|preset| {
                    let preset_id = preset.id.as_str();
                    let selected = preset_here == Some(preset_id);
                    components::ghost_button(
                        preset_id,
                        if selected {
                            gpui::SharedString::from(format!("• {}", preset.label))
                        } else {
                            gpui::SharedString::new_static(preset.label.as_str())
                        },
                        theme,
                    )
                    .id(crate::motion::MotionKey::new(preset_id, &id).element_id("kf-preset"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = keyframe_rows::apply(
                                    state,
                                    Row::Easing {
                                        param,
                                        preset: preset_id,
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

            let piecewise_chips: Vec<_> = PIECEWISE_PRESETS
                .iter()
                .map(|(preset, label)| {
                    let preset = *preset;
                    components::ghost_button(
                        crate::motion::MotionKey::new(label, &id),
                        *label,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) =
                                    keyframe_rows::apply(state, Row::Piecewise { param, preset })
                                {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                })
                .collect();

            div()
                .id(id.clone())
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.text_faint)
                                .child(format!("{label} — {count} keys")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(Theme::SPACE_XS))
                                .when(count > 0, |el| {
                                    el.child(
                                        components::ghost_button("kf-prev", "‹", theme)
                                            .id(crate::motion::MotionKey::new("kf-prev", &id)
                                                .element_id("kf-prev"))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _: &MouseDownEvent, _, cx| {
                                                        this.state.update(cx, |state, _| {
                                                            if let Err(err) = keyframe_rows::apply(
                                                                state,
                                                                Row::Previous(param),
                                                            ) {
                                                                state.report_error(&err);
                                                            }
                                                        });
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                    )
                                    .child(
                                        components::ghost_button("kf-next", "›", theme)
                                            .id(crate::motion::MotionKey::new("kf-next", &id)
                                                .element_id("kf-next"))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _: &MouseDownEvent, _, cx| {
                                                        this.state.update(cx, |state, _| {
                                                            if let Err(err) = keyframe_rows::apply(
                                                                state,
                                                                Row::Next(param),
                                                            ) {
                                                                state.report_error(&err);
                                                            }
                                                        });
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                    )
                                })
                                .child(
                                    components::ghost_button(
                                        "kf-toggle",
                                        if here { "◆" } else { "◇" },
                                        theme,
                                    )
                                    .id(crate::motion::MotionKey::new("kf-toggle", &id)
                                        .element_id("kf-toggle"))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.state.update(cx, |state, _| {
                                                let result = keyframe_rows::apply(
                                                    state,
                                                    Row::Key { param, keyed: here },
                                                );
                                                if let Err(err) = result {
                                                    state.report_error(&err);
                                                }
                                            });
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .when(count > 0, |el| {
                                    el.child(
                                        components::ghost_button("kf-flatten", "Flatten", theme)
                                            .id(crate::motion::MotionKey::new("kf-flatten", &id)
                                                .element_id("kf-flatten"))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _: &MouseDownEvent, _, cx| {
                                                        this.state.update(cx, |state, _| {
                                                            if let Err(err) = keyframe_rows::apply(
                                                                state,
                                                                Row::Flatten(param),
                                                            ) {
                                                                state.report_error(&err);
                                                            }
                                                        });
                                                        cx.notify();
                                                    },
                                                ),
                                            ),
                                    )
                                }),
                        ),
                )
                .when(here, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(Theme::SPACE_XS))
                            .children(easing_chips)
                            .children(piecewise_chips),
                    )
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
