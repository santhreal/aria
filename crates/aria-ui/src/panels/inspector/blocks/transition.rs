//! Transition block: the blend at the selected clip's right cut, in the Look tab
//! for a cut between pictures and in the Audio tab for a cut between sounds.
//!
//! A picture cut lists `aria_models::transition_catalog` at render time, so a
//! new blend appears without a second list here; a sound cut lists the audio
//! crossfades. The tabs draw the block only where the projected
//! `transition_media` says the junction takes that kind, which is the same rule
//! the engine enforces.

use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::app::clip_transitions::TRANSITION_STEP_TICKS;
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::{Chip, Columns, Readout, chip_row, choice_chip, pick_button, unkeyed_row};

use crate::components::section_label;

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
    let mut chips: Vec<_> = choices
        .iter()
        .map(|choice| {
            let id = choice.id.to_string();
            let on = applied.as_ref().is_some_and(|t| t.id == choice.id);
            choice_chip(
                MotionKey::new("transition-choice", &choice.id),
                choice.label.clone(),
                on,
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::Transition(id.clone())),
            )
        })
        .collect();
    if applied.is_some() {
        chips.push(Chip::other(pick_button(
            "transition-clear",
            "Remove transition",
            theme,
            cx,
            |state| clip_pick::apply(state, Pick::ClearTransition),
        )));
    }

    let duration_ticks = applied.as_ref().map(|t| t.duration_ticks).unwrap_or(0);
    let alignment = applied.as_ref().map(|t| t.alignment).unwrap_or_default();
    let align_chips: Vec<_> = aria_models::TransitionAlignment::ALL
        .into_iter()
        .map(|choice| {
            let on = applied.is_some() && alignment == choice;
            choice_chip(
                MotionKey::new("transition-align", choice.id()),
                choice.label(),
                on,
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::TransitionAlignment(choice)),
            )
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
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "transition-at"),
            "At cut",
            Columns::Keyed,
            applied
                .as_ref()
                .map_or_else(|| SharedString::new_static("None"), |t| t.label.clone()),
            chips,
        ))
        .when(applied.is_some(), |this| {
            this.child(unkeyed_row(
                theme,
                state,
                MotionKey::new("insp.row", "transition-duration"),
                "Duration (ticks)",
                Readout::number(duration_ticks as f32, 0, TRANSITION_STEP_TICKS as f32),
                Columns::Keyed,
                move |state, by| clip_pick::apply(state, Pick::TransitionLength(by.round() as i64)),
                cx,
            ))
            .child(chip_row(
                theme,
                MotionKey::new("insp.row", "transition-align"),
                "Alignment",
                Columns::Keyed,
                alignment.label(),
                align_chips,
            ))
            .children(transition_params)
        })
}
