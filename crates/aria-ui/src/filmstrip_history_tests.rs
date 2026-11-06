//! Contracts for filmstrip cells across undo, redo and a history jump.
//!
//! The class closed here: every history move dropped every filmstrip cell,
//! so one undo repainted the whole strip, one cell per frame. A move now
//! drops the cells inside the edits it changed back and keeps the rest: one
//! undo, one redo, an undo after a redo, a jump of one step and of two steps
//! either way, and an undo and a redo of one step made of two edits, named
//! or not.
//!
//! Not covered: a step recorded outside `apply_command` (a marker, an
//! assistant prompt), which drops every cell by the journal fallback that
//! `filmstrip_rollback_tests` pins.

use aria_engine::EngineError;

use super::filmstrip_rollback_tests::{Pictures, bounds, has, land, opacity, pictures};
use crate::app::AppState;

/// Run the history move `go`, check it dropped every cell of `dropped` and
/// kept every cell of `kept`, then land the dropped cells again, as the
/// timeline's next draw does.
fn moved(
    state: &mut AppState,
    name: &str,
    go: impl FnOnce(&mut AppState) -> bool,
    dropped: &[i64],
    kept: &[i64],
) {
    assert!(go(state), "{name} did not move");
    for &tick in dropped {
        assert!(
            !has(state, tick),
            "{name} kept the cell at {tick}, inside an edit it changed back"
        );
    }
    for &tick in kept {
        assert!(
            has(state, tick),
            "{name} dropped the cell at {tick}, outside every edit it changed back"
        );
    }
    for &tick in dropped {
        land(state, tick);
    }
}

#[test]
fn every_history_move_keeps_the_cells_it_did_not_change_back() {
    let Pictures {
        mut state,
        a,
        b,
        in_a,
        in_b,
    } = pictures();
    // A frame between the two pictures, which no edit here changes.
    let gap = bounds(&state, a).1 + 1;
    for tick in [in_a, in_b, gap] {
        land(&mut state, tick);
    }
    state.apply_command(opacity(b, 0.5)).expect("edit b");
    land(&mut state, in_b);
    state.apply_command(opacity(a, 0.2)).expect("edit a");
    land(&mut state, in_a);
    let top = state.history_depth();
    let one = [in_a];
    let two = [in_a, in_b];
    moved(&mut state, "an undo", AppState::undo, &one, &[in_b, gap]);
    moved(&mut state, "a redo", AppState::redo, &one, &[in_b, gap]);
    moved(
        &mut state,
        "an undo after a redo",
        AppState::undo,
        &one,
        &[in_b, gap],
    );
    moved(
        &mut state,
        "a redo after an undo",
        AppState::redo,
        &one,
        &[in_b, gap],
    );
    let jump = |depth: usize| move |state: &mut AppState| state.history_jump(depth);
    moved(
        &mut state,
        "a jump back one step",
        jump(top - 1),
        &one,
        &[in_b, gap],
    );
    moved(
        &mut state,
        "a jump forward one step",
        jump(top),
        &one,
        &[in_b, gap],
    );
    moved(
        &mut state,
        "a jump back two steps",
        jump(top - 2),
        &two,
        &[gap],
    );
    moved(
        &mut state,
        "a jump forward two steps",
        jump(top),
        &two,
        &[gap],
    );
    moved(
        &mut state,
        "an undo after the jumps",
        AppState::undo,
        &one,
        &[in_b, gap],
    );
}

#[test]
fn a_step_of_two_edits_moves_the_cells_of_both() {
    for named in [false, true] {
        let Pictures {
            mut state,
            a,
            b,
            in_a,
            in_b,
        } = pictures();
        let gap = bounds(&state, a).1 + 1;
        for tick in [in_a, in_b, gap] {
            land(&mut state, tick);
        }
        let edits = |state: &mut AppState| {
            state.apply_command(opacity(b, 0.5))?;
            state.apply_command(opacity(a, 0.2))?;
            Ok::<(), EngineError>(())
        };
        if named {
            state.one_step_as("Fade both", edits)
        } else {
            state.one_step(edits)
        }
        .expect("one step");
        for tick in [in_a, in_b] {
            land(&mut state, tick);
        }
        let case = if named { "named" } else { "unnamed" };
        let both = [in_a, in_b];
        moved(
            &mut state,
            &format!("{case}: an undo"),
            AppState::undo,
            &both,
            &[gap],
        );
        moved(
            &mut state,
            &format!("{case}: a redo"),
            AppState::redo,
            &both,
            &[gap],
        );
    }
}
