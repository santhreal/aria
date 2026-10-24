//! Media support: what this build decodes, and what the selection is made of.
//!
//! The panel for [`crate::app::media_reach`]. It shows itself once a report has
//! been built, so the inspector column is not carrying a decoder listing during
//! ordinary editing: the palette row builds the report, the panel appears with
//! it, and the actions that need a whole-file decode are buttons here rather
//! than palette rows, because a job started from a panel is started next to the
//! answer it produces.
//!
//! Nothing in this file names a container, a pixel layout, an output mode or a
//! section heading. The rows come from the report and the headings come from the
//! rows, so a catalog row added anywhere upstream draws itself.

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::media_reach::{Exposure, MediaReachForm};
use crate::components::{ghost_button, readout, readout_tinted, section_label};
use crate::motion;
use crate::theme::Theme;

/// One row as the panel draws it, flattened out of the report so the state
/// borrow ends before the elements are built.
struct Row {
    section: &'static str,
    label: SharedString,
    value: SharedString,
    warn: bool,
}

/// One button and the verb it runs.
struct Action {
    id: &'static str,
    label: &'static str,
    run: fn(&mut AppState),
}

/// The actions the panel offers, in order of use.
///
/// A table rather than four hand-built buttons: each row is one verb, and the
/// only thing a new capability needs here is a row.
const ACTIONS: &[Action] = &[
    Action {
        id: "media-refresh",
        label: "Re-read",
        run: AppState::media_support_report,
    },
    Action {
        id: "media-verify",
        label: "Verify decode",
        run: AppState::verify_selected_decode,
    },
    Action {
        id: "media-loudness",
        label: "Loudness overview",
        run: AppState::file_loudness_overview,
    },
    Action {
        id: "media-exposure",
        label: "Measure exposure",
        run: AppState::measure_exposure,
    },
    Action {
        id: "media-normalise",
        label: "Normalise selection",
        run: AppState::normalise_selected_clip,
    },
];

/// Every id in [`ACTIONS`], for the controls census.
#[cfg(test)]
pub(crate) fn action_ids() -> impl Iterator<Item = &'static str> {
    ACTIONS.iter().map(|action| action.id)
}

/// Whether the panel has anything to draw.
pub(crate) fn has_report(state: &AppState) -> bool {
    state.media_reach.report.is_some()
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let (heading, rows, sections, target, target_at, exposure) = {
        let app = state.read(cx);
        let report = app.media_reach.report.as_ref();
        let warnings = report.map_or(0, |report| report.warnings());
        let count = report.map_or(0, |report| report.lines.len());
        (
            if warnings == 0 {
                format!("Media support: {}", counted(count, "fact", "facts"))
            } else {
                format!("Media support: {warnings} of {count} to look at")
            },
            report
                .map(|report| {
                    report
                        .lines
                        .iter()
                        .map(|line| Row {
                            section: line.section,
                            label: line.label.clone(),
                            value: line.value.clone(),
                            warn: line.warn,
                        })
                        .collect::<Vec<Row>>()
                })
                .unwrap_or_default(),
            report.map(|report| report.sections()).unwrap_or_default(),
            app.media_reach.target_label(),
            app.media_reach.target_at(),
            app.media_reach.exposure,
        )
    };

    let mut blocks: Vec<gpui::AnyElement> = Vec::new();
    for section in sections {
        blocks.push(section_label(&theme, section).into_any_element());
        for row in rows.iter().filter(|row| row.section == section) {
            blocks.push(fact(&theme, row));
        }
    }
    if let Some(exposure) = exposure {
        blocks.push(exposure_block(&theme, exposure));
    }

    motion::fade_in(
        "media-support-panel",
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
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(heading)),
            )
            .child(target_chip(&theme, target, target_at, cx))
            .children(blocks)
            .child(action_bar(&theme, cx)),
    )
    .into_any_element()
}

/// One label/value row. A row that states a problem carries the warning colour
/// on its value, which is the part that states what is wrong.
fn fact(theme: &Theme, row: &Row) -> gpui::AnyElement {
    let tint = if row.warn { theme.warning } else { theme.text };
    readout_tinted(theme, row.label.clone(), row.value.clone(), tint).into_any_element()
}

/// The normalise target, cycled by clicking. Dots come from the form's own
/// table, so a level added there shows up here with no edit.
fn target_chip(
    theme: &Theme,
    value: String,
    at: Option<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("media-support", "target");
    let background = motion::hover_blend(key, theme.input_bg, theme.surface_raised_hover);
    div()
        .id("media-target")
        .flex()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(background)
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.cycle_normalise_target());
                cx.notify();
            }),
        )
        .child(div().text_color(theme.text_muted).child("Normalise to"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(div().child(SharedString::from(value)))
                .child(dots(theme, at, MediaReachForm::TARGETS.len())),
        )
        .into_any_element()
}

/// One dot per level, the live one filled.
fn dots(theme: &Theme, at: Option<usize>, count: usize) -> gpui::AnyElement {
    let mut row = div().flex().items_center().gap(px(3.));
    for index in 0..count {
        let live = at == Some(index);
        row = row.child(div().w(px(5.)).h(px(5.)).rounded(px(2.5)).bg(if live {
            theme.accent
        } else {
            theme.border
        }));
    }
    row.into_any_element()
}

/// The exposure readout: the mean against neutral, where the extremes are, and
/// how far off neutral the average colour sits.
fn exposure_block(theme: &Theme, exposure: Exposure) -> gpui::AnyElement {
    let neutral = f32::from(exposure.neutral);
    let offset = exposure.mean_luma - neutral;
    let (cb, cr) = exposure.cast;
    let facts = [
        (
            "Mean luma",
            format!(
                "{:.0} against {neutral:.0} neutral ({offset:+.0})",
                exposure.mean_luma
            ),
        ),
        (
            "Darkest",
            format!(
                "{:.0} at {:.0}% across",
                exposure.darkest.1,
                exposure.darkest.0 * 100.0
            ),
        ),
        (
            "Brightest",
            format!(
                "{:.0} at {:.0}% across",
                exposure.brightest.1,
                exposure.brightest.0 * 100.0
            ),
        ),
        (
            "Cast",
            format!("Cb {cb:+.3}, Cr {cr:+.3} on the Cb/Cr plane"),
        ),
        (
            "Read from",
            format!(
                "{} px at {:.2}s",
                exposure.sampled, exposure.timestamp_seconds
            ),
        ),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Exposure"))
        .children(
            facts
                .into_iter()
                .map(|(label, value)| readout(theme, label, value)),
        )
        .into_any_element()
}

/// The buttons, built from [`ACTIONS`].
fn action_bar(theme: &Theme, cx: &mut gpui::Context<crate::shell::Shell>) -> gpui::AnyElement {
    let mut bar = div().flex().wraps().items_center().gap(px(Theme::SPACE_XS));
    for action in ACTIONS {
        let run = action.run;
        bar = bar.child(ghost_button(action.id, action.label, theme).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| run(state));
                cx.notify();
            }),
        ));
    }
    bar.into_any_element()
}
