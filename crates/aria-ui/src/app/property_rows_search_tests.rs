//! The property search in the timeline header.
//!
//! The class closed here: a search that filters one row kind and not its
//! siblings (a property row, a mask's Path row, a group header), that matches
//! by case, that opens clips left shut, that keeps a group with
//! nothing in it, or that writes over the outline each clip had open, so
//! clearing it leaves the rows changed. The expected rows are read from the
//! same clip with every group twirled open by hand, so a new property is
//! covered without naming it here.
//!
//! Not covered: the box's drawing, which the header's drawn tests own.

use super::super::{PropertyGroup, TimelineOutlineRow};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::keys::fields;

/// The names of the property and Path rows, in order.
fn names(rows: &[TimelineOutlineRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            TimelineOutlineRow::Property(row) => Some(row.label.to_string()),
            TimelineOutlineRow::MaskPath(row) => Some(row.label.to_string()),
            TimelineOutlineRow::GroupHeader { .. } => None,
        })
        .collect()
}

fn search(state: &mut AppState, text: &str) {
    state.focus_property_search(true);
    while !state.property_query().is_empty() {
        state.property_query_pop();
    }
    assert!(
        fields::insert(state, text),
        "the property search took {text:?}"
    );
}

/// A clip with its rows open, a mask on it so the Path row is in play, and
/// every group shut; with the rows the clip shows with every group open.
fn shut_clip() -> (AppState, u64, Vec<String>) {
    let (mut state, clip) = with_clip();
    let triangle = [[0.2, 0.2], [0.8, 0.2], [0.5, 0.8]]
        .map(aria_models::ShapePathPoint::corner)
        .to_vec();
    state.add_selected_mask_path(triangle).expect("a path mask");
    let track = state.store.sequence.tracks[0].id;
    state.toggle_clip_outline(clip.raw());
    for group in PropertyGroup::ALL {
        let open = state.track_outline_rows(track).iter().any(|row| {
            matches!(row, TimelineOutlineRow::GroupHeader { group: g, expanded: true, .. } if *g == group)
        });
        if !open {
            state.toggle_property_group(clip, group);
        }
    }
    let all = names(&state.track_outline_rows(track));
    for group in PropertyGroup::ALL {
        state.toggle_property_group(clip, group);
    }
    assert!(
        names(&state.track_outline_rows(track)).is_empty(),
        "every group is shut before the search"
    );
    (state, track, all)
}

#[test]
fn a_search_shows_every_matching_row_of_an_open_clip_in_any_case() {
    let (mut state, track, all) = shut_clip();
    assert!(
        all.iter().any(|name| name.ends_with("Path")),
        "the fixture draws a mask Path row: {all:?}"
    );
    for typed in ["pos", "POS", "Path", "a", "opacity", "  scale  "] {
        search(&mut state, typed);
        let needle = typed.trim().to_lowercase();
        let want: Vec<String> = all
            .iter()
            .filter(|name| name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        assert!(!want.is_empty(), "{typed:?} matches something on the clip");
        let rows = state.track_outline_rows(track);
        assert_eq!(names(&rows), want, "{typed:?}");
        // Every group drawn is open and holds a match.
        for (at, row) in rows.iter().enumerate() {
            if let TimelineOutlineRow::GroupHeader {
                group, expanded, ..
            } = row
            {
                assert!(*expanded, "{typed:?} drew {group:?} shut");
                assert!(
                    !matches!(
                        rows.get(at + 1),
                        None | Some(TimelineOutlineRow::GroupHeader { .. })
                    ),
                    "{typed:?} drew {group:?} with nothing under it"
                );
            }
        }
    }
}

#[test]
fn a_search_with_no_match_draws_no_rows_and_a_shut_clip_stays_shut() {
    let (mut state, track, _) = shut_clip();
    search(&mut state, "zzzz");
    assert_eq!(state.track_outline_rows(track), Vec::new());

    // A clip whose rows are shut is not opened by the search.
    let clip = state.store.sequence.tracks[0].clips[0].id;
    state.toggle_clip_outline(clip);
    search(&mut state, "pos");
    assert_eq!(state.track_outline_rows(track), Vec::new());
}

#[test]
fn clearing_the_search_brings_back_the_groups_the_clip_had_open() {
    let (mut state, track, _) = shut_clip();
    let clip = aria_models::ClipId::from_raw(state.store.sequence.tracks[0].clips[0].id);
    state.toggle_property_group(clip, PropertyGroup::Transform);
    let before = state.track_outline_rows(track);
    search(&mut state, "a");
    assert_ne!(
        state.track_outline_rows(track),
        before,
        "the search filtered"
    );

    // Esc and Enter give the keys back and keep the filter.
    let field = fields::focused(&state).expect("the search holds the keys");
    assert_eq!(field.id, "aria.field.property-search");
    (field.escape)(&mut state);
    assert!(!state.property_search_focused());
    assert_eq!(state.property_query(), "a");
    assert_ne!(state.track_outline_rows(track), before);

    state.clear_property_query();
    assert_eq!(state.track_outline_rows(track), before);
}

#[test]
fn backspace_takes_the_search_back_one_character() {
    let (mut state, track, all) = shut_clip();
    search(&mut state, "posz");
    assert_eq!(state.track_outline_rows(track), Vec::new());
    assert!(fields::truncate(&mut state, 1));
    assert_eq!(state.property_query(), "pos");
    let want: Vec<String> = all
        .iter()
        .filter(|name| name.to_lowercase().contains("pos"))
        .cloned()
        .collect();
    assert_eq!(names(&state.track_outline_rows(track)), want);
}

#[test]
fn the_search_takes_the_keys_from_the_other_boxes() {
    let (mut state, _) = with_clip();
    state.focus_clock(true);
    state.focus_media_search(true);
    state.focus_property_search(true);
    assert!(state.property_search_focused());
    assert!(!state.clock_focused(), "the playhead field let go");
    assert!(!state.media_search_focus, "the bin search let go");
    assert_eq!(
        fields::focused(&state).map(|field| field.id),
        Some("aria.field.property-search")
    );
}
