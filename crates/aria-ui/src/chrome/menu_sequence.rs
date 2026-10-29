//! The Sequence menu: rendering, edits at the playhead, the In to Out range,
//! tracks, and the sequence's nests.
//!
//! [`super::menu_items::items_for`] still dispatches here, so the bar, the
//! palette and the tests all reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::AppState;
use crate::commands::reach;

use super::menu_guards as guards;
use super::menu_items::{MenuItem, MenuItems, report};

/// Add a lane of `kind`, named after the lanes of that kind already there.
fn add_lane(state: &mut AppState, kind: aria_models::TrackKind) {
    let (shown, prefix) = match kind {
        aria_models::TrackKind::Audio => (crate::types::UiTrackKind::Audio, "A"),
        _ => (crate::types::UiTrackKind::Video, "V"),
    };
    let n = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|t| t.kind == shown)
        .count()
        + 1;
    let added = state.add_track(kind, format!("{prefix}{n}"));
    report(state, added)
}

const ADD_TRACKS: &str = "Add Tracks";
const NESTED: &str = "Nested Sequence";
const SOURCE: &str = "Source";
const DUCKING: &str = "Ducking";

/// The id prefix of a Sequence > Analyze row for one registered sequence analyzer.
pub(crate) const SEQ_ANALYZE_PREFIX: &str = "seq-analyze:";
const ANALYZE: &str = "Analyze";
static SEQUENCE_ITEMS: &[MenuItem] = &[
    MenuItem::new("seq-settings", "Sequence Settings…", |_, s, _| {
        s.open_settings_at("Canvas")
    }),
    MenuItem::new("seq-render-in-to-out", "Render In to Out", |_, s, _| {
        if let Err(err) = s.render_in_to_out() {
            s.status_message = Some(err);
        }
    })
    .key("key-render-in-to-out")
    .when(guards::clips),
    MenuItem::new("seq-render-selection", "Render Selection", |_, s, _| {
        if let Err(err) = s.render_selection() {
            s.status_message = Some(err);
        }
    })
    .when(guards::selection),
    MenuItem::new(
        "seq-delete-render-files",
        "Delete Render Files",
        |_, s, _| {
            if let Err(err) = s.delete_render_files() {
                s.status_message = Some(err);
            }
        },
    ),
    MenuItem::new("seq-add-edit", "Add Edit", |_, s, _| {
        let added = s.add_edit(false);
        report(s, added)
    })
    .key("key-split")
    .when(guards::clips),
    MenuItem::new("seq-add-edit-all", "Add Edit to All Tracks", |_, s, _| {
        let added = s.add_edit(true);
        report(s, added)
    })
    .key("key-split-all")
    .when(guards::clips),
    MenuItem::new("seq-trim-edit", "Trim Edit", |_, s, _| {
        s.select_nearest_edit()
    })
    .when(guards::clips),
    MenuItem::new("seq-extend-edit", "Extend Edit", |_, s, _| {
        let extended = s.extend_edit_to_playhead();
        report(s, extended)
    })
    .when(guards::edge),
    MenuItem::new(
        "seq-default-transitions",
        "Apply Default Transitions",
        |_, s, _| {
            let applied = s.add_default_transition(aria_models::TransitionMedia::Picture);
            report(s, applied)
        },
    )
    .key("key-transition")
    .when(guards::clips),
    MenuItem::new("seq-lift", "Lift", |_, s, _| {
        let lifted = s.remove_marked_range(false);
        report(s, lifted)
    })
    .key("key-lift")
    .when(guards::range),
    MenuItem::new("seq-extract", "Extract", |_, s, _| {
        let extracted = s.remove_marked_range(true);
        report(s, extracted)
    })
    .key("key-extract")
    .when(guards::range),
    MenuItem::new("seq-snap", "Snap", |_, s, _| s.toggle_snapping())
        .key("key-snap")
        .checked(|_, s| s.snap_enabled),
    MenuItem::new("seq-linked-selection", "Linked Selection", |_, s, _| {
        s.toggle_linked_selection()
    })
    .checked(|_, s| s.linked_selection),
    MenuItem::new("seq-v", "Video Track", |_, s, _| {
        add_lane(s, aria_models::TrackKind::Video)
    })
    .under(ADD_TRACKS),
    MenuItem::new("seq-a", "Audio Track", |_, s, _| {
        add_lane(s, aria_models::TrackKind::Audio)
    })
    .under(ADD_TRACKS),
    MenuItem::new("seq-delete-tracks", "Delete Empty Tracks", |_, s, _| {
        let deleted = s.delete_empty_tracks();
        report(s, deleted)
    })
    .when(guards::empty_tracks),
    MenuItem::new("seq-adjustment", "Add Adjustment Layer", |_, s, _| {
        let added = s.add_adjustment_layer().map(drop);
        report(s, added)
    }),
    MenuItem::new("seq-match-frame", "Match Frame", |_, s, _| {
        s.match_frame();
    })
    .key("key-match-frame")
    .when(guards::selection),
    MenuItem::new("seq-source-insert", "Insert", |_, s, _| {
        let cut = s.cut_source_in(crate::types::EditMode::Ripple);
        report(s, cut)
    })
    .when(guards::source_range)
    .under(SOURCE),
    MenuItem::new("seq-source-overwrite", "Overwrite", |_, s, _| {
        let cut = s.cut_source_in(crate::types::EditMode::Overwrite);
        report(s, cut)
    })
    .when(guards::source_range)
    .under(SOURCE),
    MenuItem::new("seq-ripple-insert", "Insert Pool Selection", |_, s, _| {
        reach::ripple_insert(s)
    })
    .when(guards::pool_selection)
    .under(SOURCE),
    MenuItem::new("seq-ripple-in", "Ripple Trim In", |_, s, _| {
        let trimmed = s.trim_selected_to_playhead(true);
        report(s, trimmed)
    })
    .when(guards::selection),
    MenuItem::new("seq-ripple-out", "Ripple Trim Out", |_, s, _| {
        let trimmed = s.trim_selected_to_playhead(false);
        report(s, trimmed)
    })
    .when(guards::selection),
    MenuItem::new(
        "seq-transition-crossfade",
        "Add Cross Fade at Cut",
        |_, s, _| {
            let set = s.set_selected_transition("crossfade");
            report(s, set)
        },
    )
    .when(guards::selection),
    MenuItem::new(
        "seq-transition-clear",
        "Remove Transition at Cut",
        |_, s, _| {
            let cleared = s.clear_selected_transition();
            report(s, cleared)
        },
    )
    .when(guards::selection),
    // Rows whose bodies live in `crate::commands::reach`, which the lane-shift
    // chords call too, so a row and its chord cannot drift into doing two
    // different things.
    MenuItem::new(
        "seq-lane-shift-later",
        "Shift Lane One Second Later",
        |_, s, _| reach::shift_lane(s, true),
    ),
    MenuItem::new(
        "seq-lane-shift-earlier",
        "Shift Lane One Second Earlier",
        |_, s, _| reach::shift_lane(s, false),
    ),
    MenuItem::new(
        "seq-canvas-background",
        "Reset Canvas Background",
        |_, s, _| reach::reset_canvas_background(s),
    ),
    MenuItem::new("seq-duck-amount", "Cycle Duck Amount", |_, s, _| {
        s.cycle_duck_amount()
    })
    .under(DUCKING),
    MenuItem::new("seq-duck-attack", "Cycle Duck Attack", |_, s, _| {
        s.cycle_duck_attack()
    })
    .under(DUCKING),
    MenuItem::new("seq-duck-release", "Cycle Duck Release", |_, s, _| {
        s.cycle_duck_release()
    })
    .under(DUCKING),
    MenuItem::new("seq-nest-open", "Open Nested Sequence", |_, s, _| {
        reach::open_selected_nest(s)
    })
    .under(NESTED),
    MenuItem::new("seq-nest-leave", "Leave Nested Sequence", |_, s, _| {
        reach::leave_sequence(s)
    })
    .under(NESTED),
    MenuItem::new("seq-nest-place", "Place Nest at Playhead", |_, s, _| {
        reach::place_selected_nest(s)
    })
    .under(NESTED),
];

fn run_sequence_analyzer(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    if let Some(analyzer_id) = id.strip_prefix(SEQ_ANALYZE_PREFIX)
        && let Err(err) = state.run_registered_analyzer(analyzer_id, cx)
    {
        state.report_error(err);
    }
}

pub(super) fn sequence_items(state: &AppState) -> MenuItems {
    let analyzers: Vec<_> = state
        .ctx
        .analyzers()
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.spec().scope,
                aria_plugin::AnalyzerScope::Sequence | aria_plugin::AnalyzerScope::InOut
            )
        })
        .collect();
    if analyzers.is_empty() {
        return Cow::Borrowed(SEQUENCE_ITEMS);
    }
    let intern = crate::interned::intern;
    let mut items = Vec::with_capacity(SEQUENCE_ITEMS.len() + analyzers.len());
    items.extend_from_slice(SEQUENCE_ITEMS);
    for entry in analyzers {
        let spec = entry.spec();
        items.push(
            MenuItem::new(
                intern(&format!("{SEQ_ANALYZE_PREFIX}{}", spec.id)),
                intern(&spec.label),
                run_sequence_analyzer,
            )
            .when(guards::clips)
            .under(ANALYZE),
        );
    }
    Cow::Owned(items)
}
