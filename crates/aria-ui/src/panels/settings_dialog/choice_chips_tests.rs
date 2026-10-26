//! Every Settings choice is a chip that sets the choice it names, and a press
//! reports the choice by the name the dialog prints.
//!
//! The class closed here: a choice row drawn with a solid button for the
//! choice in force, so the choice read as an action, filled as the Done button
//! under it is; and a press that reported the persisted key, `AI protocol is
//! chat_completions`, for a chip that reads Chat Completions. The choices are
//! read from each setting's own list, so a new source, protocol, reasoning
//! mode, autonomy level or accent is pressed without an edit here.
//!
//! Not caught: the chips' colors, which the test renderer does not raster. A
//! chip is told from a button by the hover it lights, which is keyed by the
//! kind of control drawn.

use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary, ThemeChoice};
use gpui::{Modifiers, TestAppContext, TextRunLayout, VisualTestContext, px, size};

use crate::app::{AppState, DialogKind};
use crate::motion::{ControlId, MotionKey, hover_t};
use crate::settings::MIN_WINDOW_WIDTH;
use crate::test_support::drawn_shell;

/// One choice to press.
struct Choice {
    /// The label of the row the choice stands on, or `None` for a row that
    /// is itself the choice.
    row: Option<&'static str>,
    /// The text the choice shows.
    shown: String,
    /// The hover key of the chip the choice is drawn as, if it is a chip.
    chip: Option<MotionKey>,
    /// The status line a press leaves.
    status: String,
    /// Whether the state holds the choice.
    holds: Box<dyn Fn(&AppState) -> bool>,
}

/// Every choice of one AI chip row, last first, so each press changes the
/// value from the one before it.
fn ai_row<T: Copy + PartialEq + 'static>(
    out: &mut Vec<Choice>,
    (row, prefix, status): (&'static str, &'static str, &'static str),
    all: &[T],
    label: fn(T) -> &'static str,
    read: fn(&AppState) -> T,
) {
    for &choice in all.iter().rev() {
        let shown = label(choice);
        out.push(Choice {
            row: Some(row),
            shown: shown.to_owned(),
            chip: Some(ControlId::from(MotionKey::new(prefix, shown)).motion("chip")),
            status: format!("{status}: {shown}"),
            holds: Box::new(move |state| read(state) == choice),
        });
    }
}

/// The two chips of an On/Off row, the one not in force first.
fn on_off_row(
    out: &mut Vec<Choice>,
    (row, id): (&'static str, &'static str),
    now: bool,
    read: fn(&AppState) -> bool,
) {
    for on in [!now, now] {
        let shown = if on { "On" } else { "Off" };
        out.push(Choice {
            row: Some(row),
            shown: shown.to_owned(),
            chip: Some(ControlId::from(ControlId::from(id).motion(shown)).motion("chip")),
            status: format!("{row}: {shown}"),
            holds: Box::new(move |state| read(state) == on),
        });
    }
}

