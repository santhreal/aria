//! The Captions tab: a language, a granularity, a run, and what the transcript becomes.
//!
//! The panel starts nothing and polls nothing. Transcribe hands the selection to
//! [`crate::app::AppState::transcribe_selected`], which owns the decode, the job, and
//! the projection; the rows below are the same job projection the Jobs board draws,
//! filtered to transcription, so a running run has one view in the app rather than two.
//!
//! With no model installed the panel says where to install one instead of offering a
//! button whose only outcome is that message.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::captions::{CaptionCommit, CaptionsForm, THREAD_CHOICES};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;

/// How much of the panel the job list may take before the transcript below it
/// starts scrolling out of reach.
const JOB_LIMIT: usize = 4;

/// The panel's claim on the shared job list.
///
/// A model download is the panel's work too: it is the wait that stands
/// between pressing Transcribe and anything happening, so it belongs in the
/// same list rather than only in the Jobs board.
fn mine(job: &crate::app::jobs::UiJob) -> bool {
    matches!(
        job.kind,
        crate::app::jobs::JobKind::Transcription | crate::app::jobs::JobKind::Download
    )
}

/// The language box's text, with the caret when it has focus.
///
/// A code is one or two characters, so this is the panel's one small format per
/// frame rather than a copy of anything the store holds.
fn language_label(form: &CaptionsForm) -> SharedString {
    if form.language.is_empty() {
        SharedString::new_static("Language: detect (click to set, e.g. en)")
    } else if form.language_focused {
        format!("Language: {}|", form.language).into()
    } else {
        format!("Language: {}", form.language).into()
    }
}

