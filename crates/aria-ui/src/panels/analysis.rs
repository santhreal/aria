//! The Analysis tab: run a detector, read what it found, act on it.
//!
//! The panel starts nothing and polls nothing. Each Detect hands the selection to
//! [`crate::app::AppState::analyze_selected`], which owns the decode, the job, and the
//! index write; the rows below are what the index holds for the selected media, already
//! filtered and ordered by the crate that owns the query.
//!
//! A stale row is labelled rather than hidden: the analyzer that wrote it has moved on,
//! and the row offers both a re-run and a clear.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::analysis::{AnalysisKind, CONFIDENCE_CHOICES};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::analysis_pick::{self, Pick};
use crate::theme::Theme;

/// Press one row of the registry, reporting a refusal in the status line rather
/// than swallowing it.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = analysis_pick::apply(state, pick) {
        state.report_error(err);
    }
}

/// How much of the panel the job list may take before the results below it
/// start scrolling out of reach.
const JOB_LIMIT: usize = 4;

/// The panel's claim on the shared job list.
///
/// Analysis owns both the detectors and the loudness measurement, and neither
/// keeps a flag of its own — the projection is the only record that the work
/// is running, so the claim is made against the label the job was filed under.
fn mine(job: &crate::app::jobs::UiJob) -> bool {
    matches!(
        job.kind,
        crate::app::jobs::JobKind::Analysis | crate::app::jobs::JobKind::Loudness
    )
}

pub(crate) fn analysis_panel(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // One borrow, scalars out. Nothing below needs the rows themselves — the
    // list is drawn by `moment_rows`, which reads them in place — so cloning the
    // whole form (rows, contour and all) once per frame bought nothing.
    let (kind_filter, minimum, total, shots, silence, selected, peak, clipping, stored) = {
        let state = state.read(cx);
        let count = |kind: &str| {
            state
                .analysis
                .moments
                .iter()
                .filter(|moment| moment.kind == kind)
                .count()
        };
        (
            state.analysis.kind_filter,
            state.analysis.minimum_confidence,
            state.analysis.moments.len(),
            count("shot_boundary"),
            count("silence"),
            state.selected_ui_clip_ref().is_some(),
            state.loudness_peak_dbfs(),
            state.loudness_clips(),
            AnalysisKind::ALL.map(|kind| state.analysis_exists(kind)),
        )
    };
    let jobs = crate::pending::job_rows(theme, state, JOB_LIMIT, mine, cx);

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            div().flex().gap(px(Theme::SPACE_XS)).children(
                AnalysisKind::ALL
                    .iter()
                    .zip(stored)
                    .map(|(kind, stored)| detect_button(theme, *kind, stored, cx)),
            ),
        )
        .child(
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::ghost_button("analysis-loudness", "Loudness", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, cx| {
                                if let Err(err) = state.measure_loudness(cx) {
                                    state.report_error(err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button("analysis-refresh", "Refresh", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| press(state, Pick::Refresh));
                            cx.notify();
                        }),
                    ),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .child(kind_chip(theme, None, kind_filter, cx))
                .children(
                    AnalysisKind::ALL
                        .iter()
                        .map(|kind| kind_chip(theme, Some(*kind), kind_filter, cx)),
                ),
        )
        .child(
            div().flex().gap(px(Theme::SPACE_XS)).children(
                CONFIDENCE_CHOICES
                    .iter()
                    .map(|choice| confidence_chip(theme, *choice, minimum, cx)),
            ),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(if !selected {
                    "Select a clip, then detect".to_string()
                } else if total == 0 {
                    "Nothing detected for this media yet".to_string()
                } else {
                    format!(
                        "{total} moment(s){}",
                        match peak {
                            Some(dbfs) if clipping => format!(" · peak {dbfs:.1} dBFS, clipping"),
                            Some(dbfs) => format!(" · peak {dbfs:.1} dBFS"),
                            None => String::new(),
                        }
                    )
                })),
        )
        .when(shots > 0, |el| {
            el.child(
                components::ghost_button("analysis-split-shots", "Split at shots", theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| press(state, Pick::SplitAtShots));
                            cx.notify();
                        }),
                    ),
            )
        })
        .when(silence > 0, |el| {
            el.child(
                components::ghost_button("analysis-cut-silence", "Ripple-delete silence", theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| press(state, Pick::CutSilence));
                            cx.notify();
                        }),
                    ),
            )
        })
        .when(total > 0, |el| {
            el.child(
                div()
                    .flex()
                    .gap(px(Theme::SPACE_XS))
                    .child(
                        components::ghost_button("analysis-prev", "Previous", theme).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, _| press(state, Pick::SeekPrevious));
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        components::ghost_button("analysis-next", "Next", theme).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, _| press(state, Pick::SeekNext));
                                cx.notify();
                            }),
                        ),
                    ),
            )
        })
        .children(moment_rows(theme, state, cx))
        .children(jobs)
        .into_any_element()
}

