//! Driving the source monitor's bar: the two marks, the clear, and the two
//! edits that land the marked source.
//!
//! # Why
//!
//! Three commands claimed this bar and nothing pressed it, and one of them
//! claimed a control that was never drawn: there is no fit-to-fill button. A fit
//! is what Insert and Overwrite do when the sequence carries a marked range of
//! its own, so the state the row is pressed against decides which of three
//! commands it sends. That makes the dressing part of the row
//! ([`Dress`]), and it is why the same two picks appear twice in the table.
//!
//! The marking rows send nothing: a mark is the monitor's own view of the
//! source, held in session state, and no project changes when one moves. They
//! are in the table anyway, with what they must do instead, because a mark
//! button wired to the wrong end is exactly the defect a census of the edits
//! alone cannot see.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::fixtures::marked_source;
use crate::app::sent_census::Watch;
use crate::surfaces::source_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "source_pick.rs";

/// The state a row is driven against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dress {
    /// The monitor open with a marked source, and nothing marked on the
    /// sequence: an edit lands at the playhead at the source's own length.
    Loaded,
    /// The same, with a range marked on the sequence: the destination has a
    /// length, so an edit retimes the source to fill it.
    Fitted,
}

impl Dress {
    /// Build the state this dressing names, with the pool id the monitor holds.
    fn state(self) -> (AppState, u64) {
        let (mut state, media) = marked_source();
        if self == Dress::Fitted {
            // A range wide enough that filling it retimes the source rather
            // than landing it at its own length.
            state.set_mark_in();
            state.set_playhead(48);
            state.set_mark_out();
            assert!(
                state.marked_range().is_some(),
                "the sequence carries a range to fill"
            );
        }
        (state, media)
    }
}

/// What a row does when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lands {
    /// One command on the timeline, by that label.
    Command(&'static str),
    /// Nothing on the project: the monitor's own marks move.
    Marks,
}

/// Every control the bar draws, with the state it is driven against and what it
/// must do there.
///
/// Both edits appear twice because the sequence's own marks change which command
/// they send, and a row that only reached one of the two would leave the other
/// unproven.
const ROWS: &[(Pick, Dress, Lands)] = &[
    (Pick::MarkIn, Dress::Loaded, Lands::Marks),
    (Pick::MarkOut, Dress::Loaded, Lands::Marks),
    (Pick::ClearMarks, Dress::Loaded, Lands::Marks),
    (Pick::Insert, Dress::Loaded, Lands::Command("Ripple insert")),
    (Pick::Overwrite, Dress::Loaded, Lands::Command("Overwrite")),
    (Pick::Insert, Dress::Fitted, Lands::Command("Fit to fill")),
    (
        Pick::Overwrite,
        Dress::Fitted,
        Lands::Command("Fit to fill"),
    ),
];

/// Press one control the way the bar does.
fn press(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    source_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
}

/// The monitor's own marks, as the bar draws them.
fn marks(state: &AppState) -> (Option<i64>, Option<i64>) {
    let view = state
        .source_view
        .as_ref()
        .expect("the monitor stays open across a press");
    (view.mark_in, view.mark_out)
}

/// Every command the bar's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for (pick, dress, _) in ROWS {
        let (mut state, _) = dress.state();
        let watch = Watch::on(&state);
        press(&mut state, *pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_control_the_bar_draws_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not the table drives it.
    let pressed: BTreeSet<String> = ROWS
        .iter()
        .map(|(pick, _, _)| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these monitor controls are drawn and the table drives none of them: \
         {missing:?}. The answer is a row with the state it needs, never an \
         exception"
    );
}

#[test]
fn an_edit_row_sends_the_command_the_table_names() {
    // The whole point of the second dressing: the same button is three
    // different edits depending on what is marked, and a fit that quietly
    // became an overwrite would leave the clip at the wrong length.
    for (pick, dress, lands) in ROWS {
        let Lands::Command(label) = lands else {
            continue;
        };
        let (mut state, _) = dress.state();
        let watch = Watch::on(&state);
        let what = pick.what();
        press(&mut state, *pick);
        let sent = watch.take();
        assert!(
            sent.contains(label),
            "{what} against {dress:?} sent {sent:?} rather than `{label}`"
        );
    }
}

#[test]
fn a_marking_row_moves_a_mark_and_asks_for_nothing() {
    // A mark is session state, so the fingerprint is the monitor's own pair. A
    // marking row that reached the engine would put an undo entry on the stack
    // for a click that edited no project.
    for (pick, dress, lands) in ROWS {
        if *lands != Lands::Marks {
            continue;
        }
        let (mut state, media) = dress.state();
        // Park the monitor where neither mark is. A mark button writes the
        // monitor's own tick, so pressing it where the mark already stands is
        // the same value again: an idempotent press proves nothing about which
        // end the button owns.
        state.hover_pool_item(media, 30);
        let before = marks(&state);
        let watch = Watch::on(&state);
        let what = pick.what();
        press(&mut state, *pick);
        assert!(
            watch.take().is_empty(),
            "{what} reached the engine, so marking the source writes history"
        );
        assert_ne!(
            before,
            marks(&state),
            "{what} left both marks where they were"
        );
    }
}

#[test]
fn the_marks_land_on_the_ends_they_name() {
    // Which end each button writes, asserted once: the two are drawn side by
    // side and swapping them is invisible to every test above.
    let (mut state, _) = Dress::Loaded.state();
    press(&mut state, Pick::ClearMarks);
    assert_eq!(marks(&state), (None, None), "the clear drops both");

    press(&mut state, Pick::MarkIn);
    let (in_point, out_point) = marks(&state);
    assert!(in_point.is_some(), "In wrote no in point");
    assert_eq!(out_point, None, "In wrote the out point too");

    press(&mut state, Pick::MarkOut);
    let (_, out_point) = marks(&state);
    assert!(out_point.is_some(), "Out wrote no out point");
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    for label in ["Ripple insert", "Overwrite", "Fit to fill"] {
        assert!(
            sent.contains(label),
            "the monitor census pressed every control the bar draws and none of \
             them sent `{label}`"
        );
    }
}

#[test]
fn the_bar_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "preview_source.rs");
}
