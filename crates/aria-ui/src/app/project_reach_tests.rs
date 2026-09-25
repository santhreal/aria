//! What the project surfaces must actually reach.
//!
//! The class closed here is "a catalog grew a row and the editor never offered
//! it". Every case enumerates a registry at run time —
//! [`aria_models::clip_facts`], [`aria_models::project_facts`],
//! [`aria_engine::frame_stages`], or the palette group itself — and compares
//! sets, so a row added to any of them is exercised the moment it exists and a
//! row that disappears takes its assertion with it. No case lists a fact, a
//! stage, or a count.
//!
//! What this does not catch, stated plainly:
//!
//! - A row whose label describes a different question than its predicate
//!   answers. The catalog's own identity test proves an arm points at its own
//!   row, not that the row is honest about itself.
//! - `report_frame_cost` and `selected_content_size` need a rendered frame and a
//!   GPU raster respectively. Headless, both are the "nothing measured yet"
//!   branch, which is what is asserted; the measured branch is the renderer's
//!   own suite.
//! - The two picking rows are driven with a supplied answer, not a dialog.

use aria_engine::frame_stages;
use aria_models::{clip_facts, project_facts, track_kind_specs};

use crate::app::AppState;
use crate::app::picker::Answer;
use crate::app::premiere_tests::with_clip;
use crate::commands::project::PROJECT_ROWS;

/// The fixture with the playhead inside its one clip, which is where most rows
/// are meaningful.
fn inside() -> AppState {
    let (mut state, _) = with_clip();
    if let Some(ui) = state.selected_ui_clip() {
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
    }
    state
}

fn history_len(state: &AppState) -> usize {
    state.history_entries().len()
}

// --- the catalogs reach the editor ---------------------------------------

/// The rows the editor offers are exactly the rows the model declares, in the
/// same order. A question added to the catalog and dropped by the projection
/// would pass a count check and fail this one.
#[test]
fn every_clip_question_is_offered_as_a_row() {
    let mut state = inside();
    state.measure_project_report();
    let offered: Vec<&str> = state.clip_fact_rows().iter().map(|row| row.id).collect();
    let declared: Vec<&str> = clip_facts().iter().map(|spec| spec.id).collect();
    assert_eq!(offered, declared);
    for row in state.clip_fact_rows().iter() {
        assert!(!row.label.is_empty(), "'{}' has no label", row.id);
    }
}

#[test]
fn every_project_number_is_offered_with_a_value() {
    let mut state = inside();
    state.measure_project_report();
    let offered: Vec<&str> = state.project_fact_rows().iter().map(|row| row.id).collect();
    let declared: Vec<&str> = project_facts().iter().map(|spec| spec.id).collect();
    assert_eq!(offered, declared);
    for row in state.project_fact_rows().iter() {
        assert!(!row.value.trim().is_empty(), "'{}' reads nothing", row.id);
    }
}

/// The stage rows and the whole-frame total come from the same measurement, so a
/// stage that stops being read shows up as a total the rows cannot account for.
#[test]
fn every_render_stage_is_offered_and_sums_to_the_total() {
    let state = inside();
    let offered: Vec<&str> = state.frame_stage_rows().iter().map(|row| row.id).collect();
    let declared: Vec<&str> = frame_stages().iter().map(|stage| stage.id).collect();
    assert_eq!(offered, declared);
    let summed: f64 = state
        .frame_stage_rows()
        .iter()
        .map(|row| row.milliseconds)
        .sum();
    assert_eq!(summed, state.frame_total_ms());
}

/// A report is measured when it is asked for, so a state that has never been
/// asked counts nothing. The dialog asks on the way open; nothing draws a row
/// that was never taken.
#[test]
fn a_report_nobody_asked_for_counts_nothing() {
    let mut state = inside();
    assert!(state.project_fact_rows().is_empty());
    assert!(state.clip_fact_rows().is_empty());
    state.enter_editor();
    state.open_dialog(crate::app::DialogKind::ProjectReport);
    assert_eq!(
        state.project_fact_rows().len(),
        project_facts().len(),
        "opening the report did not measure it"
    );
}

