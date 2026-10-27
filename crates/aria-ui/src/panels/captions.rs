//! The Captions tab: a language, a granularity, a run, and what the transcript becomes.
//!
//! The panel starts nothing and polls nothing. Transcribe hands the selection to
//! [`crate::app::AppState::transcribe_selected`], which owns the decode, the job, and
//! the projection; the rows below are the same job projection the Jobs board draws,
//! filtered to transcription, so a running run has one view in the app rather than two.
//!
//! With no model installed the panel offers the Settings section that installs one
//! in place of Transcribe.

use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::captions::{CaptionCommit, THREAD_CHOICES};
use crate::components;
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

/// The threads chips' counts, in order: **Auto** (zero, the decoder's default
/// for the machine), then each count in [`THREAD_CHOICES`].
pub(super) fn thread_offers() -> impl Iterator<Item = usize> {
    std::iter::once(0).chain(THREAD_CHOICES)
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
            state.captions.language.clone(),
            state.captions.language_focused,
            state.captions.word_level,
            state.captions.threads,
            state.captions.commit,
            state.caption_unit_count(),
            state.installed_model().is_some(),
        )
    };
    let rows = crate::pending::job_rows(theme, state, JOB_LIMIT, mine, cx);
    let status = if !installed {
        "Install a transcription model in Settings to caption".to_string()
    } else if focused {
        "Esc leaves the box".to_string()
    } else if units == 0 {
        "Select a clip with audio, then transcribe".to_string()
    } else {
        format!("{} ready to commit", counted(units, "caption", "captions"))
    };

    // The actions in force, as solid buttons on one row under the line that
    // states what they act on. With no model the only action is the Settings
    // section that installs one.
    let mut actions: Vec<gpui::AnyElement> = Vec::with_capacity(2);
    if installed {
        actions.push(
            components::button("caption-run", "Transcribe", theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            if let Err(err) = state.transcribe_selected(cx) {
                                state.report_error(err);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    } else {
        actions.push(
            components::button("caption-models", "Open Settings", theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.open_settings_at("Transcription"));
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    }
    if units > 0 {
        actions.push(
            components::button("caption-commit", "Commit", theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.commit_captions() {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(components::form_row(
            theme,
            "Language",
            [components::text_box(
                "caption-language",
                language,
                "Detect, or a code such as en",
                focused,
                theme,
            )
            .flex_1()
            .min_w_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.focus_caption_language(true));
                    cx.notify();
                }),
            )
            .into_any_element()],
        ))
        .child(components::form_row(
            theme,
            "Output",
            CaptionCommit::ALL
                .iter()
                .map(|(target, id, label)| commit_chip(theme, *target, id, label, commit, cx)),
        ))
        .child(components::form_row(
            theme,
            "Split",
            [word_toggle(theme, word_level, cx)],
        ))
        .child(components::form_row(
            theme,
            "Threads",
            thread_offers().map(|choice| thread_chip(theme, choice, threads, cx)),
        ))
        .child(
            div()
                .pt(px(Theme::SPACE_XS))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(status)),
        )
        .child(div().flex().gap(px(Theme::SPACE_XS)).children(actions))
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

/// The chip that sets `target` decoder threads, lit while `current` holds it.
/// Zero is **Auto**, the decoder's default for the machine.
fn thread_chip(
    theme: &Theme,
    target: usize,
    current: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let label = match target {
        0 => SharedString::from("Auto"),
        count => SharedString::from(count.to_string()),
    };
    components::chip(
        crate::motion::MotionKey::new("caption-threads", target),
        label,
        target == current,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_threads(target));
            cx.notify();
        }),
    )
    .into_any_element()
}

fn commit_chip(
    theme: &Theme,
    target: CaptionCommit,
    id: &'static str,
    label: &'static str,
    current: CaptionCommit,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    components::chip(
        crate::motion::MotionKey::new("caption-target", id),
        SharedString::from(label),
        target == current,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_commit(target));
            cx.notify();
        }),
    )
    .into_any_element()
}

fn word_toggle(
    theme: &Theme,
    word_level: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    components::chip(
        "caption-words",
        SharedString::from("One clip per word"),
        word_level,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_caption_word_level(!word_level));
            cx.notify();
        }),
    )
    .into_any_element()
}
