//! The Analysis tab: run a detector, read what it found, act on it.
//!
//! The panel starts nothing and polls nothing. Each detector's button hands the selection to
//! [`crate::app::AppState::analyze_selected`], which owns the decode, the job, and the
//! index write; the rows below are what the index holds for the selected media, already
//! filtered and ordered by the crate that owns the query.
//!
//! A stale row is labelled rather than hidden: the analyzer that wrote it has moved on,
//! and the row offers both a re-run and a clear.

use aria_core::count::counted;
use gpui::prelude::FluentBuilder;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::analysis::{AnalysisKind, CONFIDENCE_CHOICES};
use crate::components;
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
    let status = if !selected {
        "Select a clip, then detect".to_string()
    } else if total == 0 {
        "Nothing detected for this media yet".to_string()
    } else {
        counted(total, "moment", "moments").to_string()
    };

    // The steps through the rows, then the edits the rows offer, on one line
    // of ghost buttons hung one inset left so their labels start on the
    // column's edge.
    let mut acts = Vec::with_capacity(4);
    if total > 0 {
        acts.push(pressing(
            components::ghost_button("analysis-prev", "Previous", theme),
            Pick::SeekPrevious,
            cx,
        ));
        acts.push(pressing(
            components::ghost_button("analysis-next", "Next", theme),
            Pick::SeekNext,
            cx,
        ));
    }
    if shots > 0 {
        acts.push(pressing(
            components::ghost_button("analysis-split-shots", "Split at shots", theme),
            Pick::SplitAtShots,
            cx,
        ));
    }
    if silence > 0 {
        acts.push(pressing(
            components::ghost_button("analysis-cut-silence", "Ripple-delete silence", theme),
            Pick::CutSilence,
            cx,
        ));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(components::form_row(
            theme,
            "Detect",
            AnalysisKind::ALL
                .iter()
                .zip(stored)
                .map(|(kind, stored)| detect_button(theme, *kind, stored, selected, cx)),
        ))
        .child(components::form_row(
            theme,
            "Loudness",
            std::iter::once(measure_button(theme, selected && peak.is_none(), cx)).chain(peak.map(
                |dbfs| {
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(if clipping {
                            theme.danger
                        } else {
                            theme.text_muted
                        })
                        .child(crate::app::analysis::peak_line(dbfs, clipping))
                        .into_any_element()
                },
            )),
        ))
        .child(components::form_row(
            theme,
            "Show",
            std::iter::once(None)
                .chain(AnalysisKind::ALL.iter().copied().map(Some))
                .map(|kind| kind_chip(theme, kind, kind_filter, cx)),
        ))
        .child(components::form_row(
            theme,
            "Confidence",
            CONFIDENCE_CHOICES
                .iter()
                .map(|choice| confidence_chip(theme, *choice, minimum, cx)),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .pt(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(status)),
                )
                .child(
                    pressing(
                        components::ghost_button("analysis-refresh", "Refresh", theme),
                        Pick::Refresh,
                        cx,
                    )
                    .mr(px(-components::GHOST_INSET.0)),
                ),
        )
        .when(!acts.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(Theme::SPACE_XXS))
                    .ml(px(-components::GHOST_INSET.0))
                    .children(acts),
            )
        })
        .children(moment_rows(theme, state, cx))
        .children(jobs)
        .into_any_element()
}

/// `button`, pressing `pick` on a click.
fn pressing(
    button: gpui::Stateful<gpui::Div>,
    pick: Pick,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Stateful<gpui::Div> {
    button.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| press(state, pick));
            cx.notify();
        }),
    )
}

/// The loudness measurement: solid while a clip is selected and has no
/// contour, a ghost otherwise, as a detector's button is.
fn measure_button(
    theme: &Theme,
    offered: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let button = if offered {
        components::button("analysis-loudness", "Measure", theme)
    } else {
        components::ghost_button("analysis-loudness", "Measure", theme)
    };
    button
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, cx| {
                    if let Err(err) = state.measure_loudness(cx) {
                        state.report_error(err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

/// One detector: run it, and clear what it already stored.
///
/// While a clip is selected and the detector has nothing stored, the run gets
/// the solid button, because running it is the action the panel is offering.
/// With no clip selected, or once it holds results, the same control is a
/// ghost, and a stored detector has the clear beside it. A clear with nothing
/// to clear is not drawn at all.
fn detect_button(
    theme: &Theme,
    kind: AnalysisKind,
    stored: bool,
    selected: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let label = SharedString::from(kind.label());
    let run = if selected && !stored {
        components::button(("analysis-run", kind.id()), label, theme)
    } else {
        components::ghost_button(("analysis-run", kind.id()), label, theme)
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
