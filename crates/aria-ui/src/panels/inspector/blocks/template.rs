//! Template block of the Info tab: replace media, and the authoring marks.
//!
//! Three engine commands surface here. Replace media repoints a clip at other
//! pooled media while keeping its placement. The slot marks a clip as fillable
//! when the project is used as a template, and the hint is what the placeholder
//! reads. Text editability is offered only on a text clip, which is the only clip
//! the model accepts it on.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use aria_models::MediaId;

use crate::app::clip_template::slot_media_label;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::{UiClip, UiClipKind};

use super::widgets::{Readout, audio_row, section_label};

pub fn template_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let slot = clip.replaceable.clone();
    let choices = state.read(cx).selected_slot_choices();

    // Only pooled media of a compatible kind can replace this clip's source: an
    // audio clip cannot take a picture, and a visual clip cannot take a song.
    let want_audio = clip.kind == UiClipKind::Audio;
    let pool: Vec<_> = state
        .read(cx)
        .store
        .media
        .iter()
        .filter(|item| !item.missing && (item.kind == UiClipKind::Audio) == want_audio)
        .filter(|item| Some(item.id) != clip.media_id)
        .map(|item| (item.id, item.name.clone()))
        .collect();

    let replace_rows: Vec<_> = pool
        .into_iter()
        .map(|(id, name)| {
            components::ghost_button("inspector-replace-media", name, theme)
                .id(crate::motion::MotionKey::new("insp-replace", id).element_id("insp-replace"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) =
                                clip_pick::apply(state, Pick::ReplaceMedia(MediaId::from_raw(id)))
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

    let accepts_chips: Vec<_> = choices
        .into_iter()
        .map(|(kind, label)| {
            let selected = slot.as_ref().is_some_and(|s| s.accepts == kind);
            components::ghost_button(
                label,
                if selected {
                    format!("• {label}")
                } else {
                    label.to_string()
                },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::SlotAccepts(kind)) {
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
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Template"))
        .when(clip.media_id.is_some() && !replace_rows.is_empty(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("Replace media"),
            )
            .children(replace_rows)
        })
        .child(
            components::ghost_button(
                "inspector-slot-toggle",
                if slot.is_some() {
                    "Clear template slot"
                } else {
                    "Mark as template slot"
                },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::ToggleReplaceable) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        )
        .when_some(slot, |el, slot| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(format!(
                        "Slot {} — {} — {}",
                        slot.order,
                        slot_media_label(slot.accepts),
                        slot.label.clone().unwrap_or_else(|| "no hint".into())
                    )),
            )
            .child(audio_row(
                theme,
                &state,
                crate::motion::MotionKey::new("insp.row", "inspector-slot-order"),
                "Fill order",
                Readout::number(slot.order as f32, 0, 1.0),
                |state, by| clip_pick::apply(state, Pick::SlotOrder(by.round() as i32)),
                cx,
            ))
            .children(accepts_chips)
            .child(
                components::ghost_button("inspector-slot-hint", "Edit hint…", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            state.begin_rename(crate::types::RenameTarget::SlotLabel);
                        });
                        cx.notify();
                    }),
                ),
            )
            .when(slot.label.is_some(), |el| {
                el.child(
                    components::ghost_button("inspector-slot-hint-clear", "Clear hint", theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| {
                                    if let Err(err) = clip_pick::apply(state, Pick::ClearSlotLabel)
                                    {
                                        state.report_error(&err);
                                    }
                                });
                                cx.notify();
                            }),
                        ),
                )
            })
        })
        .when(clip.kind == UiClipKind::Text, |el| {
            el.child(
                components::ghost_button(
                    "inspector-text-editable",
                    if clip.text_editable {
                        "Text editable in template: on"
                    } else {
                        "Text editable in template: off"
                    },
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = clip_pick::apply(state, Pick::ToggleTextEditable) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
}
