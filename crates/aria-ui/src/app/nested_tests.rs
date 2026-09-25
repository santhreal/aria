//! Nested sequences driven the way an operator drives them.
//!
//! Defect class these close: three commands (`CreateNestedSequence`,
//! `PlaceNestedSequence`, `OpenSequence`) declared themselves reachable by the
//! operator and no surface sent any of them. A nest could exist in a project
//! and there was no way to open it, leave it, or place a second copy. So the
//! cases are: nesting replaces the selection with one clip that names its
//! sequence; opening a nest swaps the timeline under the editor and the
//! breadcrumb says which; leaving returns to the root; placing puts a second
//! clip on the same sequence; and each refusal states what is missing rather
//! than picking a sequence nobody asked for.

use super::AppState;
use crate::app::transform_handle_tests::selected;

/// Every clip on the timeline that is open, as `(id, nested sequence)`.
fn clips(state: &AppState) -> Vec<(u64, Option<u64>)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.id, clip.nested_sequence))
        .collect()
}

/// A session whose one clip has been nested, with the nest clip selected.
fn nested() -> AppState {
    let mut state = selected();
    state.nest_selection("Act one").expect("nest the selection");
    let nest = clips(&state)
        .into_iter()
        .find(|(_, sequence)| sequence.is_some())
        .expect("the selection is replaced by one nest clip");
    state.select_clip(aria_models::ClipId::from_raw(nest.0));
    state
}

#[test]
fn nesting_the_selection_replaces_it_with_one_clip_that_names_its_sequence() {
    let mut state = selected();
    let before = clips(&state);
    assert_eq!(before.len(), 1, "the fixture starts with one clip");
    assert_eq!(before[0].1, None, "a still is not a nest");

    state.nest_selection("Act one").expect("nest the selection");

    let after = clips(&state);
    assert_eq!(after.len(), 1, "one clip in, one nest clip out");
    assert_ne!(after[0].0, before[0].0, "the original clip is gone");
    let sequence = after[0].1.expect("the new clip plays a sequence");
    assert_eq!(
        state.store.sequences.len(),
        1,
        "one nest means one secondary sequence"
    );
    assert_eq!(state.store.sequences[0].id, sequence);
    assert_eq!(state.store.sequences[0].name, "Act one");
    assert_eq!(
        state.store.open_sequence, None,
        "nesting does not open what it built"
    );
    assert_eq!(state.open_sequence_name(), None);
}

#[test]
fn opening_a_nest_swaps_the_timeline_and_leaving_brings_it_back() {
    let mut state = nested();
    let sequence = state.store.sequences[0].id;
    let nest_clip = clips(&state)[0].0;

    state.open_selected_nest().expect("open the nest");
    assert_eq!(state.store.open_sequence, Some(sequence));
    assert_eq!(state.open_sequence_name(), Some("Act one"));
    let inside = clips(&state);
    assert_eq!(
        inside.len(),
        1,
        "the nested still is on the nest's timeline"
    );
    assert_eq!(inside[0].1, None, "the still inside is not itself a nest");
    assert_ne!(
        inside[0].0, nest_clip,
        "the clip inside is not the clip that plays it"
    );
    assert!(
        state.selection.is_empty(),
        "the selection named a clip of the timeline that was left"
    );

    state.leave_sequence().expect("leave the nest");
    assert_eq!(state.store.open_sequence, None);
    assert_eq!(state.open_sequence_name(), None);
    assert_eq!(clips(&state), vec![(nest_clip, Some(sequence))]);
}

#[test]
fn an_edit_made_inside_a_nest_stays_inside_it() {
    let mut state = nested();
    let root = clips(&state);
    state.open_selected_nest().expect("open the nest");
    let inside = clips(&state)[0].0;
    state.select_clip(aria_models::ClipId::from_raw(inside));
    state.delete_selected().expect("delete inside the nest");
    assert!(clips(&state).is_empty(), "the nest's timeline is now empty");

    state.leave_sequence().expect("leave the nest");
    assert_eq!(clips(&state), root, "the root timeline is untouched");
}

#[test]
fn placing_the_selected_nest_adds_a_second_clip_on_the_same_sequence() {
    let mut state = nested();
    let sequence = state.store.sequences[0].id;
    let end = state.store.sequence.duration_ticks;
    state.set_playhead(end);

    state
        .place_selected_nest()
        .expect("place the nest again at the playhead");

    let after = clips(&state);
    assert_eq!(after.len(), 2, "a second clip plays the same nest");
    assert!(
        after.iter().all(|(_, seq)| *seq == Some(sequence)),
        "both clips play the sequence that already existed, got {after:?}"
    );
    assert_eq!(
        state.store.sequences.len(),
        1,
        "placing a nest does not build another sequence"
    );
}

#[test]
fn every_refusal_names_what_is_missing() {
    let mut state = selected();
    state.selection.clear();
    let empty = state
        .nest_selection("nothing")
        .expect_err("nesting an empty selection is refused");
    assert!(
        format!("{empty}").contains("select the clips to nest"),
        "got {empty}"
    );

    let root = state
        .leave_sequence()
        .expect_err("there is nothing to leave on the root timeline");
    assert!(format!("{root}").contains("already open"), "got {root}");

    let mut media = selected();
    let not_a_nest = media
        .open_selected_nest()
        .expect_err("a still is not a nest");
    assert!(
        format!("{not_a_nest}").contains("nested sequence"),
        "got {not_a_nest}"
    );
    assert!(
        media.place_selected_nest().is_err(),
        "a still cannot be placed as a nest"
    );
}

#[test]
fn a_generated_name_counts_the_sequences_that_exist() {
    let mut state = selected();
    state
        .nest_selection_named_in_order()
        .expect("nest under a generated name");
    assert_eq!(state.store.sequences[0].name, "Nest 1");

    let nest = clips(&state)[0].0;
    state.select_clip(aria_models::ClipId::from_raw(nest));
    state
        .nest_selection_named_in_order()
        .expect("nest the nest itself");
    let names: Vec<&str> = state
        .store
        .sequences
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(names, vec!["Nest 1", "Nest 2"], "two nests, two names");
}
