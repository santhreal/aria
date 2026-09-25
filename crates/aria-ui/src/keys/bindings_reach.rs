//! Chords for capabilities that had no key at all.
//!
//! One of the tables [`super::bindings::SHIPPED_BINDINGS`] joins. Each of these
//! actions was added to the palette and the Sequence menu in the same change;
//! this table is the third registry, and it holds only the ones a chord earns —
//! the palette names every reach row, and a keyboard has no room for all of them.
//!
//! Every chord here was checked against the shipped table before it was written.
//! The check is not a courtesy: `KeyHost::add` refuses a chord that is already
//! taken and records the refusal, so a collision here would ship as a key that
//! silently does nothing, and `the_shipped_table_has_no_internal_conflict` in
//! `super::bindings` is what turns that into a red test instead.
//!
//! `After::Nothing` throughout. The two side effects a key can carry are starting
//! the playback clock and asking for a fresh preview frame, and neither of these
//! is a transport or a mark — the shell already respawns the preview after a key
//! that edited the timeline.

use super::bind;
use super::{Binding, Chord};
use crate::app::analysis::AnalysisKind;
use crate::commands::reach;

/// Chords for the reach rows that earn one.
pub(super) const REACH_ROWS: &[Binding] = &[
    bind!(
        "key-ripple-insert",
        Chord::shift(","),
        Editor,
        reach::ripple_insert
    ),
    bind!(
        "key-marker-snap",
        Chord::ctrl_alt("m"),
        Editor,
        reach::snap_marker
    ),
    bind!(
        "key-lane-shift-later",
        Chord::alt("right"),
        Editor,
        |state| reach::shift_lane(state, true)
    ),
    bind!(
        "key-lane-shift-earlier",
        Chord::alt("left"),
        Editor,
        |state| reach::shift_lane(state, false)
    ),
    bind!("key-duck-amount", Chord::alt("d"), Editor, |state| state
        .cycle_duck_amount()),
    // Shift+Alt is where work lives: every chord that queues a job is on it, so
    // a key that starts something is never one modifier away from a key that
    // edits. Ducking sits beside `key-duck-amount` — Alt+D until the number is
    // right, Shift+Alt+D to apply it — and the detectors follow their own
    // initials. `every_detector_has_a_chord` is red until a new one is here.
    bind!("key-duck-apply", Chord::shift_alt("d"), Editor, windowed: |state, cx| {
        if let Err(err) = state.duck_selected(cx) {
            state.status_message = Some(err);
        }
    }),
    bind!("key-detect-shots", Chord::shift_alt("s"), Editor, windowed: |state, cx| state.run_detector(AnalysisKind::Shots, cx)),
    bind!("key-detect-silence", Chord::shift_alt("q"), Editor, windowed: |state, cx| state.run_detector(AnalysisKind::Silence, cx)),
    bind!("key-detect-beats", Chord::shift_alt("b"), Editor, windowed: |state, cx| state.run_detector(AnalysisKind::Beats, cx)),
    bind!(
        "key-effect-reset",
        Chord::alt("r"),
        Editor,
        reach::reset_effects
    ),
];
