//! Driving the Align panel: every edge and both distribute axes.
//!
//! # Why
//!
//! The panel's buttons are a registry
//! ([`align_pick`](crate::surfaces::align_pick)), and this presses every row of
//! it against three placed clips, reading the arms back out of the registry's
//! own source so a button added to the enum has no press and no green suite
//! until someone writes one. A button wired to nothing, or to a move that
//! leaves every clip where it was, turns `every_control_sends_something` red.
//!
//! Not covered: where the clips land, which `align::tests` measures on the
//! composited bounds.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::align::{AlignAxis, AlignEdge};
use crate::app::fixtures::spread_three;
use crate::app::sent_census::Watch;
use crate::surfaces::align_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "align_pick.rs";

/// Every control the panel draws, in the order it draws them.
fn picks() -> Vec<Pick> {
    AlignEdge::ALL
        .into_iter()
        .map(Pick::Align)
        .chain(AlignAxis::ALL.into_iter().map(Pick::Distribute))
        .collect()
}

/// Press one control the way the panel does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    align_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// Every command the panel's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for pick in picks() {
        let (mut state, _) = spread_three();
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
        "these Align controls are drawn and nothing presses them: {missing:?}. \
         The answer is a dressed project this census builds, never an exception"
    );
}

#[test]
fn every_control_sends_something() {
    // A button that dispatches nothing on three spread clips looks like it
    // aligned them and left them where they were.
    for pick in picks() {
        let (mut state, _) = spread_three();
        let watch = Watch::on(&state);
        press(&mut state, pick);
        assert!(
            !watch.take().is_empty(),
            "pressing {pick:?} sent nothing, so the control is inert"
        );
    }
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    assert!(
        sent.contains("Set parameter"),
        "the Align census pressed every control the panel draws and none of them \
         wrote a Position: {sent:?}"
    );
}

#[test]
fn the_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "align_panel.rs");
}
