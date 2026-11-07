//! Contracts for the status strip: a status expires, an error does not.
//!
//! The classes closed here:
//! - A receipt posted once stays on the strip for the rest of the session,
//!   which made the strip a log of the oldest thing that happened. Ageing is
//!   driven by a passed-in `now`, so these assertions need no sleeping and no
//!   wall clock.
//! - A refusal of the selection (`"Select a clip to analyze"`, `"Select a clip
//!   with audio to transcribe"`, `"“texture” has no audio track"`) outlives the
//!   selection change that replaces it, or a selection writer updates
//!   `AppState::selection` without finishing in
//!   [`AppState::refresh_projected_selection`]. Every selection writer in
//!   `app/` is derived from source at run time and exercised against selection
//!   refusals and unrelated errors.
//!
//! Not covered: the drawn strip and the 500ms ticker that calls `age_status`,
//! which need a render surface; an error string whose wording shares none of
//! the selection clauses in `app/status.rs`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, LabelColor, MediaId, ModelError, TrackId};

use super::AppState;
use super::analysis::AnalysisKind;
use super::premiere_tests::with_clip;
use super::status::STATUS_TTL;
use crate::test_support::sources::{code_lines, production_sources};
use crate::tool_catalog::TrackSelectDir;
use crate::types::TrimSide;

fn state() -> AppState {
    with_clip().0
}

#[test]
fn a_status_clears_once_its_time_is_spent() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    assert!(!state.age_status(now), "the first look starts the clock");
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(
        !state.age_status(now + STATUS_TTL - Duration::from_millis(1)),
        "not yet"
    );
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(state.age_status(now + STATUS_TTL), "spent");
    assert_eq!(state.status_message, None);
}

#[test]
fn a_second_status_gets_its_own_full_life() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    state.age_status(now);
    // A new message replaces the old one halfway through its life; it must not
    // inherit the remainder.
    let half = now + STATUS_TTL / 2;
    state.status_message = Some("Pasted".into());
    assert!(!state.age_status(half));
    assert!(
        !state.age_status(half + STATUS_TTL - Duration::from_millis(1)),
        "the clock restarted with the new text"
    );
    assert_eq!(state.status_message.as_deref(), Some("Pasted"));
    assert!(state.age_status(half + STATUS_TTL));
    assert_eq!(state.status_message, None);
}

#[test]
fn the_same_text_posted_again_after_expiry_is_shown_again() {
    let mut state = state();
    let now = Instant::now();
    state.status_message = Some("Copied".into());
    state.age_status(now);
    assert!(state.age_status(now + STATUS_TTL));
    let later = now + STATUS_TTL * 2;
    state.status_message = Some("Copied".into());
    assert!(!state.age_status(later), "a re-post is a new message");
    assert_eq!(state.status_message.as_deref(), Some("Copied"));
    assert!(state.age_status(later + STATUS_TTL));
    assert_eq!(state.status_message, None);
}

#[test]
fn an_empty_strip_reports_no_change_and_holds_no_clock() {
    let mut state = state();
    let now = Instant::now();
    // The fixture's own import posts a receipt; this test is about the empty
    // strip, so start from one.
    state.status_message = None;
    assert!(!state.age_status(now));
    assert!(state.status_seen.is_none());
    // A status cleared by its own writer must not leave a clock behind that
    // could expire the next message early.
    state.status_message = Some("Analyzing…".into());
    state.age_status(now);
    state.status_message = None;
    assert!(!state.age_status(now + STATUS_TTL));
    assert!(state.status_seen.is_none());
}

#[test]
fn an_error_is_never_aged_out() {
    let mut state = state();
    let now = Instant::now();
    state.last_error = Some("could not open the file".into());
    state.status_message = Some("Copied".into());
    state.age_status(now);
    assert!(state.age_status(now + STATUS_TTL));
    assert_eq!(state.status_message, None, "the receipt went");
    assert_eq!(
        state.last_error.as_deref(),
        Some("could not open the file"),
        "the failure stays until something works"
    );
}

#[test]
fn a_successful_edit_clears_the_error() {
    let (mut state, _clip) = with_clip();
    state.last_error = Some("that lane is locked".into());
    state.add_marker_at_playhead().expect("a marker lands");
    assert_eq!(
        state.last_error, None,
        "an error superseded by a successful action is stale"
    );
}

