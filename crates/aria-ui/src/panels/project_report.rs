//! The project report: what the sequence carries, and what the last frame cost.
//!
//! Three tables the model and the engine already answered and nothing drew:
//!
//! * the pool and sequence counts (`media_count`, `bin_count`, `marker_count`,
//!   `clip_count`, transitions, soloed lane kinds),
//! * one row per per-clip question, with how many clips answer it — a speed ramp
//!   put on one clip three hundred clips ago was findable only by scrubbing,
//! * the render stages of the last measured frame, which is the one fact that
//!   says whether a slow scrub is decode-bound, raster-bound, or composite-bound.
//!
//! Every row comes from a catalog: [`aria_models::project_facts`],
//! [`aria_models::clip_facts`], and [`aria_engine::frame_stages`]. Nothing here
//! names a fact or counts them, so a row added to any of those tables draws here
//! with no edit. The counts are taken when the report is opened and again on any
//! edit that lands while it is open; the stage timings are recorded with the
//! frame they measure. Drawing this panel asks the session for nothing.
//!
//! A clip-question row is a control, not a readout: clicking it selects and
//! seeks to the next clip that answers it, wrapping at the end, so repeated
//! presses walk every match. A row nothing answers is drawn dim and says so
//! rather than being hidden — "no clip carries a speed ramp" is an answer.
//!
//! It shows itself, like the duck tuner: the panel is registered with
//! `project_report_available()` as its visibility, so an empty project with no
//! sources and no clips does not get a column of zeroes.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::project_report::{ClipFactRow, FrameStageRow, ProjectFactRow};
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let facts = state.read(cx).project_fact_rows();
    let clip_facts = state.read(cx).clip_fact_rows();
    let stages = state.read(cx).frame_stage_rows();
    let total = state.read(cx).frame_total_ms();
    let content = state.read(cx).selected_content_size();
    let markers = state.read(cx).marker_count();

    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface_raised)
        .border_1()
        .border_color(theme.border)
        .child(heading(&theme, "Project"))
        .children(facts.iter().map(|row| fact_row(&theme, row)))
        .child(heading(&theme, "Clips carrying"))
        .children(
            clip_facts
                .iter()
                .copied()
                .map(|row| clip_fact_row(&theme, row, cx)),
        )
        .child(heading(&theme, "Last frame"))
        .children(stages.into_iter().map(|row| stage_row(&theme, row)))
        .child(readout(
            &theme,
            SharedString::new_static("Total"),
            SharedString::from(format!("{total:.2} ms")),
            total == 0.0,
        ));

    // Only a generated clip has content to measure, so the row appears with the
    // selection rather than sitting empty beside a media clip.
    if let Some((width, height)) = content {
        body = body
            .child(heading(&theme, "Selected content"))
            .child(readout(
                &theme,
                SharedString::new_static("Drawn size"),
                SharedString::from(format!("{width} × {height} px")),
                false,
            ));
    }
    // The markers tab counts its own rows; this says the same number beside the
    // rest of the inventory so a report read on its own is complete.
    body = body.child(readout(
        &theme,
        SharedString::new_static("Marker rows"),
        SharedString::from(markers.to_string()),
        markers == 0,
    ));

    motion::fade_in("project-report", body).into_any_element()
}

fn heading(theme: &Theme, label: &'static str) -> gpui::AnyElement {
    div()
        .pt(px(Theme::SPACE_XS))
        .text_size(px(11.))
        .text_color(theme.text_muted)
        .child(label)
        .into_any_element()
}

/// A label and a value, dimmed when the value is the empty answer.
fn readout(theme: &Theme, label: SharedString, value: SharedString, dim: bool) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .text_size(px(12.))
        .child(div().text_color(theme.text_muted).child(label))
        .child(
            div()
                .text_color(if dim { theme.text_faint } else { theme.text })
                .child(value),
        )
        .into_any_element()
}

fn fact_row(theme: &Theme, row: &ProjectFactRow) -> gpui::AnyElement {
    let empty = row.value == "0" || row.value == "no" || row.value == "none";
    readout(
        theme,
        SharedString::new_static(row.label),
        row.value.clone(),
        empty,
    )
}

fn stage_row(theme: &Theme, row: FrameStageRow) -> gpui::AnyElement {
    // The stage that a smaller render would buy back is marked, because that is
    // what the number is for: a decode-bound frame does not get faster at a
    // lower preview resolution.
    let label = if row.scales_with_output {
        SharedString::from(format!("{} (scales)", row.label))
    } else {
        SharedString::new_static(row.label)
    };
    div()
        .id(crate::motion::MotionKey::new("stage", row.id).element_id("stage"))
        .flex()
        .flex_col()
        .child(readout(
            theme,
            label,
            SharedString::from(format!("{:.2} ms", row.milliseconds)),
            row.milliseconds == 0.0,
        ))
        .child(
            div()
                .text_size(px(10.))
                .text_color(theme.text_faint)
                .child(row.note),
        )
        .into_any_element()
}

/// One per-clip question. Clicking walks to the next clip that answers it.
fn clip_fact_row(
    theme: &Theme,
    row: ClipFactRow,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("project-report", row.id);
    let empty = row.count == 0;
    let fact_id = row.id;
    div()
        .id(crate::motion::MotionKey::new("fact", row.id).element_id("fact"))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(12.))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .when(!empty, |el| {
            el.cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.go_to_clip_with_fact(fact_id));
                        cx.notify();
                    }),
                )
        })
        .child(
            div()
                .text_color(if empty { theme.text_faint } else { theme.text })
                .child(SharedString::new_static(row.label)),
        )
        .child(
            div()
                .text_color(if empty {
                    theme.text_faint
                } else {
                    theme.accent
                })
                .child(if empty {
                    SharedString::new_static("none")
                } else {
                    SharedString::from(row.count.to_string())
                }),
        )
        .into_any_element()
}
