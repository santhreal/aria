//! Transition block of the Look tab: the blend at the selected clip's right cut.
//!
//! The catalog comes from `aria_models::transition_catalog` at render time, so a
//! new blend appears without a second list here. The whole block is hidden unless
//! the projected `can_transition` flag says the junction can carry one, which is
//! the same rule the engine enforces.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::app::clip_transitions::TRANSITION_STEP_TICKS;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{audio_row, section_label};

pub fn transition_block(
    theme: &Theme,
    clip: &UiClip,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let applied = clip.transition.clone();
    let chips: Vec<_> = crate::catalog_text::transitions()
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

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Transition"))
        .child(
            div()
                .text_size(px(11.))
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
                crate::motion::MotionKey::new("insp.row", "transition-duration"),
                "Duration (ticks)",
                format!("{duration_ticks}"),
                1.0,
                move |state, step| {
                    let delta = if step >= 0.0 {
                        TRANSITION_STEP_TICKS
                    } else {
                        -TRANSITION_STEP_TICKS
                    };
                    clip_pick::apply(state, Pick::TransitionLength(delta))
                },
                cx,
            ))
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
