//! Driving the markers panel: the add button, a marker's colour swatch, and its
//! delete button.
//!
//! # Why
//!
//! `Remove marker` claimed the panel's delete button and nothing pressed it, so
//! a button holding a stale id, or wired to the wrong marker, turned nothing
//! red. The panel's edits are a registry now
//! ([`markers_pick`](crate::surfaces::markers_pick)) and this presses every one
//! of them, reading the arms back out of the registry's own source so a control
//! added to the enum has no press and no green suite until someone writes one.
//!
//! The row controls are pressed against a marker the panel would have drawn: a
//! project with one on the ruler. The add button is pressed against the same
//! project, because a second marker at the playhead is what an operator gets by
//! clicking twice.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::surfaces::markers_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "markers_pick.rs";

/// A project with one marker on the ruler, and that marker's id.
///
/// The id comes off the sequence rather than out of a counter here: marker ids
/// are handed out by the session, and a census that guessed one would press a
/// row the panel never drew.
fn with_marker() -> (AppState, u64) {
    let (mut state, _) = with_clip();
    press(&mut state, Pick::AddAtPlayhead);
    let id = state
        .store
        .sequence
        .markers
        .first()
        .expect("adding a marker at the playhead put one on the ruler")
        .id;
    (state, id)
}

/// Every control the panel draws, in the order it draws them.
///
/// The row controls come off the state's own markers, so a project with none
/// offers only the add button, which is what the panel shows.
fn picks(state: &AppState) -> Vec<Pick> {
    let mut out = vec![Pick::AddAtPlayhead];
    for marker in state.store.sequence.markers.iter() {
        out.push(Pick::CycleColour(marker.id));
        out.push(Pick::Remove(marker.id));
    }
    out
}

/// Press one control the way the panel does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    markers_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// The `index`th control, and a project dressed for pressing it.
///
/// A marker id belongs to the session that minted it, so the pick is read off
/// the state it is pressed against: two projects with a marker each hold two
/// different ids, and a pick carried across from an earlier build names a marker
/// the panel never drew.
fn staged(index: usize) -> (AppState, Pick) {
    let (state, _) = with_marker();
    let pick = picks(&state)[index];
    (state, pick)
}

/// How many controls the panel draws on a dressed project.
fn drawn() -> usize {
    picks(&with_marker().0).len()
}

/// Every command the panel's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for index in 0..drawn() {
        let (mut state, pick) = staged(index);
        let watch = Watch::on(&state);
        press(&mut state, pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_control_the_panel_draws_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not anyone pressed it.
    let pressed: BTreeSet<String> = picks(&with_marker().0)
        .into_iter()
        .map(|pick| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these markers controls are drawn and nothing presses them: {missing:?}. \
         The answer is a dressed project this census builds, never an exception"
    );
}

#[test]
fn every_control_sends_something() {
    // A control that dispatches nothing is a button that looks like it edited
    // the ruler and left it as it was.
    for index in 0..drawn() {
        let (mut state, pick) = staged(index);
        let watch = Watch::on(&state);
        let what = pick.what();
        press(&mut state, pick);
        assert!(
            !watch.take().is_empty(),
            "pressing {what} sent nothing, so the control is inert"
        );
    }
}

#[test]
fn a_control_on_a_marker_that_is_gone_asks_for_nothing() {
    // A stale index is a click, not a bug report: the panel draws rows from a
    // projection, so a row can outlive its marker by one frame. Pressing it must
    // be silent rather than writing an undo entry for a marker that is not
    // there.
    for pick in [Pick::CycleColour, Pick::Remove] {
        let (mut state, id) = with_marker();
        press(&mut state, Pick::Remove(id));
        let watch = Watch::on(&state);
        press(&mut state, pick(id));
        assert!(
            watch.take().is_empty(),
            "{} on a deleted marker reached the engine",
            pick(id).what()
        );
    }
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    for label in ["Add marker", "Edit marker", "Remove marker"] {
        assert!(
            sent.contains(label),
            "the markers census pressed every control the panel draws and none of \
             them sent `{label}`"
        );
    }
}

#[test]
fn the_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "markers.rs");
}
