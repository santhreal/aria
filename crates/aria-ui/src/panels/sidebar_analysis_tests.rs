//! A run in the Analysis tab draws blocked exactly while the selection
//! refuses it.
//!
//! WHY: with no clip selected, Shots, Silence, Beats and Measure stood as live
//! ghosts under the hand, and a press put "Select a clip to analyze" on the
//! status line; over a selected still, Silence, Beats and Measure stood solid,
//! the action the panel offered, and a press reported that the still has no
//! audio track. The class closed: a run drawn live while its own precheck
//! refuses the selection, and one drawn blocked while the precheck takes it.
//! Each detector and Measure is read with no clip selected, with a still
//! selected, and with the still's media marked as holding sound, so every run
//! is read both blocked and live, and the cursor over it is held to what
//! [`AppState::analysis_target`](crate::app::AppState::analysis_target) and
//! [`AppState::loudness_target`](crate::app::AppState::loudness_target)
//! answer for it.
//!
//! Not caught: the 40 % opacity, which the test renderer does not read and
//! `components_control_tests` holds
//! [`blocked`](crate::components::blocked::blocked) to; an offline or
//! generated clip, whose refusals come from the same target.

use gpui::{CursorStyle, Modifiers, TestAppContext};

use super::body_tests::open;
use super::layout_tests::bounds;
use crate::app::analysis::AnalysisKind;
use crate::test_support::drawn_shell;
use crate::types::SidebarTab;

#[gpui::test]
fn a_run_draws_blocked_exactly_while_the_selection_refuses_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "analysis-refused");
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    let clip = visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            let tracks = &state.store.sequence.tracks;
            let first = tracks.iter().find_map(|track| track.clips.first());
            aria_models::ClipId::from_raw(first.expect("a clip").id)
        })
    });
    for (selected, sounds) in [(false, false), (true, false), (true, true)] {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                if selected {
                    state.select_clip(clip);
                } else {
                    state.clear_selection();
                }
                let media = state.store_mut().media.first_mut().expect("the still");
                media.has_audio = sounds;
                cx.notify();
            })
        });
        open(&state, visual, SidebarTab::Analysis);
        let runs: Vec<(&str, &str, bool)> = visual.read(|app| {
            let state = state.read(app);
            let refuses = |kind| state.analysis_target(kind).is_err();
            AnalysisKind::ALL
                .iter()
                .map(|&kind| (kind.label(), "Detect", refuses(kind)))
                .chain([("Measure", "Loudness", state.loudness_target().is_err())])
                .collect()
        });
        // The still holds pictures and no sound, so with it selected one run
        // is taken and the rest refused; marked as sounding, every run is
        // taken. All three draws are read.
        let refused: Vec<bool> = runs.iter().map(|(_, _, refused)| *refused).collect();
        let want = match (selected, sounds) {
            (false, _) => vec![true; 4],
            (true, false) => vec![false, true, true, true],
            (true, true) => vec![false; 4],
        };
        assert_eq!(
            refused, want,
            "selected {selected}, sounds {sounds}: the refused runs"
        );
        for (label, row, refused) in runs {
            let row_at = bounds(visual, &format!("form-{row}-controls"));
            let drawn = visual.update(|window, _| window.rendered_text_runs().to_vec());
            let at = drawn
                .iter()
                .find(|run| run.text.as_ref() == label && row_at.contains(&run.bounds.center()))
                .unwrap_or_else(|| panic!("the `{row}` row draws no `{label}` run"))
                .bounds
                .center();
            visual.simulate_mouse_move(at, None, Modifiers::none());
            visual.run_until_parked();
            let cursor = visual.update(|window, _| window.rendered_cursor_style());
            let (want, drawn_as) = if refused {
                (CursorStyle::Arrow, "blocked")
            } else {
                (CursorStyle::PointingHand, "live")
            };
            assert_eq!(
                cursor,
                Some(want),
                "`{label}` draws {drawn_as} under the hand, selected {selected}, sounds {sounds}"
            );
        }
    }
}
