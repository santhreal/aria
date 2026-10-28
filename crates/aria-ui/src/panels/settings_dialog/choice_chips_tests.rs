//! Every Settings choice is a chip that sets the choice it names, every
//! Settings boolean is a switch that flips it, and a press reports the new
//! value by the name the dialog prints.
//!
//! The class closed here: a choice row drawn with a solid button for the
//! choice in force, so the choice read as an action, filled as the Done button
//! under it is; a press that reported the persisted key, `AI protocol is
//! chat_completions`, for a chip that reads Chat Completions; and a boolean
//! drawn as an On chip beside an Off chip, two controls for one bit. The
//! choices are read from each setting's own list, so a new source, protocol,
//! reasoning mode, autonomy level, zoom bar knob hold or accent is pressed
//! without an edit here.
//!
//! Not caught: the controls' colors, which the test renderer does not raster.
//! A chip is told from a button, and a switch from a chip, by the hover it
//! lights, which is keyed by the kind of control drawn.

use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary, ThemeChoice};
use gpui::{Modifiers, TestAppContext, TextRunLayout, VisualTestContext, px, size};

use crate::app::{AppState, DialogKind};
use crate::motion::{ControlId, MotionKey, hover_t};
use crate::settings::MIN_WINDOW_WIDTH;
use crate::test_support::drawn_shell;
use crate::timeline_display::ZoomBarKnobs;

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

/// Every choice of one chip row, last first, so each press changes the value
/// from the one before it.
fn chip_choices<T: Copy + PartialEq + 'static>(
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

/// One Settings switch: the label of its row, its selector, and the setting
/// it flips.
struct Switch {
    row: &'static str,
    id: &'static str,
    read: fn(&AppState) -> bool,
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

    let mut choices = Vec::new();
    chip_choices(
        &mut choices,
        ("Source", "ai-source", "AI source"),
        &AiSource::ALL,
        AiSource::label,
        |state| state.cloud_settings().ai.source,
    );
    chip_choices(
        &mut choices,
        ("Protocol", "ai-protocol", "AI protocol"),
        &AiApiProtocol::ALL,
        AiApiProtocol::label,
        |state| state.cloud_settings().ai.api_protocol,
    );
    chip_choices(
        &mut choices,
        ("Reasoning", "ai-reasoning", "AI reasoning"),
        &ReasoningSummary::ALL,
        ReasoningSummary::label,
        |state| state.cloud_settings().ai.reasoning_summary,
    );
    chip_choices(
        &mut choices,
        ("Autonomy", "ai-autonomy", "AI autonomy"),
        &Autonomy::ALL,
        Autonomy::label,
        |state| state.cloud_settings().ai.autonomy,
    );
    chip_choices(
        &mut choices,
        ("Zoom bar knobs", "settings-zoom-knobs", "Zoom bar knobs"),
        &ZoomBarKnobs::ALL,
        ZoomBarKnobs::label,
        |state| state.ui_settings.zoom_bar_knobs,
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

#[gpui::test]
fn every_settings_switch_flips_its_setting_and_reports_it(cx: &mut TestAppContext) {
    let (state, visual) = open_settings(cx);
    let switches = [
        Switch {
            row: "Audio scrubbing",
            id: "settings-audio-scrubbing",
            read: |state| state.ui_settings.audio_scrubbing,
        },
        Switch {
            row: "Log dropped frames",
            id: "settings-log-dropped-frames",
            read: |state| state.ui_settings.log_dropped_frames,
        },
    ];

    let mut faults = Vec::new();
    for switch in &switches {
        let label = Choice {
            row: None,
            shown: switch.row.to_owned(),
            chip: None,
            status: String::new(),
            holds: Box::new(|_| true),
        };
        // Twice, so the switch is pressed from each end.
        for _ in 0..2 {
            let was = visual.read(|app| (switch.read)(state.read(app)));
            let card = visual
                .debug_bounds("dialog-card")
                .expect("the Settings dialog draws a card");
            let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
            let row = locate(&runs, card, &label).bounds;
            let Some(bounds) = visual.debug_bounds(switch.id) else {
                faults.push(format!("{} draws no switch", switch.row));
                break;
            };
            if (bounds.center().y - row.center().y).abs() >= px(1.0) || bounds.left() <= row.right()
            {
                faults.push(format!(
                    "the {} switch at {bounds:?} is not on its label's line right of {row:?}",
                    switch.row
                ));
            }
            visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            visual.run_until_parked();
            if !(hover_t(ControlId::from(switch.id).motion("switch")) > 0.0) {
                faults.push(format!("{} is not drawn as a switch", switch.row));
            }
            visual.simulate_click(bounds.center(), Modifiers::default());
            visual.run_until_parked();
            let shown = if was { "Off" } else { "On" };
            let status = format!("{}: {shown}", switch.row);
            visual.read(|app| {
                let state = state.read(app);
                if (switch.read)(state) == was {
                    faults.push(format!("pressing {} left it {was}", switch.row));
                }
                if state.status_message.as_deref() != Some(status.as_str()) {
                    faults.push(format!(
                        "pressing {} reported {:?}, not {status:?}",
                        switch.row, state.status_message
                    ));
                }
                if let Some(error) = &state.last_error {
                    faults.push(format!("pressing {} failed: {error}", switch.row));
                }
                if state.dialog != DialogKind::Settings {
                    faults.push(format!("pressing {} closed Settings", switch.row));
                }
            });
        }
    }
    assert!(
        faults.is_empty(),
        "Settings switch faults:\n  {}",
        faults.join("\n  ")
    );
}
