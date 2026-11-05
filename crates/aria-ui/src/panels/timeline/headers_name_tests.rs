//! A lane header draws a lane's name only when the name adds to its badge.
//!
//! The class closed here is a new lane that reads its kind twice: a badge from
//! the UI's lane grouping beside a default name from the model's catalog, which
//! disagree for a kind whose badge is not its name's stem (a sticker lane read
//! "G1 S1", an adjustment lane "ADJ1 FX1"). The kinds come from
//! [`aria_models::track_kind_specs`], so a lane kind added later is made here
//! and checked without an edit to this file.
//!
//! Not caught: a lane renamed after another lane's default ("V3" on the lane
//! badged V1) draws no name, by the same rule that hides "V2" on a lane
//! reordered to V1.

use aria_models::track_kind_specs;
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::types::UiTrack;

use super::shown_name;

fn project() -> AppState {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("lane-names"),
    )
    .expect("open a state");
    state.new_project().expect("open a project");
    state
}

/// The lanes not in `before`, as the header draws each: its name, its badge,
/// and the name drawn beside the badge.
fn drawn_new(state: &AppState, before: &[u64]) -> Vec<(String, String, String)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, lane)| !before.contains(&lane.id))
        .map(|(row, lane)| {
            let badge = state.lane_badge_at(row);
            let name = shown_name(lane, &badge).to_string();
            (lane.name.to_string(), badge, name)
        })
        .collect()
}

#[test]
fn a_lane_made_with_its_kinds_name_draws_its_badge_alone() {
    let mut state = project();
    // The new project's own lanes ("Main" among them) are named by the
    // project, not by their kind.
    let before: Vec<u64> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| lane.id)
        .collect();
    for spec in track_kind_specs() {
        // Both ways a lane is made: with the kind's default name, as a clip
        // placed with no lane for it makes one, and with the stem and the next
        // ordinal, as "add a lane" does.
        state
            .add_track(spec.kind, spec.default_name)
            .unwrap_or_else(|err| panic!("add a {} lane: {err}", spec.id));
        state
            .add_lane_of_kind(spec.kind, None)
            .unwrap_or_else(|err| panic!("add a {} lane: {err}", spec.id));
    }
    let lanes = drawn_new(&state, &before);
    for spec in track_kind_specs() {
        assert!(
            lanes.iter().any(|(name, _, _)| *name == spec.default_name),
            "no '{}' lane was made: {lanes:?}",
            spec.default_name
        );
    }
    let named: Vec<_> = lanes
        .iter()
        .filter(|(_, _, shown)| !shown.is_empty())
        .collect();
    assert!(
        named.is_empty(),
        "these new lanes draw a name beside the badge: {named:?}"
    );
}

#[test]
fn a_name_the_kind_does_not_make_is_drawn() {
    let state = project();
    let lane: &UiTrack = state
        .store
        .sequence
        .tracks
        .first()
        .expect("a new project has a lane");
    let badge = state.lane_badge_at(0);
    let foreign = track_kind_specs()
        .iter()
        .find(|spec| crate::types::UiTrackKind::from_model(spec.kind) != lane.kind)
        .expect("another lane kind");
    let stem = track_kind_specs()
        .iter()
        .find(|spec| crate::types::UiTrackKind::from_model(spec.kind) == lane.kind)
        .expect("the lane's own kind")
        .name_stem();
    for name in [
        "Dialogue".to_string(),
        stem.to_string(),
        format!("{stem}1 B"),
        format!("{stem}x"),
        foreign.default_name.to_string(),
    ] {
        let mut renamed = lane.clone();
        renamed.name = name.clone().into();
        assert_eq!(
            shown_name(&renamed, &badge).as_ref(),
            name,
            "the {badge} lane named '{name}' draws no name"
        );
    }
}
