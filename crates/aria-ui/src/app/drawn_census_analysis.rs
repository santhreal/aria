//! Driving the Analysis panel: the clears, the refresh, the two filters, the two
//! seeks, and the two edits that act on what a detector found.
//!
//! # Why
//!
//! `Clear beats` claimed the panel's clear button and nothing pressed it, so a
//! clear wired to another detector would have deleted the wrong batch with
//! nothing red. The controls are a registry now
//! ([`analysis_pick`](crate::surfaces::analysis_pick)) and this presses every one
//! of them.
//!
//! Every row is pressed against a project that has already been analysed, which
//! is the only state the panel draws most of them in. The records are written
//! into the session's own index by
//! [`store_moments`](crate::app::fixtures::store_moments), which is what a
//! detector run does when it finishes, so no detector runs here: a census must
//! not decode a file or start a job.
//!
//! `Set beats` is the one command in this pair that no button sends. Its surface
//! is the beat run itself, writing what it found, so the census drives that
//! write ([`AppState::store_beats`]) rather than inventing a button for it.

use std::collections::BTreeSet;

use aria_models::ClipId;

use crate::app::AppState;
use crate::app::analysis::AnalysisKind;
use crate::app::fixtures::store_moments;
use crate::app::fixtures::with_video;
use crate::app::sent_census::Watch;
use crate::surfaces::analysis_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "analysis_pick.rs";

/// A project whose selected clip has been analysed by every detector, and that
/// clip.
///
/// The media is the audio-and-video fixture rather than the still every other
/// census uses, because two of the three detectors read audio and the clear
/// beside each of them keys its cache row through the same precheck the run
/// does: on a still, clearing the silence is refused for want of an audio
/// track, which is a real refusal and not the one this census is measuring.
///
/// The spans are inside the clip on purpose: a boundary outside it is not an
/// edit, and a silence that does not overlap it is not a cut, so a fixture with
/// spans outside would draw the buttons and reach nothing. They are also at
/// three different seconds, so a seek between rows has somewhere to go: two
/// detectors that found the same instant are one position on the ruler.
fn analysed() -> (AppState, ClipId) {
    let (mut state, clip) = with_video();
    store_moments(&state, AnalysisKind::Beats, &[(0.5, 0.5, 1.0)]);
    store_moments(&state, AnalysisKind::Shots, &[(1.0, 1.0, 0.9)]);
    store_moments(&state, AnalysisKind::Silence, &[(1.5, 2.0, 1.0)]);
    let rate = state.store.media[0].frame_rate;
    state.store_beats(clip, &[0.5], rate);
    state
        .refresh_moments()
        .expect("the index the fixture wrote is readable");
    assert!(
        !state.analysis.moments.is_empty(),
        "the fixture's moments reached the panel's rows"
    );
    (state, clip)
}

/// Every control the panel draws, in the order it draws them.
///
/// The clears come off the detector catalog and the confidence rows off the
/// panel's own choices, so a detector or a threshold added later is pressed
/// without an edit here.
fn picks() -> Vec<Pick> {
    let mut out: Vec<Pick> = AnalysisKind::ALL.into_iter().map(Pick::Clear).collect();
    out.push(Pick::Refresh);
    out.push(Pick::Filter(None));
    out.extend(
        AnalysisKind::ALL
            .into_iter()
            .map(|kind| Pick::Filter(Some(kind))),
    );
    out.extend(
        crate::app::analysis::CONFIDENCE_CHOICES
            .iter()
            .map(|choice| Pick::Confidence(*choice)),
    );
    out.push(Pick::SeekPrevious);
    out.push(Pick::SeekNext);
    out.push(Pick::SplitAtShots);
    out.push(Pick::CutSilence);
    out
}

/// Press one control the way the panel does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    analysis_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// Every command the panel's controls asked for, plus what the beat run writes.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for pick in picks() {
        let (mut state, _) = analysed();
        let watch = Watch::on(&state);
        press(&mut state, pick);
        out.extend(watch.take());
    }
    let (mut state, clip) = with_video();
    let rate = state.store.media[0].frame_rate;
    let watch = Watch::on(&state);
    state.store_beats(clip, &[0.5, 1.0], rate);
    out.extend(watch.take());
    out
}

#[test]
fn every_control_the_panel_draws_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not anyone pressed it.
    let pressed: BTreeSet<String> = picks()
        .into_iter()
        .map(|pick| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these analysis controls are drawn and nothing presses them: {missing:?}. \
         The answer is a catalog this census reads, never an exception"
    );
}

#[test]
fn the_clear_beside_a_detector_drops_that_detector_and_no_other() {
    // The defect the missing surface hid: one clear per detector, and each one
    // keys its own batch. A clear that deleted the wrong batch, or all of them,
    // would leave the panel showing results for a detector that was kept.
    for kind in AnalysisKind::ALL {
        let (mut state, _) = analysed();
        press(&mut state, Pick::Clear(kind));
        assert!(
            !state.analysis_exists(kind),
            "clearing {} left its own results behind",
            kind.label()
        );
        for other in AnalysisKind::ALL {
            if other == kind {
                continue;
            }
            assert!(
                state.analysis_exists(other),
                "clearing {} also dropped {}",
                kind.label(),
                other.label()
            );
        }
    }
}

