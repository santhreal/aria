//! Contracts for the adjustment-layer command.
//!
//! The class closed here: the model and the resolver have held adjustment lanes
//! for two milestones while no surface could make one, and the first attempt at
//! a surface aimed at a lane by the coarse projected kind, which collapses
//! sticker, effect, filter, and adjustment lanes into one — so the placement
//! would land on a sticker lane and be refused by the engine with an error
//! that no next step can resolve. These pin the lane the bar lands on, the span it
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

    // Out at 18 keeps frame 18.
    assert_eq!(lanes(&state), vec![("FX1".to_string(), vec![(6, 19)])]);
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
        assert_eq!(spans, &vec![(0, 13)], "{name} holds the wrong span");
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
    assert_eq!(spans, vec![(0, 13), (24, 37)]);
}

#[test]
fn the_layer_is_one_undoable_step() {
    let (mut state, _) = with_clip();
    let before = state.history_depth();
    state.add_adjustment_layer().expect("place the layer");
    assert_eq!(
        state.history_depth(),
        before + 1,
        "adding a layer and its lane is one undo step"
    );
    assert_ne!(
        state.next_undo_label().as_deref(),
        Some("Add track"),
        "the undo step is named after the layer placement, not the lane add"
    );
    assert!(state.undo(), "undo must walk back to the opening state");
    assert!(
        lanes(&state).is_empty(),
        "one undo removes both the bar and the lane it created"
    );
}

