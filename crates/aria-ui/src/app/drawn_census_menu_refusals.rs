//! What the context menu's offered rows answer when the edit is refused.
//!
//! WHY: closes, for the context menus, the class of a row offered over an edit
//! the engine rejects, whose error line reports a failure the menu invited,
//! and of a structural refusal shown as the value copy, "That value isn't
//! accepted here. Enter another value", whose fix names a field a menu row does
//! not have: an extraction from a clip that shares its link, a freeze frame or
//! extraction on a locked lane. Every row the census presses is held to the
//! first in every dressed state that offers it, and every clip row to the
//! second over a clip whose own lane, or every other lane, is locked, so a new
//! row or stage is held to both with no edit here.
//!
//! What this does not catch: a refusal reached only in a state no stage
//! dresses, and a refusal worded by the model that a typed variant should
//! carry but that does not read the value copy.

use super::pressed;
use crate::app::error_copy::describe_engine_error;
use crate::app::fixtures;
use crate::app::menu_reach_tests::value_copy;
use crate::surfaces::context_menu::{CLIP_ITEMS, ClipMenu, MenuItem};

/// A row the menu offers does not fail when pressed: the offer and the edit
/// read one state.
#[test]
fn an_offered_context_row_never_fails() {
    let wrong: Vec<String> = pressed()
        .refusals
        .iter()
        .map(|(at, copy)| format!("{at}: {copy}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "offered, then failed:\n{}",
        wrong.join("\n")
    );
}

/// A clip row pressed over a clip on a locked lane, or over a clip whose sound
/// would land on a locked lane, is refused with copy that names the lock, or
/// acts. The editor mounts every built-in plugin, as the product does.
#[test]
fn a_clip_row_over_a_locked_lane_never_answers_with_the_value_copy() {
    let copy = value_copy();
    let (mut refused, mut wrong) = (0usize, Vec::new());
    for own in [true, false] {
        let what = if own {
            "its own lane locked"
        } else {
            "every other lane locked"
        };
        // A refused press leaves the state as it was, so the state is built
        // again only after a press that acted.
        let mut dressed = fixtures::locked_around_video(own);
        for row in CLIP_ITEMS.iter().filter_map(|item| match item {
            MenuItem::Row(row) => Some(row),
            _ => None,
        }) {
            let (state, clip) = &mut dressed;
            let facts = ClipMenu::of(state, clip.raw());
            if !(row.when)(&facts) {
                continue;
            }
            match (row.run)(state, &facts) {
                Ok(()) => dressed = fixtures::locked_around_video(own),
                Err(err) => {
                    refused += 1;
                    if describe_engine_error(&err) == copy {
                        wrong.push(format!("{} with {what}", row.id));
                    }
                }
            }
        }
    }
    assert!(
        refused > 0,
        "no clip row was refused over a locked lane, so the census read no refusal"
    );
    assert!(
        wrong.is_empty(),
        "refused with the value copy:\n{}",
        wrong.join("\n")
    );
}
