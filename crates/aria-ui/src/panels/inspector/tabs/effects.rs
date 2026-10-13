//! Effects tab: the compositor effect chain.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::components;
use crate::surfaces::effects_rack::{self, Gesture};
use crate::theme::Theme;
use crate::types::UiClip;

use crate::fluent::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{audio_row, section_label};

pub fn effects_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let hover = theme.glass_hover();
    let remove_rows: Vec<_> = clip
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            let name = if effect.name.is_empty() {
                effect.id.clone()
            } else {
                effect.name.clone()
            };
            let last = index + 1 == clip.effects.len();
            // Built in three passes rather than one chained iterator: every row
            // needs `cx`, and chained closures would each want it at once.
            let mut params: Vec<gpui::AnyElement> = Vec::new();
            for (slot, spec) in &crate::app::clip_effects::scalar_params(&effect.id) {
                let value = effect.scalar(slot).unwrap_or(spec.default);
                let step = ((spec.max - spec.min) / 20.0).max(0.01);
                params.push(
                    audio_row(
                        theme,
                        crate::motion::MotionKey::new(&spec.name, index),
                        spec.label.as_str(),
                        format!("{value:.2}"),
                        step,
                        move |state, by| {
                            effects_rack::apply(state, Gesture::NudgeScalar { index, slot, by })
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
            for (slot, spec) in &crate::app::clip_effects::vec2_params(&effect.id) {
                let value = effect.vec2(slot).unwrap_or(spec.default_vec2);
                let step = ((spec.max - spec.min) / 20.0).max(0.01);
                for (axis, name) in [(0usize, "x"), (1usize, "y")] {
                    params.push(
                        audio_row(
                            theme,
                            crate::motion::MotionKey::new(&spec.name, index).with(name),
                            &format!("{} {name}", spec.label),
                            format!("{:.3}", value[axis]),
                            step,
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
            for (slot, spec) in &crate::app::clip_effects::color_params(&effect.id) {
                let value = effect.color(slot).unwrap_or(spec.default_color);
                params.push(
                    color_row(theme, index, slot, spec.label.as_str(), value, cx)
                        .into_any_element(),
                );
            }
            div()
                .id(gpui::ElementId::named_usize("inspector-fx", index))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(Theme::SPACE_XS))
                        .py(px(Theme::SPACE_XS))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .hover(move |el| el.bg(hover))
                        .child(div().text_size(px(13.)).text_color(theme.text).child(name))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(Theme::SPACE_XS))
                                .when(index > 0, |el| {
                                    el.child(
                                        components::ghost_button(
                                            crate::motion::MotionKey::new("inspector-fx-up", index),
                                            "↑",
                                            theme,
                                        )
                                        .when_some(
                                            crate::tooltip::builder("inspector-fx-up"),
                                            |el, tip| el.tooltip(tip),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                this.state.update(cx, |state, _| {
                                                    if let Err(err) = effects_rack::apply(
                                                        state,
                                                        Gesture::MoveUp(index),
                                                    ) {
                                                        state.report_error(&err);
                                                    }
                                                });
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                })
                                .when(!last, |el| {
                                    el.child(
                                        components::ghost_button(
                                            crate::motion::MotionKey::new(
                                                "inspector-fx-down",
                                                index,
                                            ),
                                            "↓",
                                            theme,
                                        )
                                        .when_some(
                                            crate::tooltip::builder("inspector-fx-down"),
                                            |el, tip| el.tooltip(tip),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                this.state.update(cx, |state, _| {
                                                    if let Err(err) = effects_rack::apply(
                                                        state,
                                                        Gesture::MoveDown(index),
                                                    ) {
                                                        state.report_error(&err);
                                                    }
                                                });
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                })
                                .child(
                                    components::icon_button(
                                        ("inspector-fx-remove", index),
                                        crate::icons::TRASH,
                                        theme,
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.state.update(cx, |state, _| {
                                                if let Err(err) = effects_rack::apply(
                                                    state,
                                                    Gesture::Remove(index),
                                                ) {
                                                    state.report_error(&err);
                                                }
                                            });
                                            cx.notify();
                                        }),
                                    ),
                                ),
                        ),
                )
                .children(params)
                .into_any_element()
        })
        .collect();

    let catalog: Vec<_> = state
        .read(cx)
        .effect_browse_list()
        .into_iter()
        .map(|(spec, label)| {
            components::ghost_button(
                crate::motion::MotionKey::new("fx-add", spec.id.as_str()),
                label,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = effects_rack::apply(state, Gesture::Add(spec.id.clone()))
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
    let presets: Vec<_> = state
        .read(cx)
        .effect_presets()
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let apply_name = preset.name.to_string();
            let drop_name = preset.name.to_string();
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::ghost_button(
                        crate::motion::MotionKey::new("inspector-fx-preset", index),
                        preset.name.to_string(),
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) =
                                    effects_rack::apply(state, Gesture::Preset(apply_name.clone()))
                                {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button(
                        crate::motion::MotionKey::new("inspector-fx-preset-delete", index),
                        "Delete",
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = effects_rack::apply(
                                    state,
                                    Gesture::DeletePreset(drop_name.clone()),
                                ) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element()
        })
        .collect();

    // Saving needs a name, so the button opens a field and the field's Enter is
    // the save. A refused name (empty, or one already taken) leaves the field
    // open with what was typed still in it.
    let naming = state.read(cx).effect_preset_draft_open();
    let draft = state.read(cx).effect_preset_draft_text();
    let save = components::ghost_button(
        "inspector-fx-preset-save",
        if naming { "Save" } else { "Save as preset" },
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if naming {
                    if let Err(err) = effects_rack::apply(state, Gesture::SavePreset) {
                        state.report_error(&err);
                    }
                } else {
                    state.open_effect_preset_draft();
                }
            });
            cx.notify();
        }),
    );
    let name_box = naming.then(|| {
        components::text_box(
            "inspector-fx-preset-name",
            &draft,
            "Name this preset",
            true,
            theme,
        )
    });

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Effects"))
        .children(remove_rows)
        .child(section_label(theme, "Presets"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .children(presets)
                .when_some(name_box, |el, field| el.child(field))
                .child(save),
        )
        .child(section_label(theme, "Add"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .children(catalog),
        )
}

/// One colour parameter: its current value as a swatch, then the palette.
///
/// A colour slot has no constant command, so each click writes a keyframe at the
/// playhead — or at the clip start when the playhead is elsewhere.
fn color_row(
    theme: &Theme,
    index: usize,
    slot: usize,
    label: &str,
    value: [u8; 4],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let swatches: Vec<_> = crate::app::clip_effects::PARAM_COLORS
        .iter()
        .map(|(name, _, rgba)| {
            let rgba = *rgba;
            let picked = rgba == value;
            div()
                .id(crate::motion::MotionKey::new("fx-slot", index)
                    .with(slot)
                    .with(name)
                    .element_id("fx-slot"))
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
                            if let Err(err) =
                                effects_rack::apply(state, Gesture::Color { index, slot, rgba })
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
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.))
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
}

/// A colour parameter's bytes as a paint colour. Alpha 0 shows as the panel's own
/// border tint rather than nothing, so a cleared slot is still a target.
fn swatch(rgba: [u8; 4]) -> gpui::Hsla {
    gpui::Hsla::from(gpui::Rgba {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: rgba[3] as f32 / 255.0,
    })
}

/// The palette name of a colour, or its hex when it is not one of the presets, so
/// a value written by the agent or an older project still reads as something.
fn color_name(rgba: [u8; 4]) -> String {
    crate::app::clip_effects::PARAM_COLORS
        .iter()
        .find(|(_, _, preset)| *preset == rgba)
        .map(|(_, label, _)| (*label).to_string())
        .unwrap_or_else(|| {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )
        })
}