/// One detector: run it, and clear what it already stored.
///
/// A detector with nothing stored gets the solid button, because running it is
/// the action the panel is offering; once it holds results the same control
/// steps back to a ghost and the clear appears beside it. A clear with nothing
/// to clear is not drawn at all.
fn detect_button(
    theme: &Theme,
    kind: AnalysisKind,
    stored: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let label = SharedString::from(kind.label());
    let run = if stored {
        components::ghost_button(("analysis-run", kind.id()), label, theme)
    } else {
        components::button(("analysis-run", kind.id()), label, theme)
    };
    div()
        .flex()
        .gap(px(Theme::SPACE_XXS))
        .child(run.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, cx| {
                    if let Err(err) = state.analyze_selected(kind, cx) {
                        state.report_error(err);
                    }
                });
                cx.notify();
            }),
        ))
        .when(stored, |el| {
            el.child(
                components::ghost_button(
                    crate::motion::MotionKey::new("analysis-clear", kind.id()),
                    "×",
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| press(state, Pick::Clear(kind)));
                        cx.notify();
                    }),
                ),
            )
        })
        .into_any_element()
}

/// The rows themselves: what was found, when, how sure, and whether it is current.
fn moment_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rate = state.read(cx).playhead.rate;
    state
        .read(cx)
        .analysis
        .moments
        .iter()
        .take(30)
        .map(|moment| {
            let timecode = crate::timecode::format_ticks(moment.start_tick, rate.num, rate.den);
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XXS))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_raised)
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(format!(
                            "{timecode} · {:.0}%",
                            moment.confidence * 100.0
                        ))),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(if moment.stale {
                            theme.text_muted
                        } else {
                            theme.text
                        })
                        .child(SharedString::from(if moment.stale {
                            format!(
                                "{} · stale (analyzer v{})",
                                moment.kind.replace('_', " "),
                                moment.analyzer_version
                            )
                        } else {
                            moment.kind.replace('_', " ")
                        })),
                )
                .into_any_element()
        })
        .collect()
}

fn kind_chip(
    theme: &Theme,
    target: Option<AnalysisKind>,
    current: Option<AnalysisKind>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = target.map_or("analysis-kind-all", AnalysisKind::id);
    let label = target.map_or("All", AnalysisKind::label);
    components::chip(
        crate::motion::MotionKey::new("analysis-kind", id),
        SharedString::from(label),
        target == current,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| press(state, Pick::Filter(target)));
            cx.notify();
        }),
    )
    .into_any_element()
}

fn confidence_chip(
    theme: &Theme,
    target: f32,
    current: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let percent = (target * 100.0) as i32;
    components::chip(
        crate::motion::MotionKey::new("analysis-confidence", percent),
        SharedString::from(format!("≥{percent}%")),
        (target - current).abs() < f32::EPSILON,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| press(state, Pick::Confidence(target)));
            cx.notify();
        }),
    )
    .into_any_element()
}
