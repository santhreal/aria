//! Grade block of the Look tab: blend mode, stabilization, denoise, and the
//! manual colour adjustments.
//!
//! The slider rows are generated from the property catalog's `grade` group, so
//! the panel names no slider and an edit writes the one property it addresses.
//! `SetClipAdjustments` carries every slider at once, so the old per-slider
//! setter rebuilt the block and flattened its keyframes.

use super::wheels;

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use aria_models::{BlendMode, StabilizeLevel};

use crate::components;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::param_rows;

use crate::components::section_label;

pub fn grade_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let rows = param_rows(theme, "grade", state, cx);

    let blends: Vec<_> = BlendMode::ALL
        .iter()
        .map(|mode| {
            let mode = *mode;
            components::ghost_button(("grade-blend", mode.id()), mode.label(), theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = inspector_pick::apply(state, Pick::Blend(mode)) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element()
        })
        .collect();

    let mut stabilizers: Vec<_> = StabilizeLevel::ALL
        .iter()
        .map(|level| {
            let level = *level;
            components::ghost_button(("stabilize-level", level.id()), level.label(), theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) =
                                inspector_pick::apply(state, Pick::Stabilize(Some(level)))
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
    stabilizers.push(
        components::ghost_button("stabilize-off", "Off", theme)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::Stabilize(None)) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element(),
    );

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(wheels::wheels_block(theme, "inspector", state, cx))
        .child(section_label(theme, "Grade"))
        .children(rows)
        .child(
            components::ghost_button("grade-clear", "Reset grade", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::ClearGrade) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        )
        .child(section_label(theme, "Blend"))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(clip.blend_mode.label()),
        )
        .children(blends)
        .when(clip.can_stabilize, |this| {
            this.child(section_label(theme, "Stabilize"))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_faint)
                        .child(match clip.stabilize {
                            Some(level) => level.label(),
                            None => "Off",
                        }),
                )
                .children(stabilizers)
        })
        .child(section_label(theme, "Denoise"))
        .child(
            components::ghost_button(
                "denoise-toggle",
                if clip.denoise { "On" } else { "Off" },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::ToggleDenoise) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        )
}
