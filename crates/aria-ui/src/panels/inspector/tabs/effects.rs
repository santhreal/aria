//! Effects tab: the compositor effect chain.

#[path = "effects_params.rs"]
pub(crate) mod effects_params;

use crate::tooltip::WithTip as _;
use gpui::{
    AppContext, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::components;
use crate::panels::sidebar::DragGhost;
use crate::surfaces::effects_rack::{self, Gesture};
use crate::theme::Theme;
use crate::types::{CatalogDrag, ContextTarget, RenameTarget, UiClip};

use gpui::prelude::FluentBuilder;

use crate::components::section_label;

pub fn effects_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let hover = theme.glass_hover();
    let accent = theme.accent;
    let sequence_clips: Vec<(aria_models::ClipId, gpui::SharedString)> = state
        .read(cx)
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .filter(|c| c.id != clip.id)
        .map(|c| (aria_models::ClipId::from_raw(c.id), c.name.clone()))
        .collect();
    let font_families = state.read(cx).font_families();
    let clip_id = clip.id;
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
            let bypass = effect.bypass;
            let carried = EffectDrag {
                clip: clip_id,
                index,
                label: name.clone(),
            };
            let params = effects_params::effect_param_rows(
                theme,
                state,
                index,
                effect,
                &sequence_clips,
                font_families,
                cx,
            );
            div()
                .id(gpui::ElementId::named_usize("inspector-fx", index))
                // A dragged effect lands above the effect it is dropped on.
                .drag_over::<CatalogDrag>(move |style, _, _, _| {
                    style.border_t_2().border_color(accent)
                })
                .on_drop(cx.listener(move |this, drag: &CatalogDrag, _, cx| {
                    drop_on_chain(this, drag, Some(index), cx);
                }))
                // So does an effect dragged by its header.
                .drag_over::<EffectDrag>(move |style, _, _, _| {
                    style.border_t_2().border_color(accent)
                })
                .on_drop(cx.listener(move |this, drag: &EffectDrag, _, cx| {
                    move_on_chain(this, drag, Some(index), cx);
                }))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .id(gpui::ElementId::named_usize("inspector-fx-header", index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(Theme::SPACE_XS))
                        .py(px(Theme::SPACE_XS))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .hover(move |el| el.bg(hover))
                        .on_drag(carried, |drag, _offset, _window, cx| {
                            let label = drag.label.clone();
                            cx.new(|_| DragGhost { label })
                        })
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                open_effect_menu(this, clip_id, index, event, cx);
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(Theme::SPACE_XS))
                                .min_w_0()
                                .child(
                                    components::chip(
                                        crate::motion::MotionKey::new("inspector-fx-bypass", index),
                                        "fx",
                                        !bypass,
                                        components::ChipSize::Compact,
                                        theme,
                                    )
                                    .debug_selector(move || format!("inspector-fx-bypass-{index}"))
                                    .when_some(
                                        crate::tooltip::builder("inspector-fx-bypass"),
                                        |el, tip| el.with_tip(tip),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.state.update(cx, |state, _| {
                                                if let Err(err) = effects_rack::apply(
                                                    state,
                                                    Gesture::Bypass(index),
                                                ) {
                                                    state.report_error(&err);
                                                }
                                            });
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .child(
                                    div()
                                        .id(gpui::ElementId::named_usize(
                                            "inspector-fx-name",
                                            index,
                                        ))
                                        .min_w_0()
                                        .truncate()
                                        .text_size(Theme::TEXT_UI)
                                        .text_color(if bypass {
                                            theme.text_faint
                                        } else {
                                            theme.text
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                move |this, event: &MouseDownEvent, _, cx| {
                                                    if event.click_count < 2 {
                                                        return;
                                                    }
                                                    cx.stop_propagation();
                                                    this.state.update(cx, |state, _| {
                                                        state.begin_rename(RenameTarget::Effect {
                                                            clip: clip_id,
                                                            index,
                                                        });
                                                    });
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                        .child(name),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(Theme::SPACE_XS))
                                .child(reorder_button(
                                    "inspector-fx-up",
                                    index,
                                    crate::icons::CHEVRON_UP,
                                    (index > 0).then_some(move || Gesture::MoveUp(index)),
                                    theme,
                                    cx,
                                ))
                                .child(reorder_button(
                                    "inspector-fx-down",
                                    index,
                                    crate::icons::CHEVRON_DOWN,
                                    (!last).then_some(move || Gesture::MoveDown(index)),
                                    theme,
                                    cx,
                                ))
                                .child(
                                    components::icon_button(
                                        ("inspector-fx-reset", index),
                                        crate::icons::ROTATE_CCW,
                                        theme,
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.state.update(cx, |state, _| {
                                                if let Err(err) = effects_rack::apply(
                                                    state,
                                                    Gesture::Reset(index),
                                                ) {
                                                    state.report_error(&err);
                                                }
                                            });
                                            cx.notify();
                                        }),
                                    ),
                                )
                                .child(
                                    components::icon_button(
                                        ("inspector-fx-menu", index),
                                        crate::icons::ELLIPSIS,
                                        theme,
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            open_effect_menu(this, clip_id, index, event, cx);
                                        }),
                                    ),
                                )
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
        .presets()
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
    let plugin_presets: Vec<_> = state
        .read(cx)
        .plugin_presets()
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let apply_name = entry.preset.name.clone();
            let plugin_label = entry.plugin.to_string();
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::ghost_button(
                        crate::motion::MotionKey::new("inspector-fx-plugin-preset", index),
                        entry.preset.name.clone(),
                        theme,
                    )
                    .with_tip(crate::tooltip::text_builder(plugin_label.clone()))
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
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(plugin_label),
                )
                .into_any_element()
        })
        .collect();
    let broken_presets: Vec<_> = state
        .read(cx)
        .broken_presets()
        .iter()
        .enumerate()
        .map(|(index, broken)| {
            let file_name = broken
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| broken.path.display().to_string());
            div()
                .id(gpui::ElementId::named_usize(
                    "inspector-fx-preset-broken",
                    index,
                ))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.danger)
                .child(format!("{file_name}: {}", broken.reason))
                .into_any_element()
        })
        .collect();

    // Saving needs a name and part selection: the button opens a draft with one
    // toggle per part (all on by default), and the field's Enter or the Save
    // button commits. A refused name or empty selection leaves the draft open.
    let open_draft = state.read(cx).preset_draft().cloned();
    let naming = open_draft.is_some();
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
                    state.open_preset_draft();
                }
            });
            cx.notify();
        }),
    );
    let draft_block = open_draft.map(|draft| {
        let mut part_rows: Vec<gpui::AnyElement> = Vec::new();
        for (fx_idx, label, on) in draft.effects {
            let text = format!("{} {label}", if on { "✓" } else { "○" });
            part_rows.push(
                components::ghost_button(
                    crate::motion::MotionKey::new("inspector-fx-preset-part-fx", fx_idx),
                    text,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            let _ = effects_rack::apply(state, Gesture::TogglePresetEffect(fx_idx));
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
        }
        if !draft.audio_effects.is_empty() {
            let text = format!(
                "{} Audio inserts ({})",
                if draft.include_audio_effects {
                    "✓"
                } else {
                    "○"
                },
                draft.audio_effects.len()
            );
            part_rows.push(
                components::ghost_button("inspector-fx-preset-part-audio", text, theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                let _ =
                                    effects_rack::apply(state, Gesture::TogglePresetAudioEffects);
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
            );
        }
        for (prop_id, label, on) in draft.properties {
            let text = format!("{} {label}", if on { "✓" } else { "○" });
            let id_for_click = prop_id.clone();
            part_rows.push(
                components::ghost_button(
                    crate::motion::MotionKey::new(
                        "inspector-fx-preset-part-prop",
                        prop_id.as_str(),
                    ),
                    text,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            let _ = effects_rack::apply(
                                state,
                                Gesture::TogglePresetProperty(id_for_click.clone()),
                            );
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(components::text_box(
                "inspector-fx-preset-name",
                &draft.name,
                "Name this preset",
                true,
                theme,
            ))
            .children(part_rows)
    });

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        // Anywhere else in the tab, a dragged effect lands at the end of the
        // chain.
        .drag_over::<CatalogDrag>(move |style, _, _, _| style.bg(accent.opacity(0.06)))
        .on_drop(cx.listener(|this, drag: &CatalogDrag, _, cx| {
            drop_on_chain(this, drag, None, cx);
        }))
        .drag_over::<EffectDrag>(move |style, _, _, _| style.bg(accent.opacity(0.06)))
        .on_drop(cx.listener(|this, drag: &EffectDrag, _, cx| {
            move_on_chain(this, drag, None, cx);
        }))
        .child(section_label(theme, "Effects"))
        .children(remove_rows)
        .child(section_label(theme, "Presets"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .children(presets)
                .children(plugin_presets)
                .children(broken_presets)
                .when_some(draft_block, |el, field| el.child(field))
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

/// Land a catalog entry dragged onto the tab: before the effect at `at`, or at
/// the end of the chain.
fn drop_on_chain(
    this: &mut crate::shell::Shell,
    drag: &CatalogDrag,
    at: Option<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    this.state.update(cx, |state, _| {
        if let Err(err) = state.drop_catalog_on_effect_controls(drag, at) {
            state.report_error(&err);
        }
    });
    crate::app::spawn_preview(this.state.clone(), cx);
    cx.notify();
}

/// An effect carried by its header to another place in its chain.
#[derive(Clone)]
struct EffectDrag {
    /// The clip whose chain the effect is in.
    clip: u64,
    /// Where the effect sits in the chain.
    index: usize,
    /// The name the header draws, for the ghost.
    label: gpui::SharedString,
}

/// Land an effect carried by its header: above the effect at `before`, or at
/// the end of the chain. A header carried over another clip's chain lands
/// nowhere.
fn move_on_chain(
    this: &mut crate::shell::Shell,
    drag: &EffectDrag,
    before: Option<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let from = drag.index;
    let clip = drag.clip;
    this.state.update(cx, |state, _| {
        if state.selected_clip().map(aria_models::ClipId::raw) != Some(clip) {
            return;
        }
        if let Err(err) = effects_rack::apply(state, Gesture::Drop { from, before }) {
            state.report_error(&err);
        }
    });
    crate::app::spawn_preview(this.state.clone(), cx);
    cx.notify();
}

/// The control on the header of the effect at `index` that applies the gesture
/// `gesture` builds, a move one place earlier or later, or the room of one where
/// the effect cannot move that way: the controls after it stand in the columns
/// the other headers draw them in.
fn reorder_button(
    id: &'static str,
    index: usize,
    icon: &'static str,
    gesture: Option<impl Fn() -> Gesture + 'static>,
    theme: &Theme,
    cx: &gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let Some(gesture) = gesture else {
        return components::icon_space().into_any_element();
    };
    components::icon_button((id, index), icon, theme)
        .debug_selector(move || format!("{id}-{index}"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = effects_rack::apply(state, gesture()) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

/// Open the effect header's menu at the pointer, on the effect at `index` in
/// `clip`'s chain.
fn open_effect_menu(
    this: &mut crate::shell::Shell,
    clip: u64,
    index: usize,
    event: &MouseDownEvent,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    this.state.update(cx, |state, _| {
        state.open_context(
            ContextTarget::Effect { clip, index },
            f32::from(event.position.x),
            f32::from(event.position.y),
        );
    });
    cx.notify();
}
