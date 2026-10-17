//! The Markers menu: In and Out marks, and the sequence's markers.

use std::borrow::Cow;

use crate::app::AppState;

use super::menu_guards as guards;
use super::menu_items::{MenuItem, MenuItems};

/// Report a failed edit on the status line.
fn report(state: &mut AppState, result: Result<(), aria_engine::EngineError>) {
    if let Err(err) = result {
        state.report_error(&err);
    }
}

static MARKER_ITEMS: &[MenuItem] = &[
    MenuItem::new("seq-mark-in", "Mark In", |_, s, _| s.mark_in_here()).key("key-mark-in"),
    MenuItem::new("seq-mark-out", "Mark Out", |_, s, _| s.mark_out_here()).key("key-mark-out"),
    MenuItem::new("mark-clip", "Mark Clip", |_, s, _| {
        s.mark_clip_at_playhead()
    })
    .key("key-mark-clip")
    .when(guards::clips),
    MenuItem::new("mark-go-in", "Go to In", |_, s, _| s.go_to_in())
        .key("key-mark-in-go")
        .when(guards::mark_in),
    MenuItem::new("mark-go-out", "Go to Out", |_, s, _| s.go_to_out())
        .key("key-mark-out-go")
        .when(guards::mark_out),
    MenuItem::new("seq-clear-in", "Clear In", |_, s, _| s.clear_mark_in_here())
        .key("key-mark-in-clear")
        .when(guards::mark_in),
    MenuItem::new("seq-clear-out", "Clear Out", |_, s, _| {
        s.clear_mark_out_here()
    })
    .key("key-mark-out-clear")
    .when(guards::mark_out),
    MenuItem::new("mark-clear", "Clear In and Out", |_, s, _| {
        s.clear_marks_here()
    })
    .key("key-mark-clear")
    .when(guards::marks),
    MenuItem::new("seq-marker", "Add Marker", |_, s, _| {
        let added = s.add_or_edit_marker();
        report(s, added)
    })
    .key("key-marker-add"),
    MenuItem::new("seq-marker-next", "Go to Next Marker", |_, s, _| {
        s.go_to_next_marker()
    })
    .key("key-marker-next")
    .when(guards::markers),
    MenuItem::new("seq-marker-prev", "Go to Previous Marker", |_, s, _| {
        s.go_to_prev_marker()
    })
    .key("key-marker-prev")
    .when(guards::markers),
    MenuItem::new("seq-marker-remove", "Clear Marker", |_, s, _| {
        let removed = s.remove_marker_at_playhead();
        report(s, removed)
    })
    .key("key-marker-remove")
    .when(guards::markers),
    MenuItem::new("marker-clear-all", "Clear All Markers", |_, s, _| {
        let cleared = s.clear_all_markers();
        report(s, cleared)
    })
    .key("key-marker-clear-all")
    .when(guards::markers),
    MenuItem::new("seq-marker-rename", "Edit Marker…", |_, s, _| {
        s.edit_marker_at_playhead()
    })
    .when(guards::markers),
    MenuItem::new(
        "seq-marker-snap",
        "Move nearest marker to playhead",
        |_, s, _| crate::commands::reach::snap_marker(s),
    )
    .when(guards::markers),
];

pub(super) fn marker_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(MARKER_ITEMS)
}