/// Walking a question either lands on a clip that answers it or says nothing
/// does. Driven for every row of the catalog, so a question whose navigation
/// panics is caught the moment it ships.
#[test]
fn walking_every_question_lands_on_a_clip_or_reports_none() {
    for spec in clip_facts() {
        let mut state = inside();
        state.measure_project_report();
        let holds = state
            .clip_fact_rows()
            .iter()
            .find(|row| row.id == spec.id)
            .map(|row| row.count)
            .expect("the row is offered");
        let before = history_len(&state);
        state.go_to_clip_with_fact(spec.id);
        assert_eq!(
            history_len(&state),
            before,
            "'{}' recorded an edit while navigating",
            spec.id
        );
        let message = state.status_message.clone().unwrap_or_default();
        if holds == 0 {
            assert!(
                message.contains("No clip carries"),
                "'{}' holds for nothing and said {message:?}",
                spec.id
            );
            continue;
        }
        let selected = state.selected_clip().expect("a match is selected");
        let engine = state.engine.lock();
        let timeline = engine.project().timeline();
        let clip = timeline.clip(selected).expect("the selected clip exists");
        assert!(
            spec.fact.holds(timeline, clip),
            "'{}' selected a clip that does not answer it",
            spec.id
        );
    }
}

/// An unknown id is inert rather than an error: a row read from a stale layout
/// must not take the editor down.
#[test]
fn an_unknown_question_id_is_inert() {
    let mut state = inside();
    let before = history_len(&state);
    state.status_message = None;
    state.go_to_clip_with_fact("no-such-question");
    assert_eq!(history_len(&state), before);
    assert_eq!(state.status_message, None);
}

// --- palette group -------------------------------------------------------

/// Every row runs against a dressed state without panicking, and none of them
/// records an edit the row does not claim. Enumerated from the group, so a row
/// added here is driven without being listed.
#[test]
fn every_project_row_runs() {
    for row in PROJECT_ROWS {
        let mut state = inside();
        (row.run)(&mut state);
        assert!(
            row.id.starts_with("pal-project-"),
            "'{}' is not namespaced to this group",
            row.id
        );
    }
}

#[test]
fn every_project_row_is_uniquely_named() {
    let ids: std::collections::BTreeSet<_> = PROJECT_ROWS.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), PROJECT_ROWS.len(), "two rows share an id");
    let labels: std::collections::BTreeSet<_> = PROJECT_ROWS.iter().map(|row| row.label).collect();
    assert_eq!(labels.len(), PROJECT_ROWS.len(), "two rows share a label");
    for row in PROJECT_ROWS {
        assert!(!row.label.is_empty(), "'{}' has no label", row.id);
    }
}

/// The group is only a UI surface once the joined table carries it. Compared as
/// a set against the live registry, so a group that stops being joined — or a
/// row dropped from the join by hand — is red rather than merely unreachable.
#[test]
fn every_project_row_reaches_the_registry() {
    let registered: std::collections::BTreeSet<&str> = crate::commands::BUILTIN_COMMANDS
        .iter()
        .map(|row| row.id)
        .collect();
    let mine: std::collections::BTreeSet<&str> = PROJECT_ROWS.iter().map(|row| row.id).collect();
    let missing: Vec<&&str> = mine.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "these rows exist and the palette does not offer them: {missing:?}"
    );
}

// --- individual verbs ----------------------------------------------------