#[test]
fn undo_and_redo_clear_the_error_too() {
    let (mut state, _clip) = with_clip();
    state.add_marker_at_playhead().expect("a marker lands");
    state.last_error = Some("stale".into());
    assert!(state.undo());
    assert_eq!(state.last_error, None, "undo is a successful action");
    state.last_error = Some("stale again".into());
    assert!(state.redo());
    assert_eq!(state.last_error, None, "so is redo");
}

#[test]
fn a_refused_command_leaves_the_error_up() {
    let (mut state, _clip) = with_clip();
    let track = state.store.sequence.tracks[0].id;
    // An unknown clip id cannot be split, so nothing succeeded and the previous
    // failure is still the newest true thing on the strip.
    state
        .set_track_locked(aria_models::TrackId::from_raw(track), true)
        .expect("lock");
    state.last_error = Some("first failure".into());
    state.select_all_unlocked();
    assert!(
        state.split_at_playhead().is_ok(),
        "a locked lane has nothing selected to split"
    );
    assert_eq!(
        state.last_error.as_deref(),
        Some("first failure"),
        "an attempt that reached no command does not clear the strip"
    );
}

/// Patterns in blanked code that mutate `self.selection`.
const SELECTION_MUTATIONS: [&str; 12] = [
    "self.selection.clear(",
    "self.selection.push(",
    "self.selection.remove(",
    "self.selection.insert(",
    "self.selection.retain(",
    "self.selection.extend(",
    "self.selection.truncate(",
    "self.selection.pop(",
    "self.selection.drain(",
    "self.selection.dedup",
    "self.selection.sort",
    "&mut self.selection",
];

fn mutates_selection(line: &str) -> bool {
    if SELECTION_MUTATIONS
        .iter()
        .any(|pattern| line.contains(pattern))
    {
        return true;
    }
    for (at, _) in line.match_indices("self.selection") {
        let rest = line[at + "self.selection".len()..].trim_start();
        if rest.starts_with('=') && !rest.starts_with("==") {
            return true;
        }
    }
    false
}

fn fn_name_on(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    for prefix in [
        "pub fn ",
        "pub(crate) fn ",
        "pub(super) fn ",
        "fn ",
        "pub async fn ",
        "async fn ",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            if end > 0 {
                return Some(&rest[..end]);
            }
        }
    }
    None
}

/// Every function in `app/` whose body mutates `self.selection`: `(name, file:line, body)`.
fn selection_writers_in_source() -> Vec<(String, String, String)> {
    let mut writers = Vec::new();
    for (rel, code) in production_sources() {
        if rel != "app.rs" && !rel.starts_with("app/") {
            continue;
        }
        let lines = code_lines(&code);
        let mut index = 0usize;
        while index < lines.len() {
            let Some(name) = fn_name_on(&lines[index]).map(str::to_owned) else {
                index += 1;
                continue;
            };
            let start_line = index + 1;
            let mut depth = 0i32;
            let mut seen_open = false;
            let mut body = String::new();
            while index < lines.len() {
                let line = &lines[index];
                for ch in line.chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            seen_open = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                body.push_str(line);
                body.push('\n');
                index += 1;
                if seen_open && depth <= 0 {
                    break;
                }
            }
            if body.lines().any(mutates_selection)
                && !body.contains("self.apply_command(")
                && !body.contains("self.host.replace_session(")
            {
                writers.push((name, format!("{rel}:{start_line}"), body));
            }
        }
    }
    writers
}

/// Build a two-clip fixture where every selection writer has a reachable target:
/// `clip_a` has label `Rose` and starts at tick 0; `clip_b` starts after a gap
/// on the same unlocked video track and is grouped with `clip_a`.
fn with_two_grouped_clips() -> (AppState, TrackId, MediaId, ClipId, ClipId) {
    let (mut state, clip_a) = with_clip();
    let track = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let media = MediaId::from_raw(
        state.store.sequence.tracks[0].clips[0]
            .media_id
            .expect("fixture clip has media"),
    );
    state
        .drop_media_on_timeline(media, Some(track), 400)
        .expect("second clip places after a gap");
    let clip_b = state.store.sequence.tracks[0]
        .clips
        .iter()
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|&id| id != clip_a)
        .expect("second clip id");
    state
        .apply_command(Command::Edit(EditCommand::SetClipLabel {
            clip: clip_a,
            label: Some(LabelColor::Rose),
        }))
        .expect("label clip_a");
    state
        .apply_command(Command::Edit(EditCommand::GroupClips {
            clips: vec![clip_a, clip_b],
        }))
        .expect("group clips");
    (state, track, media, clip_a, clip_b)
}