pub(crate) fn captions_panel(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // The controls, not the transcript: the form carries the segments and their
    // words, and the rows below read those where they already are.
    let (language, focused, word_level, threads, commit, units, installed) = {
        let state = state.read(cx);
        (
            language_label(&state.captions),
            state.captions.language_focused,
            state.captions.word_level,
            state.captions.threads,
            state.captions.commit,
            state.caption_unit_count(),
            state.installed_model().is_some(),
        )
    };
    let rows = crate::pending::job_rows(theme, state, JOB_LIMIT, mine, cx);

    let border = if focused { theme.accent } else { theme.border };

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .id("caption-language")
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_raised)
                .border_1()
                .border_color(border)
                .cursor(CursorStyle::IBeam)
                .text_size(Theme::TEXT_TITLE)
                .text_color(if language.is_empty() {
                    theme.text_muted
                } else {
                    theme.text
                })
                .child(language)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.focus_caption_language(true));
                        cx.notify();
                    }),
                ),
        )
        .child(
            div().flex().gap(px(Theme::SPACE_XS)).children(
                THREAD_CHOICES
                    .iter()
                    .map(|threads_choice| thread_chip(theme, *threads_choice, threads, cx)),
            ),
        )
        .child(
            div().flex().gap(px(Theme::SPACE_XS)).children(
                CaptionCommit::ALL
                    .iter()
                    .map(|(target, id, label)| commit_chip(theme, *target, id, label, commit, cx)),
            ),
        )
        .child(word_toggle(theme, word_level, cx))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(if !installed {
                    "Install a transcription model in Settings to caption".to_string()
                } else if focused {
                    "Esc leaves the box".to_string()
                } else if units == 0 {
                    "Select a clip with audio, then transcribe".to_string()
                } else {
                    format!("{units} caption(s) ready to commit")
                })),
        )
        .when(installed, |el| {
            el.child(
                components::ghost_button("caption-run", "Transcribe", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            if let Err(err) = state.transcribe_selected(cx) {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
        .when(units > 0, |el| {
            el.child(
                components::ghost_button("caption-commit", "Commit", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.commit_captions() {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
        .children(segment_rows(theme, state, cx))
        .children(rows)
        .into_any_element()
}

/// Confidence bands, in ascending order of the exclusive upper bound they own.
///
/// One table so a band edit is one row. The last entry is the ceiling and must
/// stay at `1.0`: `band_for` walks until a bound is not exceeded, and a
/// probability is never above one.
const CONFIDENCE_BANDS: &[(f32, &str)] = &[(0.5, "low"), (0.8, "fair"), (1.0, "good")];

/// The band a probability falls in, plus the colour that names it.
fn band_for(theme: &Theme, value: f32) -> (&'static str, gpui::Hsla) {
    let colors = [theme.danger, theme.warning, theme.success];
    CONFIDENCE_BANDS
        .iter()
        .zip(colors)
        .find(|((bound, _), _)| value < *bound)
        .map_or(("good", theme.success), |((_, name), color)| (*name, color))
}

/// The model's certainty, as a percentage that scans down the column.
///
/// Shown because a transcript that reads plausibly and was guessed at is the
/// expensive failure: it survives review and ships. The number here is the same
/// mean the moments index stores for the segment.
fn confidence_chip(theme: &Theme, value: f32) -> gpui::AnyElement {
    let (name, color) = band_for(theme, value);
    let percent = (value * 100.).round() as i32;
    div()
        .text_size(Theme::TEXT_SMALL)
        .text_color(color)
        .child(SharedString::from(format!("{percent}% {name}")))
        .into_any_element()
}

/// The transcript itself, showing what will be committed.
fn segment_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rate = state.read(cx).playhead.rate;
    state
        .read(cx)
        .captions
        .segments
        .iter()
        .take(20)
        .map(|segment| {
            // The weakest word is where a re-record or a manual fix goes. A
            // segment mean of 80% hides one word the model never heard.
            let weakest = segment
                .words
                .iter()
                .min_by(|a, b| a.confidence.total_cmp(&b.confidence));
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
                        .flex()
                        .items_center()
                        .gap(px(Theme::SPACE_SM))
                        .child(
                            div()
                                .text_size(Theme::TEXT_SMALL)
                                .text_color(theme.text_muted)
                                .child(SharedString::from(crate::timecode::format_ticks(
                                    segment.start_tick,
                                    rate.num,
                                    rate.den,
                                ))),
                        )
                        .children(
                            segment
                                .confidence
                                .map(|value| confidence_chip(theme, value)),
                        ),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(segment.text.clone()),
                )
                .children(weakest.map(|word| {
                    let (_, color) = band_for(theme, word.confidence);
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(color)
                        .child(SharedString::from(format!(
                            "weakest: {} {}%",
                            word.text,
                            (word.confidence * 100.).round() as i32
                        )))
                        .into_any_element()
                }))
                .into_any_element()
        })
        .collect()
}

fn thread_chip(
    theme: &Theme,
    target: usize,
    current: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let active = target == current;
    chip(
        theme,
        crate::motion::MotionKey::new("caption-threads", target).into(),
        SharedString::from(format!("{target} thread(s)")),
        active,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_threads(target));
            cx.notify();
        }),
    )
}

fn commit_chip(
    theme: &Theme,
    target: CaptionCommit,
    id: &'static str,
    label: &'static str,
    current: CaptionCommit,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    chip(
        theme,
        crate::motion::MotionKey::new("caption-commit", id).into(),
        SharedString::from(label),
        target == current,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_commit(target));
            cx.notify();
        }),
    )
}

fn word_toggle(
    theme: &Theme,
    word_level: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    chip(
        theme,
        crate::motion::MotionKey::new("caption", "words").into(),
        SharedString::from("One clip per word"),
        word_level,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_word_level(!word_level));
            cx.notify();
        }),
    )
}

/// One selectable chip. The look belongs to [`crate::components::chip`], which
/// also gives it the eased hover every other control in the app has; this
/// carries only the click.
fn chip(
    theme: &Theme,
    id: crate::motion::ControlId,
    label: SharedString,
    active: bool,
    on_click: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    crate::components::chip(
        id,
        label,
        active,
        crate::components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(MouseButton::Left, on_click)
    .into_any_element()
}