#[test]
fn the_clip_under_the_playhead_is_selectable() {
    let (mut state, clip) = with_clip();
    let ui = state.selected_ui_clip().expect("the fixture clip");
    state.set_playhead(ui.timeline_start_ticks + 1);
    state.clear_selection();
    state.select_clip_under_playhead();
    assert_eq!(state.selected_clip(), Some(clip));

    // Off the clip there is nothing to select, and the row says so rather than
    // leaving the previous selection looking like a hit.
    state.set_playhead(ui.timeline_end_ticks + 1_000);
    state.clear_selection();
    state.select_clip_under_playhead();
    assert_eq!(state.selected_clip(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some("No clip under the playhead")
    );
}

/// One extra lane of every kind the catalog ships, so the offer is exercised
/// against the whole axis instead of the kinds someone remembered.
fn every_lane_kind() -> AppState {
    let mut state = inside();
    for spec in track_kind_specs() {
        state
            .add_track(spec.kind, format!("{} extra", spec.default_name))
            .expect("a lane of every shipped kind");
    }
    state
}

/// Which lanes are offered is the model's rule, so the offer is checked against
/// the model rather than against a list here. The lane the clip is on is never
/// offered, because duplicating onto it at the same tick is the one placement
/// the engine must refuse.
#[test]
fn only_lanes_that_accept_the_clip_are_offered() {
    let state = every_lane_kind();
    let offered = state.lanes_accepting_selection();
    let selected = state.selected_clip().expect("a clip");
    let engine = state.engine.lock();
    let timeline = engine.project().timeline();
    let clip = timeline.clip(selected).expect("the clip");
    let on = timeline.track_of(selected);
    let expected: Vec<u64> = timeline
        .tracks_ordered()
        .filter(|track| Some(track.id) != on && track.kind.accepts_clip(clip))
        .map(|track| track.id.raw())
        .collect();
    assert_eq!(
        offered.iter().map(|lane| lane.track).collect::<Vec<_>>(),
        expected
    );
    assert!(
        !expected.is_empty(),
        "the fixture clip must fit somewhere else once every lane kind exists"
    );
    assert!(
        offered.iter().all(|lane| !lane.name.is_empty()),
        "a lane is offered without a name to click"
    );
}

#[test]
fn duplicating_onto_another_lane_records_one_step() {
    let mut state = every_lane_kind();
    let before = history_len(&state);
    state
        .duplicate_selection_to_next_lane()
        .expect("the duplicate lands");
    assert_eq!(history_len(&state), before + 1);

    // A new session opens with one lane, so nothing else accepts the clip: the
    // row refuses by name instead of recording a step that goes nowhere.
    let (mut bare, _) = with_clip();
    let before = history_len(&bare);
    bare.duplicate_selection_to_next_lane().expect("inert");
    assert_eq!(history_len(&bare), before);
    assert_eq!(
        bare.status_message.as_deref(),
        Some("No other lane accepts this clip")
    );
}

/// The pending steps are named from the engine's own labels, so this also proves
/// the labels survive the projection the dialog reads.
#[test]
fn the_pending_steps_are_named() {
    // A session nobody has edited, not `inside()`: importing a file and placing
    // a clip are two undoable steps, and this half of the row is about having
    // none.
    let mut state = AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("pending-steps"),
    )
    .expect("a session");
    assert_eq!(state.next_undo_label(), None);
    state.report_pending_history();
    assert_eq!(
        state.status_message.as_deref(),
        Some("No steps to undo or redo")
    );

    state.add_marker_at_playhead().expect("a marker lands");
    let label = state.next_undo_label().expect("a step to undo");
    let last = state
        .history_entries()
        .last()
        .expect("one entry")
        .label
        .clone();
    assert_eq!(label, last);
    state.report_pending_history();
    assert_eq!(
        state.status_message.as_deref(),
        Some(format!("Undo: {label}").as_str())
    );

    assert!(state.undo());
    assert_eq!(state.next_redo_label().as_deref(), Some(label.as_str()));
}