/// The text run of `choice` inside `card`: on the line of a run that reads
/// `row` and right of it, or the topmost run that reads `choice` when it has
/// no row.
fn locate<'a>(
    runs: &'a [TextRunLayout],
    card: gpui::Bounds<gpui::Pixels>,
    choice: &Choice,
) -> &'a TextRunLayout {
    let inside = |run: &&TextRunLayout| card.contains(&run.bounds.center());
    let reads = |text: &str| {
        let text = text.to_owned();
        move |run: &&TextRunLayout| run.text.as_ref() == text
    };
    let found = match choice.row {
        None => runs
            .iter()
            .filter(inside)
            .filter(reads(&choice.shown))
            .min_by(|a, b| {
                a.bounds
                    .top()
                    .partial_cmp(&b.bounds.top())
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        Some(row) => runs
            .iter()
            .filter(inside)
            .filter(reads(row))
            .find_map(|label| {
                runs.iter()
                    .filter(inside)
                    .filter(reads(&choice.shown))
                    .find(|run| {
                        (run.bounds.center().y - label.bounds.center().y).abs() < px(1.0)
                            && run.bounds.left() > label.bounds.right()
                    })
            }),
    };
    found.unwrap_or_else(|| {
        panic!(
            "Settings draws {} on the {:?} row",
            choice.shown, choice.row
        )
    })
}

/// Settings open in a window tall enough to draw every section unscrolled.
fn open_settings(cx: &mut TestAppContext) -> (gpui::Entity<AppState>, &mut VisualTestContext) {
    let (state, visual) = drawn_shell(cx, "settings-choice-chips");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(3600.0)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    visual.run_until_parked();
    (state, visual)
}

#[gpui::test]
fn every_settings_choice_is_a_chip_that_sets_the_choice_it_names(cx: &mut TestAppContext) {
    let (state, visual) = open_settings(cx);
    let (scrubbing, logging) = visual.read(|app| {
        let settings = &state.read(app).ui_settings;
        (settings.audio_scrubbing, settings.log_dropped_frames)
    });

    let mut choices = Vec::new();
    ai_row(
        &mut choices,
        ("Source", "ai-source", "AI source"),
        &AiSource::ALL,
        AiSource::label,
        |state| state.cloud_settings().ai.source,
    );
    ai_row(
        &mut choices,
        ("Protocol", "ai-protocol", "AI protocol"),
        &AiApiProtocol::ALL,
        AiApiProtocol::label,
        |state| state.cloud_settings().ai.api_protocol,
    );
    ai_row(
        &mut choices,
        ("Reasoning", "ai-reasoning", "AI reasoning"),
        &ReasoningSummary::ALL,
        ReasoningSummary::label,
        |state| state.cloud_settings().ai.reasoning_summary,
    );
    ai_row(
        &mut choices,
        ("Autonomy", "ai-autonomy", "AI autonomy"),
        &Autonomy::ALL,
        Autonomy::label,
        |state| state.cloud_settings().ai.autonomy,
    );
    on_off_row(
        &mut choices,
        ("Audio scrubbing", "settings-audio-scrubbing"),
        scrubbing,
        |state| state.ui_settings.audio_scrubbing,
    );
    on_off_row(
        &mut choices,
        ("Log dropped frames", "settings-log-dropped-frames"),
        logging,
        |state| state.ui_settings.log_dropped_frames,
    );
    for &accent in ThemeChoice::ALL.iter().rev() {
        let shown = format!("Accent: {}", accent.label());
        choices.push(Choice {
            row: None,
            status: shown.clone(),
            shown,
            chip: None,
            holds: Box::new(move |state| state.theme_choice() == accent),
        });
    }

    let mut faults = Vec::new();
    for choice in &choices {
        let card = visual
            .debug_bounds("dialog-card")
            .expect("the Settings dialog draws a card");
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        let at = locate(&runs, card, choice).bounds.center();
        let name = format!("{:?} {}", choice.row, choice.shown);
        visual.simulate_mouse_move(at, None, Modifiers::default());
        visual.run_until_parked();
        if let Some(chip) = choice.chip
            && !(hover_t(chip) > 0.0)
        {
            faults.push(format!("{name} is not drawn as a chip"));
        }
        visual.simulate_click(at, Modifiers::default());
        visual.run_until_parked();
        visual.read(|app| {
            let state = state.read(app);
            if !(choice.holds)(state) {
                faults.push(format!("pressing {name} did not set it"));
            }
            if state.status_message.as_deref() != Some(choice.status.as_str()) {
                faults.push(format!(
                    "pressing {name} reported {:?}, not {:?}",
                    state.status_message, choice.status
                ));
            }
            if let Some(error) = &state.last_error {
                faults.push(format!("pressing {name} failed: {error}"));
            }
            if state.dialog != DialogKind::Settings {
                faults.push(format!("pressing {name} closed Settings"));
            }
        });
    }
    assert!(
        faults.is_empty(),
        "Settings choice faults:\n  {}",
        faults.join("\n  ")
    );
}