/// Every public `AppState` verb that can call `ensure_kind_track`,
/// `free_kind_track`, `drop_target_track`, or `add_lane_of_kind` and then edits
/// runs in one `one_step_named` step named after the edit: `history_depth`
/// rises by 1, the step is never named `Add track`, and one undo leaves no
/// newly created lane behind.
#[test]
fn every_placement_verb_that_adds_a_lane_is_one_step_named_after_the_edit() {
    use aria_commands::{Command, EditCommand};
    use aria_models::{Generator, MediaId, MulticamAngle, SequenceId, TrackId};

    use super::captions::{CaptionCommit, UiCaption};

    let track_ids = |state: &AppState| -> Vec<u64> {
        state
            .store
            .sequence
            .tracks
            .iter()
            .map(|lane| lane.id)
            .collect()
    };
    let remove_kind = |state: &mut AppState, kind: UiTrackKind| {
        let ids: Vec<u64> = state
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == kind)
            .map(|lane| lane.id)
            .collect();
        for id in ids {
            state
                .apply_command(Command::Edit(EditCommand::RemoveTrack {
                    track: TrackId::from_raw(id),
                }))
                .expect("remove track");
        }
    };
    let assert_one_step = |state: &mut AppState,
                           name: &str,
                           expected_label: &str,
                           run: &mut dyn FnMut(&mut AppState)| {
        let before_depth = state.history_depth();
        let before_tracks = track_ids(state);
        run(state);
        assert_eq!(
            state.history_depth(),
            before_depth + 1,
            "{name} took more than one undo step"
        );
        let label = state.next_undo_label();
        assert_ne!(
            label.as_deref(),
            Some("Add track"),
            "{name} named its undo step after the lane add"
        );
        assert_eq!(
            label.as_deref(),
            Some(expected_label),
            "{name} named its undo step {label:?}"
        );
        assert!(state.undo(), "{name} must undo");
        assert_eq!(
            track_ids(state),
            before_tracks,
            "{name} left a lane behind after one undo"
        );
    };

    // 1. commit_captions (TextClips and Markers) with multiple captions and no Text lane.
    {
        let (mut state, _) = with_clip();
        state.captions.segments = vec![
            UiCaption {
                start_tick: 0,
                end_tick: 24,
                text: "first".into(),
                confidence: None,
                words: Vec::new(),
            },
            UiCaption {
                start_tick: 24,
                end_tick: 48,
                text: "second".into(),
                confidence: None,
                words: Vec::new(),
            },
        ];
        state.set_caption_commit(CaptionCommit::TextClips);
        assert_one_step(
            &mut state,
            "commit_captions(TextClips)",
            "Commit captions",
            &mut |s| s.commit_captions().expect("commit text clips"),
        );
        state.set_caption_commit(CaptionCommit::Markers);
        assert_one_step(
            &mut state,
            "commit_captions(Markers)",
            "Commit captions",
            &mut |s| s.commit_captions().expect("commit markers"),
        );
    }

    // 2. place_multicam_group when no Video lane exists, and when V1 is occupied.
    {
        let (mut state, _) = with_clip();
        let media = MediaId::from_raw(state.store.media[0].id);
        let group = state
            .create_multicam_group("Cam group", vec![MulticamAngle::new(media, 0)])
            .expect("create group")
            .expect("group id");
        state.set_playhead(0);
        // V1 is occupied at tick 0, so place_multicam_group adds V2 on top.
        assert_one_step(
            &mut state,
            "place_multicam_group(occupied)",
            "Place multicam clip",
            &mut |s| s.place_multicam_group(group).expect("place multicam"),
        );
        // Remove all Video lanes so drop_target_track must create V1.
        remove_kind(&mut state, UiTrackKind::Video);
        assert_one_step(
            &mut state,
            "place_multicam_group(no_video_lane)",
            "Place multicam clip",
            &mut |s| s.place_multicam_group(group).expect("place multicam"),
        );
    }

    // 3. place_nested_sequence when no Video lane exists.
    {
        let (mut state, clip) = with_clip();
        state.select_clip(clip);
        state.nest_selection("Sub").expect("nest selection");
        let seq = SequenceId::from_raw(state.store.sequences[0].id);
        remove_kind(&mut state, UiTrackKind::Video);
        assert_one_step(
            &mut state,
            "place_nested_sequence(no_video_lane)",
            "Place nested sequence",
            &mut |s| s.place_nested_sequence(seq).expect("place nest"),
        );
    }

    // 4. add_adjustment_layer when no Adjustment lane exists, and when FX1 is occupied.
    {
        let (mut state, _) = with_clip();
        assert_one_step(
            &mut state,
            "add_adjustment_layer(no_adjustment_lane)",
            "Add generated clip",
            &mut |s| {
                s.add_adjustment_layer().expect("add adjustment layer");
            },
        );
        state
            .add_adjustment_layer()
            .expect("first adjustment layer");
        assert_one_step(
            &mut state,
            "add_adjustment_layer(occupied)",
            "Add generated clip",
            &mut |s| {
                s.add_adjustment_layer().expect("second adjustment layer");
            },
        );
    }

    // 5. Generated, text tool, and media drop verbs when no lane of their kind exists.
    {
        let (mut state, _) = with_clip();
        assert_one_step(
            &mut state,
            "add_generated(Text)",
            "Add generated clip",
            &mut |s| {
                s.add_generated(Generator::text("Title".to_string()))
                    .expect("add title");
            },
        );
        assert_one_step(
            &mut state,
            "create_text_at",
            "Add generated clip",
            &mut |s| {
                s.create_text_at([100.0, 100.0]).expect("create text at");
            },
        );
        let media = MediaId::from_raw(state.store.media[0].id);
        assert_one_step(
            &mut state,
            "drop_media_on_new_lane",
            "Place media",
            &mut |s| {
                s.drop_media_on_new_lane(media, true, 0)
                    .expect("drop on new lane");
            },
        );
        remove_kind(&mut state, UiTrackKind::Video);
        assert_one_step(
            &mut state,
            "drop_media_on_timeline(no_video_lane)",
            "Place media",
            &mut |s| {
                s.drop_media_on_timeline(media, None, 0)
                    .expect("drop on timeline");
            },
        );
    }
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

/// The Sequence menu has the row, and the palette offers it by name as it offers
/// every menu row: `palette_tests::every_menu_row_is_offered_by_name`.
#[test]
fn the_sequence_menu_reaches_it() {
    let (state, _) = with_clip();
    let labels = crate::chrome::labels_for(crate::types::AppMenu::Sequence, &state);
    assert!(
        labels.contains(&"Add Adjustment Layer"),
        "the Sequence menu has no adjustment layer row: {labels:?}"
    );
}
