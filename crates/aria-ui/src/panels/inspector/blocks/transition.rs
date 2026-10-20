//! Transition block: the blend at the selected clip's right cut, in the Look tab
//! for a cut between pictures and in the Audio tab for a cut between sounds.
//!
//! A picture cut lists `aria_models::transition_catalog` at render time, so a
//! new blend appears without a second list here; a sound cut lists the audio
//! crossfades. The tabs draw the block only where the projected
//! `transition_media` says the junction takes that kind, which is the same rule
//! the engine enforces.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::app::clip_transitions::TRANSITION_STEP_TICKS;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{Readout, audio_row, section_label};

pub fn transition_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let applied = clip.transition.clone();
    let picture;
    let choices: &[crate::catalog_text::Choice] = match clip.transition_media {
        Some(aria_models::TransitionMedia::Audio) => crate::catalog_text::audio_transitions(),
        _ => {
            picture = crate::catalog_text::transitions();
            &picture
        }
    };
    let chips: Vec<_> = choices
        .iter()
        .map(|choice| {
            let id = choice.id.to_string();
            let selected = applied.as_ref().is_some_and(|t| t.id == choice.id);
            components::ghost_button(
                crate::motion::MotionKey::new("transition-choice", &choice.id),
                if selected {
                    gpui::SharedString::from(format!("• {}", choice.label))
                } else {
                    choice.label.clone()
                },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::Transition(id)) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();

    let duration_ticks = applied.as_ref().map(|t| t.duration_ticks).unwrap_or(0);
    let alignment = applied.as_ref().map(|t| t.alignment).unwrap_or_default();
    let align_chips: Vec<_> = aria_models::TransitionAlignment::ALL
        .into_iter()
        .map(|choice| {
            let selected = applied.is_some() && alignment == choice;
            components::ghost_button(
                crate::motion::MotionKey::new("transition-align", choice.id()),
                if selected {
                    gpui::SharedString::from(format!("• {}", choice.label()))
                } else {
                    gpui::SharedString::new_static(choice.label())
                },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::TransitionAlignment(choice))
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
    let transition_params: Vec<gpui::AnyElement> = applied
        .as_ref()
        .and_then(|tr| {
            let spec = aria_models::transition_spec(&tr.id)?;
            Some(
                crate::panels::inspector::tabs::effects::effects_params::transition::transition_param_rows(
                    theme,
                    state,
                    tr,
                    &spec,
                    cx,
                ),
            )
        })
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Transition"))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(format!(
                    "At cut: {}",
                    applied.as_ref().map(|t| t.label.as_str()).unwrap_or("None")
                )),
        )
        .children(chips)
        .when(applied.is_some(), |this| {
            this.child(audio_row(
                theme,
                state,
                crate::motion::MotionKey::new("insp.row", "transition-duration"),
                "Duration (ticks)",
                Readout::number(duration_ticks as f32, 0, TRANSITION_STEP_TICKS as f32),
                move |state, by| clip_pick::apply(state, Pick::TransitionLength(by.round() as i64)),
                cx,
            ))
            .children(align_chips)
            .children(transition_params)
            .child(
                components::ghost_button("transition-clear", "Remove transition", theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = clip_pick::apply(state, Pick::ClearTransition) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
            )
        })
}
