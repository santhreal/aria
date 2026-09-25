//! Contracts for the adjustment-layer command.
//!
//! The class closed here: the model and the resolver have held adjustment lanes
//! for two milestones while no surface could make one, and the first attempt at
//! a surface aimed at a lane by the coarse projected kind, which collapses
//! sticker, effect, filter, and adjustment lanes into one — so the placement
//! would land on a sticker lane and be refused by the engine with an error the
//! operator cannot act on. These pin the lane the bar lands on, the span it
//! covers, and that a second layer never fails on an overlap.
//!
//! Not covered: how the bar draws, and what the resolver does with it —
//! `aria-render`'s resolve tests own the canvas pass.

use aria_models::TrackKind;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::UiTrackKind;

/// Every adjustment lane, top-first, with the spans of the bars on it.
fn lanes(state: &AppState) -> Vec<(String, Vec<(i64, i64)>)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == UiTrackKind::Adjustment)
        .map(|lane| {
            (
                lane.name.to_string(),
                lane.clips
                    .iter()
                    .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn the_layer_lands_on_its_own_lane_over_the_selection() {
    let (mut state, clip) = with_clip();
    let span = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|item| item.id == clip.raw())
        .map(|item| (item.timeline_start_ticks, item.timeline_end_ticks))
        .expect("the clip is projected");
    assert!(lanes(&state).is_empty());

    state.add_adjustment_layer().expect("place the layer");

    assert_eq!(lanes(&state), vec![("FX1".to_string(), vec![span])]);
}

#[test]
fn the_bar_is_an_adjustment_generator() {
    let (mut state, _) = with_clip();
    state.add_adjustment_layer().expect("place the layer");
    let generator = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == UiTrackKind::Adjustment)
        .flat_map(|lane| lane.clips.iter())
        .find_map(|clip| clip.generator.as_ref())
        .expect("the bar carries a generator");
    assert_eq!(generator.kind, "adjustment");
}

#[test]
fn a_marked_range_wins_over_the_selection() {
    let (mut state, _) = with_clip();
    state.set_playhead(6);
    state.set_mark_in();
    state.set_playhead(18);
    state.set_mark_out();

    state.add_adjustment_layer().expect("place the layer");

    assert_eq!(lanes(&state), vec![("FX1".to_string(), vec![(6, 18)])]);
}

#[test]
fn with_nothing_marked_or_selected_the_layer_covers_the_sequence() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    let duration = state.store.sequence.duration_ticks;
    assert!(duration > 0, "the fixture places one clip");

    state.add_adjustment_layer().expect("place the layer");

    assert_eq!(
        lanes(&state),
        vec![("FX1".to_string(), vec![(0, duration)])]
    );
}

#[test]
fn a_second_layer_over_the_same_range_stacks_onto_a_fresh_lane() {
    let (mut state, _) = with_clip();
    state.set_playhead(0);
    state.set_mark_in();
    state.set_playhead(12);
    state.set_mark_out();

    state.add_adjustment_layer().expect("first layer");
    state
        .add_adjustment_layer()
        .expect("an overlap must not surface an engine error");

    let lanes = lanes(&state);
    assert_eq!(lanes.len(), 2, "the second bar needed a lane of its own");
    for (name, spans) in &lanes {
        assert_eq!(spans, &vec![(0, 12)], "{name} holds the wrong span");
    }
    assert_eq!(lanes[1].0, "FX1");
    assert_eq!(lanes[0].0, "FX2", "the newer lane sits above the older one");
}

#[test]
fn a_second_layer_over_a_free_range_reuses_the_lane() {
    let (mut state, _) = with_clip();
    state.set_playhead(0);
    state.set_mark_in();
    state.set_playhead(12);
    state.set_mark_out();
    state.add_adjustment_layer().expect("first layer");

    state.set_playhead(24);
    state.set_mark_in();
    state.set_playhead(36);
    state.set_mark_out();
    state.add_adjustment_layer().expect("second layer");

    let lanes = lanes(&state);
    assert_eq!(lanes.len(), 1, "a free range must not open a second lane");
    let mut spans = lanes[0].1.clone();
    spans.sort();
    assert_eq!(spans, vec![(0, 12), (24, 36)]);
}

#[test]
fn the_layer_is_one_undoable_step() {
    let (mut state, _) = with_clip();
    let before = state.history_depth();
    state.add_adjustment_layer().expect("place the layer");
    assert!(state.history_depth() > before, "the layer is an edit");

    while state.history_depth() > before {
        assert!(state.undo(), "undo must walk back to the opening state");
    }
    assert!(
        lanes(&state).iter().all(|(_, spans)| spans.is_empty()),
        "undo left a bar behind"
    );
}

#[test]
fn an_adjustment_lane_is_never_mistaken_for_a_sticker_lane() {
    // The projected kind is what a lane lookup matches on, so the two must not
    // collapse: the engine refuses an adjustment bar on a sticker lane.
    let (mut state, _) = with_clip();
    state
        .add_lane_of_kind(TrackKind::Sticker, None)
        .expect("a sticker lane");

    state.add_adjustment_layer().expect("place the layer");

    assert_eq!(lanes(&state).len(), 1);
    let sticker_bars: usize = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == UiTrackKind::Generator)
        .map(|lane| lane.clips.len())
        .sum();
    assert_eq!(sticker_bars, 0, "the bar landed on the sticker lane");
}

#[test]
fn the_menu_and_the_palette_both_reach_it() {
    let (state, _) = with_clip();
    let labels = crate::chrome::labels_for(crate::types::AppMenu::Sequence, &state);
    assert!(
        labels.contains(&"Add Adjustment Layer"),
        "the Sequence menu has no adjustment layer row: {labels:?}"
    );
    assert!(
        crate::commands::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.id == "pal-adjustment-layer"),
        "the palette has no way to add an adjustment layer"
    );
}