#[test]
fn clearing_the_beats_clears_the_grid_the_run_wrote() {
    // The grid is the project's copy, and it is what the timeline snaps to. A
    // clear that dropped the batch and kept the grid would leave the magnets
    // pulling toward positions the panel no longer shows.
    let (mut state, clip) = analysed();
    assert!(
        !beat_ticks(&state, clip).is_empty(),
        "the fixture's clip carries a grid"
    );
    let watch = Watch::on(&state);
    press(&mut state, Pick::Clear(AnalysisKind::Beats));
    assert!(
        watch.take().contains(&"Clear beats"),
        "clearing the beats sent no command, so the grid is still on the clip"
    );
    assert!(
        beat_ticks(&state, clip).is_empty(),
        "the grid outlived the analysis that wrote it"
    );
}

#[test]
fn the_beat_run_writes_what_it_found() {
    // `Set beats` names the run, not a button: this is the adoption path the
    // job's completion takes. The detector reports seconds and the clip keeps
    // source ticks, so what is asserted is the projected position: where the
    // ruler draws the hairline.
    let (mut state, clip) = with_video();
    let rate = state.store.media[0].frame_rate;
    let watch = Watch::on(&state);
    state.store_beats(clip, &[0.5], rate);
    assert!(
        watch.take().contains(&"Set beats"),
        "the beat run adopted nothing"
    );
    assert_eq!(
        beat_ticks(&state, clip),
        vec![crate::app::analysis::ticks_for_seconds(
            0.5,
            state.playhead.rate
        )],
        "the grid is not where the sequence puts the second the run reported"
    );
}

#[test]
fn the_two_edits_reach_the_timeline() {
    // Both are drawn only once the count is above zero, so both are pressed on
    // a project that has the moments they act on.
    for (pick, label) in [
        (Pick::SplitAtShots, "Split clip"),
        (Pick::CutSilence, "Ripple delete"),
    ] {
        let (mut state, _) = analysed();
        let watch = Watch::on(&state);
        press(&mut state, pick);
        let sent = watch.take();
        assert!(
            sent.contains(&label),
            "{} sent {sent:?} rather than `{label}`",
            pick.what()
        );
    }
}

#[test]
fn a_seek_moves_the_playhead_and_asks_for_nothing() {
    // A seek is not an edit. It must not reach the engine, and it must land on
    // the moment the rows show rather than nowhere.
    //
    // Two forward seeks before the backward one: the fixture's rows are two
    // positions, and a previous-seek from the first of them has nothing behind
    // it, which is the correct answer rather than a defect.
    let (mut state, _) = analysed();
    state.set_playhead(0);
    let watch = Watch::on(&state);
    press(&mut state, Pick::SeekNext);
    assert!(watch.take().is_empty(), "seeking to a moment wrote history");
    let first = state.playhead.value;
    assert!(
        first > 0,
        "the playhead stayed where it was, so nothing was found to seek to"
    );
    press(&mut state, Pick::SeekNext);
    let second = state.playhead.value;
    assert!(
        second > first,
        "the second seek did not reach the row after the first"
    );
    press(&mut state, Pick::SeekPrevious);
    assert_eq!(
        state.playhead.value, first,
        "seeking back did not land on the row it came from"
    );
}

#[test]
fn a_filter_narrows_the_rows_without_touching_the_project() {
    // Both filters are view state read back out of the index, so the
    // fingerprint is the row count and the assertion is that no command left.
    let (mut state, _) = analysed();
    let all = state.analysis.moments.len();
    let watch = Watch::on(&state);
    press(&mut state, Pick::Filter(Some(AnalysisKind::Shots)));
    assert!(
        watch.take().is_empty(),
        "filtering the rows reached the engine"
    );
    let shots = state.analysis.moments.len();
    assert!(
        shots < all,
        "the filter kept {shots} of {all} rows, so it narrowed nothing"
    );
    press(&mut state, Pick::Filter(None));
    assert_eq!(
        state.analysis.moments.len(),
        all,
        "clearing the filter did not put the rows back"
    );
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    for label in ["Clear beats", "Set beats"] {
        assert!(
            sent.contains(label),
            "the analysis census pressed every control the panel draws and none \
             of them sent `{label}`"
        );
    }
}

#[test]
fn the_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "analysis.rs");
}

/// The beat grid on one clip, as the timeline reads it.
fn beat_ticks(state: &AppState, clip: ClipId) -> Vec<i64> {
    state
        .ui_clip(clip)
        .map(|clip| clip.beat_ticks.to_vec())
        .unwrap_or_default()
}
