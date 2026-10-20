//! What the pool's selection does under the three writes a click makes.
//!
//! # Why
//!
//! The class this closes is a selection that can only ever hold one item while
//! several surfaces claim to read a set. These press the set directly: no state,
//! no device, no list. The list's own order is passed in, because a span is a run
//! of what the list showed.
//!
//! What these cannot see: which write a click's modifiers ask for, and whether
//! the click also places the clip. Both belong to
//! [`pool_pick`](crate::surfaces::pool_pick) and are pressed by its census.

use super::pool_selection::PoolSelection;

/// The pool as the list draws it.
const VISIBLE: [u64; 5] = [10, 20, 30, 40, 50];

#[test]
fn a_click_replaces_whatever_was_selected() {
    let mut selection = PoolSelection::default();
    selection.toggle(10);
    selection.toggle(20);
    selection.replace(30);
    assert_eq!(selection.ids(), [30]);
    assert_eq!(selection.focus(), Some(30));
}

#[test]
fn the_selection_keeps_the_order_the_operator_picked() {
    // A multicam group's angle order is this order. Sorting it would pick the
    // angle order without a pick, and the pool's sort is a viewing order, not
    // a camera order.
    let mut selection = PoolSelection::default();
    for id in [40, 10, 30] {
        selection.toggle(id);
    }
    assert_eq!(selection.ids(), [40, 10, 30]);
    assert_eq!(selection.len(), 3);
    assert_eq!(selection.focus(), Some(30), "the last pick is the focus");
}

#[test]
fn a_second_modified_click_drops_the_item_it_added() {
    let mut selection = PoolSelection::default();
    selection.toggle(10);
    selection.toggle(20);
    selection.toggle(10);
    assert_eq!(selection.ids(), [20]);
    assert!(!selection.contains(10));
}

#[test]
fn dropping_the_anchor_leaves_a_span_a_selected_item_to_run_from() {
    // A span from an unselected anchor would pick a run starting nowhere the
    // operator can see.
    let mut selection = PoolSelection::default();
    selection.toggle(10);
    selection.toggle(30);
    selection.toggle(30);
    assert_eq!(selection.focus(), Some(10));
    selection.span(20, &VISIBLE);
    assert_eq!(selection.ids(), [10, 20]);
}

#[test]
fn a_span_runs_along_the_list_in_either_direction() {
    let mut down = PoolSelection::default();
    down.replace(20);
    down.span(40, &VISIBLE);
    assert_eq!(down.ids(), [20, 30, 40]);

    let mut up = PoolSelection::default();
    up.replace(40);
    up.span(20, &VISIBLE);
    assert_eq!(
        up.ids(),
        [20, 30, 40],
        "a span upwards is the same run of cards"
    );
}

#[test]
fn a_span_from_one_anchor_repicks_rather_than_walking_away_from_it() {
    // Shift-clicking twice is how a run is corrected. An anchor that moved to
    // the last shift-click would grow the selection on every correction instead.
    let mut selection = PoolSelection::default();
    selection.replace(20);
    selection.span(50, &VISIBLE);
    selection.span(30, &VISIBLE);
    assert_eq!(selection.ids(), [20, 30]);
    assert_eq!(selection.focus(), Some(20));
}

#[test]
fn a_span_with_nothing_to_run_from_is_a_plain_click() {
    let mut empty = PoolSelection::default();
    empty.span(30, &VISIBLE);
    assert_eq!(empty.ids(), [30]);

    // An anchor the list is no longer showing: the folder changed under the
    // selection, so the run has no start.
    let mut gone = PoolSelection::default();
    gone.replace(99);
    gone.span(30, &VISIBLE);
    assert_eq!(gone.ids(), [30]);
}

#[test]
fn clearing_takes_the_anchor_with_it() {
    let mut selection = PoolSelection::default();
    selection.span(30, &VISIBLE);
    selection.clear();
    assert!(selection.is_empty());
    assert_eq!(selection.focus(), None, "a cleared selection kept a focus");
}