#[test]
fn every_selection_writer_finishes_in_refresh_projected_selection() {
    let writers = selection_writers_in_source();
    assert!(
        writers.len() >= 15,
        "found only {} selection writers in app/; expected at least 15",
        writers.len()
    );
    let mut bypassed = Vec::new();
    for (name, at, body) in &writers {
        if !body.contains("self.refresh_projected_selection()")
            || body.contains("self.rebuild_store()")
        {
            bypassed.push(format!("{at}: fn {name}"));
        }
    }
    assert!(
        bypassed.is_empty(),
        "selection writers that bypass `refresh_projected_selection`:\n  {}",
        bypassed.join("\n  ")
    );
}

#[test]
fn every_selection_writer_clears_selection_refusals_and_keeps_unrelated_errors() {
    type Step = (
        &'static str,
        Box<dyn Fn(&mut AppState, TrackId, MediaId, ClipId, ClipId)>,
    );
    let steps: Vec<Step> = vec![
        (
            "select_clip",
            Box::new(|state, _, _, clip_a, _| {
                state.select_clip(clip_a);
                assert!(state.selection.contains(&clip_a));
            }),
        ),
        (
            "clear_selection",
            Box::new(|state, _, _, _, _| {
                state.clear_selection();
                assert!(state.selection.is_empty());
            }),
        ),
        (
            "select_edge",
            Box::new(|state, _, _, clip_a, _| {
                state.select_edge(clip_a, TrimSide::In);
                assert_eq!(state.selection, vec![clip_a]);
            }),
        ),
        (
            "select_gap_at",
            Box::new(|state, track, _, _, _| {
                let end_a = state.store.sequence.tracks[0].clips[0].timeline_end_ticks;
                let gap = state.select_gap_at(track, end_a + 1);
                assert!(
                    gap.is_some(),
                    "fixture holds a gap between clip_a and clip_b"
                );
                assert!(state.selection.is_empty());
            }),
        ),
        (
            "select_track_clips",
            Box::new(|state, track, _, _, _| {
                state.select_track_clips(track, 0, TrackSelectDir::Forward, false);
                assert!(!state.selection.is_empty());
            }),
        ),
        (
            "focus_or_select_clip",
            Box::new(|state, _, _, _, clip_b| {
                state.focus_or_select_clip(clip_b, false);
                assert_eq!(state.selection.first().copied(), Some(clip_b));
            }),
        ),
        (
            "toggle_clip_in_selection",
            Box::new(|state, _, _, clip_a, _| {
                state.toggle_clip_in_selection(clip_a);
                assert!(state.selection.contains(&clip_a));
            }),
        ),
        (
            "add_clip_to_selection",
            Box::new(|state, _, _, clip_a, _| {
                state.add_clip_to_selection(clip_a);
                assert!(state.selection.contains(&clip_a));
            }),
        ),
        (
            "select_at_playhead",
            Box::new(|state, _, _, clip_a, _| {
                state.set_playhead(0);
                state.select_at_playhead();
                assert!(state.selection.contains(&clip_a));
            }),
        ),
        (
            "select_by_label",
            Box::new(|state, _, _, clip_a, _| {
                state.select_by_label(LabelColor::Rose);
                assert_eq!(state.selection, vec![clip_a]);
            }),
        ),
        (
            "commit_marquee",
            Box::new(|state, track, _, clip_a, _| {
                state.begin_marquee(track.raw(), 0);
                state.drag_marquee(track.raw(), 48);
                assert!(state.commit_marquee() >= 1);
                assert!(state.selection.contains(&clip_a));
            }),
        ),
        (
            "select_media_clips",
            Box::new(|state, _, media, _, _| {
                assert!(state.select_media_clips(media) >= 1);
                assert!(!state.selection.is_empty());
            }),
        ),
        (
            "select_clip_linked",
            Box::new(|state, _, _, clip_a, _| {
                state.select_clip_linked(clip_a, false);
                assert_eq!(state.selection, vec![clip_a]);
            }),
        ),
        (
            "select_all_unlocked",
            Box::new(|state, _, _, _, _| {
                state.select_all_unlocked();
                assert!(!state.selection.is_empty());
            }),
        ),
        (
            "each_peer",
            Box::new(|state, _, _, clip_a, _| {
                state
                    .each_peer(&[clip_a], &mut |_| Ok::<(), EngineError>(()))
                    .expect("no-op peer step succeeds");
            }),
        ),
    ];

    let derived: BTreeSet<String> = selection_writers_in_source()
        .into_iter()
        .map(|(name, _, _)| name)
        .collect();
    let covered: BTreeSet<String> = steps.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(
        covered, derived,
        "every selection writer in `app/` must be exercised in this table"
    );

    let (mut state, track, media, clip_a, clip_b) = with_two_grouped_clips();
    state.clear_selection();
    let analysis_unselected = state
        .analysis_precheck(AnalysisKind::Shots)
        .expect_err("nothing selected refuses analysis");
    let transcribe_unselected = state
        .transcription_precheck()
        .expect_err("nothing selected refuses transcription");
    let duck_unselected = state
        .duck_precheck()
        .expect_err("nothing selected refuses ducking")
        .reason()
        .to_string();
    let bake_unselected = state
        .render_and_replace()
        .expect_err("nothing selected refuses render and replace");
    let restore_unselected = state
        .restore_unrendered()
        .expect_err("nothing selected refuses restore");
    state.select_clip_linked(clip_a, false);
    let analysis_silent = state
        .analysis_precheck(AnalysisKind::Silence)
        .expect_err("silent still refuses audio analysis");

    let selection_refusals = [
        analysis_unselected,
        transcribe_unselected,
        duck_unselected,
        bake_unselected,
        restore_unselected,
        analysis_silent,
    ];

    let prepare = |state: &mut AppState, writer: &str| {
        state.selection = vec![clip_a, clip_b];
        state.refresh_projected_selection();
        if writer != "clear_selection"
            && writer != "select_gap_at"
            && writer != "focus_or_select_clip"
        {
            state.clear_selection();
        }
    };

    for (writer, run) in &steps {
        for refusal in &selection_refusals {
            prepare(&mut state, writer);
            state.report_error(refusal);
            let posted = super::status::as_sentence(refusal.clone());
            assert_eq!(
                state.last_error.as_deref(),
                Some(posted.as_str()),
                "{writer}: refusal posted before selection change"
            );
            run(&mut state, track, media, clip_a, clip_b);
            assert_eq!(
                state.last_error, None,
                "{writer} left selection refusal {refusal:?} on the strip"
            );
        }
    }

    // Also cover the second branch of `commit_marquee` (zero-width click deselect)
    // and the grouped branch of `select_clip_linked(clip, true)`.
    state.select_clip_linked(clip_a, false);
    state.report_error(&selection_refusals[0]);
    state.begin_marquee(track.raw(), 0);
    assert_eq!(state.commit_marquee(), 0);
    assert_eq!(
        state.last_error, None,
        "commit_marquee click-deselect branch left selection refusal on the strip"
    );

    state.clear_selection();
    state.report_error(&selection_refusals[0]);
    state.select_clip_linked(clip_a, true);
    assert_eq!(
        state.last_error, None,
        "select_clip_linked grouped branch left selection refusal on the strip"
    );

    // Negative cases: errors that are not about the selection must survive every
    // selection writer.
    for (label, report) in [
        ("export failure", EngineError::Export("disk full".into())),
        (
            "import failure",
            EngineError::Import("corrupt header".into()),
        ),
        (
            "model non-selection failure",
            EngineError::Model(ModelError::TimeOverflow),
        ),
    ] {
        for (writer, run) in &steps {
            prepare(&mut state, writer);
            state.report_error(&report);
            let expected = state.last_error.clone().expect("error posted");
            run(&mut state, track, media, clip_a, clip_b);
            assert_eq!(
                state.last_error.as_deref(),
                Some(expected.as_str()),
                "{writer} cleared non-selection {label} {expected:?}"
            );
        }
    }

    for literal in [
        "Echo did not reload: symbol missing",
        "Install a transcription model in Settings first",
    ] {
        for (writer, run) in &steps {
            prepare(&mut state, writer);
            state.report_error(literal);
            run(&mut state, track, media, clip_a, clip_b);
            assert_eq!(
                state.last_error.as_deref(),
                Some(literal),
                "{writer} cleared non-selection error {literal:?}"
            );
        }
    }
}
