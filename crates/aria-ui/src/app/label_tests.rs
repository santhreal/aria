//! Contracts for label colours.
//!
//! The class closed here is "a colour is stored but nothing can find it again":
//! a label that only tints a card is decoration, so the assertions pair setting
//! a colour with reaching the group it marks, and with surviving undo.
//!
//! Not covered: the drawn swatch row and the card tint, which need a render
//! surface.

use aria_models::{ClipId, LabelColor};

use super::AppState;
use super::premiere_tests::with_clip;

fn labels(state: &AppState) -> Vec<Option<LabelColor>> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.label)
        .collect()
}

#[test]
fn a_label_is_set_projected_and_undone() {
    let (mut state, _) = with_clip();
    assert_eq!(labels(&state), vec![None], "a fresh clip carries no label");

    state
        .set_selection_label(Some(LabelColor::Teal))
        .expect("set");
    assert_eq!(labels(&state), vec![Some(LabelColor::Teal)]);
    assert_eq!(state.selected_label(), Some(LabelColor::Teal));

    assert!(state.undo(), "one command, one undo step");
    assert_eq!(labels(&state), vec![None], "undo clears the label");
}

#[test]
fn clearing_a_label_is_a_separate_edit() {
    let (mut state, _) = with_clip();
    state
        .set_selection_label(Some(LabelColor::Magenta))
        .expect("set");
    state.set_selection_label(None).expect("clear");
    assert_eq!(labels(&state), vec![None]);
    assert!(state.undo(), "the clear is undoable on its own");
    assert_eq!(labels(&state), vec![Some(LabelColor::Magenta)]);
}

#[test]
fn selecting_by_label_reaches_every_clip_that_carries_it() {
    let (mut state, first) = with_clip();
    let end = state
        .selected_ui_clip()
        .expect("the fixture clip")
        .timeline_end_ticks;
    state.set_playhead(end / 2);
    state.split_at_playhead().expect("two clips");
    let second = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != first)
        .expect("the right half");
    state.select_clip(first);
    state
        .set_selection_label(Some(LabelColor::Blue))
        .expect("set");
    state.clear_selection();

    state.select_by_label(LabelColor::Blue);
    assert_eq!(state.selection, vec![first], "exactly the labeled clip");
    assert_eq!(
        state.label_counts(),
        vec![(LabelColor::Blue, 1), (LabelColor::Teal, 1)],
        "the unlabelled half counts under the label it is drawn in"
    );

    state.select_by_label(LabelColor::Teal);
    assert_eq!(
        state.selection,
        vec![second],
        "the group a clip is drawn in reaches it without a label of its own"
    );

    state.select_by_label(LabelColor::Brown);
    assert_eq!(
        state.selection,
        vec![second],
        "a color nothing carries leaves the selection alone"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("No clips with that label")
    );
}

#[test]
fn a_label_covers_the_whole_selection() {
    let (mut state, _) = with_clip();
    let end = state
        .selected_ui_clip()
        .expect("the fixture clip")
        .timeline_end_ticks;
    state.set_playhead(end / 2);
    state.split_at_playhead().expect("two clips");
    state.select_all_unlocked();
    assert_eq!(state.selection.len(), 2);

    state
        .set_selection_label(Some(LabelColor::Mango))
        .expect("set");
    assert_eq!(
        labels(&state),
        vec![Some(LabelColor::Mango), Some(LabelColor::Mango)]
    );
    assert_eq!(state.label_counts(), vec![(LabelColor::Mango, 2)]);
}

#[test]
fn a_label_survives_a_save_and_load_round_trip() {
    let (mut state, _) = with_clip();
    state
        .set_selection_label(Some(LabelColor::Purple))
        .expect("set");
    let path = state.data_dir.join("labeled.aria");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("clear the session");
    assert!(labels(&state).is_empty());
    state.open_project(path).expect("open");
    assert_eq!(labels(&state), vec![Some(LabelColor::Purple)]);
}

#[test]
fn a_pool_row_takes_a_label_and_gives_it_back() {
    let (mut state, _) = with_clip();
    let media = state.store.media.first().expect("the fixture media").id;
    assert_eq!(state.media_label(media), None);

    state
        .set_media_label(media, Some(LabelColor::Yellow))
        .expect("set");
    assert_eq!(state.media_label(media), Some(LabelColor::Yellow));
    assert!(state.undo(), "one command, one undo step");
    assert_eq!(state.media_label(media), None, "undo clears the label");
}

#[test]
fn the_pool_filter_matches_a_label_colour() {
    use crate::types::SidebarTab;

    let (mut state, _) = with_clip();
    let media = state.store.media.first().expect("the fixture media").id;
    state
        .set_media_label(media, Some(LabelColor::Teal))
        .expect("set");

    for ch in "teal".chars() {
        state.media_query_push(ch);
    }
    assert_eq!(
        state.pool_rows(SidebarTab::Media).len(),
        1,
        "a color name reaches the row it labels"
    );

    state.clear_media_query();
    for ch in "berry".chars() {
        state.media_query_push(ch);
    }
    assert!(
        state.pool_rows(SidebarTab::Media).is_empty(),
        "an unused color name matches nothing"
    );
}

#[test]
fn a_media_label_survives_a_save_and_load_round_trip() {
    let (mut state, _) = with_clip();
    let media = state.store.media.first().expect("the fixture media").id;
    state
        .set_media_label(media, Some(LabelColor::Caribbean))
        .expect("set");
    let path = state.data_dir.join("labeled-pool.aria");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("clear the session");
    state.open_project(path).expect("open");
    let reopened = state.store.media.first().expect("the media came back").id;
    assert_eq!(state.media_label(reopened), Some(LabelColor::Caribbean));
}