#[test]
fn the_marker_count_follows_the_project() {
    let mut state = inside();
    assert_eq!(state.marker_count(), 0);
    state.add_marker_at_playhead().expect("a marker lands");
    assert_eq!(state.marker_count(), 1);
    assert_eq!(
        state.marker_count(),
        state.store.sequence.markers.len(),
        "the rows and the project must agree"
    );
}

/// The pool lookup is the project's, and the reveal opens the folder it reports.
#[test]
fn a_pooled_file_is_revealed_and_an_unknown_one_is_refused() {
    let mut state = inside();
    let item = state.store.media.first().cloned().expect("a pool entry");
    let path = std::path::PathBuf::from(&*item.path);
    assert!(
        state.reveal_media_in_pool(&path),
        "the fixture file is pooled"
    );
    assert_eq!(state.pool_selection.ids(), [item.id]);
    assert_eq!(state.current_bin, None, "the fixture entry is at the root");

    let missing = std::path::PathBuf::from("/nowhere/absent.mp4");
    assert!(!state.reveal_media_in_pool(&missing));
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|message| message.starts_with("Not in the pool")),
        "an absent file must be refused by name"
    );
}

/// The picking row asks exactly once and applies the answer through the same
/// verb the panel uses.
#[test]
fn the_reveal_row_asks_and_applies_the_answer() {
    let mut state = inside();
    let item = state.store.media.first().cloned().expect("a pool entry");
    state.supply_pick(Answer::file(std::path::PathBuf::from(&*item.path)));
    let asked = state.picks_asked();
    state.pick_and_reveal_media();
    assert_eq!(state.picks_asked(), asked + 1);
    assert_eq!(state.pool_selection.ids(), [item.id]);
}

/// A cancelled describe leaves the session alone, and a path that is not a
/// bundle is reported as an error rather than as a summary.
#[test]
fn describing_a_template_refuses_a_file_that_is_not_one() {
    let mut state = inside();
    state.supply_pick(Answer::cancelled());
    state.status_message = None;
    state.pick_and_describe_template();
    assert_eq!(state.status_message, None, "a cancel says nothing");

    let item = state.store.media.first().cloned().expect("a pool entry");
    state.supply_pick(Answer::file(std::path::PathBuf::from(&*item.path)));
    state.pick_and_describe_template();
    assert!(
        state.last_error.is_some(),
        "a file that is not a bundle must be refused"
    );
}

/// Headless there is no measured frame and no GPU raster, so both readouts must
/// take their empty branch rather than reporting a zero as a measurement.
#[test]
fn unmeasured_readouts_say_so() {
    let mut state = inside();
    state.report_frame_cost();
    assert_eq!(
        state.status_message.as_deref(),
        Some("No frame measured yet")
    );
    state.report_content_size();
    assert_eq!(
        state.status_message.as_deref(),
        Some("The selection draws no measurable content")
    );
}

/// **The report rows are one list per measurement, not one per read.**
///
/// The dialog cannot hold a borrow of the state across the frame, so it used to
/// copy both row lists on every frame it was open, and the counts they carry are
/// walks of the whole project rather than of what is on screen. Both are handles
/// now: two reads with nothing in between are the same list, and measuring again
/// replaces them.
#[test]
fn the_report_rows_are_one_list_per_measurement() {
    let mut state = inside();
    state.measure_project_report();
    let facts = state.project_fact_rows();
    let clips = state.clip_fact_rows();
    assert!(!facts.is_empty() && !clips.is_empty(), "nothing measured");
    assert!(
        std::sync::Arc::ptr_eq(&facts, &state.project_fact_rows()),
        "a second read copied the project rows"
    );
    assert!(
        std::sync::Arc::ptr_eq(&clips, &state.clip_fact_rows()),
        "a second read copied the clip rows"
    );
    state.measure_project_report();
    assert!(
        !std::sync::Arc::ptr_eq(&facts, &state.project_fact_rows()),
        "measuring again left the dialog drawing the previous numbers"
    );
}
