//! Grade block of the Look tab: the manual color adjustments, the blend mode
//! and stabilization.
//!
//! The slider rows are generated from the property catalog's `grade` group, so
//! the panel names no slider and an edit writes the one property it addresses.
//! `SetClipAdjustments` carries every slider at once, so the old per-slider
//! setter rebuilt the block and flattened its keyframes.

use super::wheels;

use gpui::{IntoElement, ParentElement, Styled, div, px};

use aria_models::{BlendMode, StabilizeLevel};

use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::{Chip, Columns, chip_row, ghosts_under_labels, param_rows, pick_button};

use crate::components::section_label;

pub fn grade_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let rows = param_rows(theme, "grade", state, cx);
    let reset = pick_button("grade-clear", "Reset grade", theme, cx, |state| {
        inspector_pick::apply(state, Pick::ClearGrade)
    });
    let blends: Vec<_> = BlendMode::ALL
        .iter()
        .map(|mode| {
            let mode = *mode;
            Chip::choice(mode.label(), clip.blend_mode == mode, |label| {
                pick_button(("grade-blend", mode.id()), label, theme, cx, move |state| {
                    inspector_pick::apply(state, Pick::Blend(mode))
                })
                .into_any_element()
            })
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(wheels::wheels_block(theme, "inspector", state, cx))
        .child(section_label(theme, "Grade"))
        .children(rows)
        .child(ghosts_under_labels(
            Columns::Keyed,
            [reset.into_any_element()],
        ))
        .child(section_label(theme, "Blend"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "blend"),
            "Mode",
            Columns::Keyed,
            clip.blend_mode.label(),
            blends,
        ))
        .when(clip.can_stabilize, |this| {
            this.child(section_label(theme, "Stabilize"))
                .child(stabilize_row(theme, clip.stabilize, cx))
        })
}

/// The Stabilize row: the level in force, `on`, and a chip for each level and
/// for Off.
fn stabilize_row(
    theme: &Theme,
    on: Option<StabilizeLevel>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Div {
    let mut chips: Vec<_> = StabilizeLevel::ALL
        .iter()
        .map(|level| {
            let level = *level;
            Chip::choice(level.label(), on == Some(level), |label| {
                pick_button(
                    ("stabilize-level", level.id()),
                    label,
                    theme,
                    cx,
                    move |state| inspector_pick::apply(state, Pick::Stabilize(Some(level))),
                )
                .into_any_element()
            })
        })
        .collect();
    chips.push(Chip::choice("Off", on.is_none(), |label| {
        pick_button("stabilize-off", label, theme, cx, |state| {
            inspector_pick::apply(state, Pick::Stabilize(None))
        })
        .into_any_element()
    }));
    chip_row(
        theme,
        MotionKey::new("insp.row", "stabilize"),
        "Level",
        Columns::Keyed,
        on.map_or("Off", StabilizeLevel::label),
        chips,
    )
}
