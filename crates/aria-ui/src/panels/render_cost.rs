//! What the last rendered frame cost, split by stage.
//!
//! `FrameStats` splits a render into decode, raster, and composite, and names
//! the part that shrinks when the preview renders smaller. The engine has always
//! published it and no surface read it, so "playback is slow" had no answer
//! inside the app: the operator could see a dropped frame and not whether it was
//! the file, the text layout, or the GPU.
//!
//! The rows are `UiRenderStats::rows`, which is the one owner of what the split
//! contains — the status line prints the same five numbers from the same call,
//! so the panel and the strip cannot drift.
//!
//! It shows itself, like the duck tuner: registered with `measured()` as its
//! visibility, so it appears once a frame has actually been rendered and is
//! absent on a session that has drawn nothing. A zeroed split is "nothing has
//! been measured", never "a frame was free", and drawing five zeroes would say
//! the second thing.
//!
//! The playback line under the split answers the other half of "playback is
//! slow": the split says where a frame's cost went, and
//! [`AppState::playback_report`] says whether the run kept the sequence's rate.
//! It appears only once playback has delivered a frame, because a paused editor
//! has no rate to miss.

use gpui::{Entity, IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::app::AppState;
use crate::theme::Theme;

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let stats = state.read(cx).last_render_stats();
    // The share each stage takes of the frame, so the bars compare against the
    // frame rather than against a fixed millisecond scale that a slow machine
    // would peg and a fast one would leave empty.
    let frame = stats.total_ms.max(f64::EPSILON);

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface_raised)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child("Last frame"),
        )
        .children(
            stats
                .rows()
                .into_iter()
                .map(|(name, ms)| row(&theme, name, ms, ms / frame)),
        )
        .children(passes(&theme, &stats))
        .children(playback(&theme, state.read(cx).playback_report()))
        .into_any_element()
}

/// The effect-pass line, or nothing at all.
///
/// Absent on a frame with no effect pass, like the playback line: a scene
/// without effects has no pass count to answer for. What it answers is "why is
/// this frame dear" — a chain the operator stacked is drawn once per layer, and
/// the second number says how much of that the compositor had to build rather
/// than reuse.
fn passes(
    theme: &Theme,
    stats: &crate::app::render_reach::UiRenderStats,
) -> Option<gpui::AnyElement> {
    let line = stats.passes_line()?;
    Some(
        div()
            .pt(px(Theme::SPACE_XS))
            .text_size(px(11.))
            .text_color(theme.text_muted)
            .child(SharedString::from(line))
            .into_any_element(),
    )
}

/// The playback line, or nothing at all.
///
/// `None` while no frame has been delivered under playback: an empty run is not
/// a slow one, and a line reading "0.0 of 24 fps" on a paused editor would be
/// read as a failure.
///
/// The colour is the verdict, not decoration: green while the run holds the
/// sequence's rate, amber once it does not, because a rate the operator has to
/// compare against a target himself is a number he will not read.
fn playback(theme: &Theme, report: aria_render::PlaybackReport) -> Option<gpui::AnyElement> {
    if report.is_empty() {
        return None;
    }
    // The same tolerance the measured run allows: one descheduled frame in
    // twenty is the machine, not the editor.
    let keeping_up = report.keeps_up(0.05);
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .pt(px(Theme::SPACE_XS))
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(Theme::SPACE_SM))
                    .text_size(px(13.))
                    .child(div().text_color(theme.text_muted).child("Playback"))
                    .child(
                        div()
                            .text_color(if keeping_up {
                                theme.success
                            } else {
                                theme.warning
                            })
                            .child(SharedString::from(format!(
                                "{:.1} of {:.0} fps",
                                report.achieved_fps, report.target_fps
                            ))),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(SharedString::from(format!(
                        "{} of {} frames late, worst {:.1} ms",
                        report.late, report.frames, report.worst_ms
                    ))),
            )
            .into_any_element(),
    )
}

fn row(theme: &Theme, name: &'static str, ms: f64, share: f64) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_SM))
                .text_size(px(13.))
                .text_color(theme.text)
                .child(div().text_color(theme.text_muted).child(name))
                .child(SharedString::from(format!("{ms:.1} ms"))),
        )
        .child(bar(theme, share))
        .into_any_element()
}

/// One stage's share of the frame. Clamped rather than trusted: the scaled-cost
/// row is a subset of the frame and the total is the frame itself, so a share of
/// one is the widest a bar can honestly be.
fn bar(theme: &Theme, share: f64) -> gpui::AnyElement {
    let filled = (share.clamp(0.0, 1.0) * 100.0) as f32;
    div()
        .w_full()
        .h(px(3.))
        .rounded(px(2.))
        .bg(theme.border)
        .child(
            div()
                .h(px(3.))
                .w(gpui::relative(filled / 100.0))
                .rounded(px(2.))
                .bg(theme.accent),
        )
        .into_any_element()
}
