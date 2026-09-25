//! Contracts for the clip card name.
//!
//! The class closed here is "the card name is derived from the source, so the
//! operator cannot say what a clip is". A name has to outrank the derived one,
//! survive a save, undo like any other edit, and be clearable back to the
//! derived name — that last one is the case a plain `String` field would get
//! wrong.
//!
//! Not covered: the double-click that opens the draft, which needs a render
//! surface. The draft itself is exercised through `begin_rename` /
//! `rename_push` / `commit_rename`, which is the state that gesture drives.

use aria_models::ClipId;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::RenameTarget;

fn card_name(state: &AppState, clip: ClipId) -> String {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|c| c.id == clip.raw())
        .expect("the clip is projected")
        .name
        .to_string()
}

/// Open the draft, clear the seeded value the way a backspacing operator would,
/// then type `text` and commit.
fn rename(state: &mut AppState, clip: ClipId, text: &str) {
    state.begin_rename(RenameTarget::Clip(clip.raw()));
    while !state.rename_draft.is_empty() {
        state.rename_pop();
    }
    for ch in text.chars() {
        state.rename_push(ch);
    }
    state.commit_rename().expect("the rename lands");
}

#[test]
fn a_renamed_clip_shows_the_operators_name() {
    let (mut state, clip) = with_clip();
    let derived = card_name(&state, clip);
    rename(&mut state, clip, "Interview take 3");
    assert_eq!(card_name(&state, clip), "Interview take 3");
    assert!(state.undo(), "a rename is an edit");
    assert_eq!(
        card_name(&state, clip),
        derived,
        "undo gives the source's name back"
    );
    assert!(state.redo());
    assert_eq!(card_name(&state, clip), "Interview take 3");
}

#[test]
fn an_empty_draft_clears_the_name_rather_than_cancelling() {
    let (mut state, clip) = with_clip();
    let derived = card_name(&state, clip);
    rename(&mut state, clip, "B-roll");
    assert_eq!(card_name(&state, clip), "B-roll");
    // The draft is empty and the target accepts one, so this is "no name", not
    // "changed my mind" — the only way back to the source's own name.
    state.begin_rename(RenameTarget::Clip(clip.raw()));
    while !state.rename_draft.is_empty() {
        state.rename_pop();
    }
    state.commit_rename().expect("clearing lands");
    assert_eq!(card_name(&state, clip), derived);
}

#[test]
fn a_blank_name_is_not_a_name() {
    let (mut state, clip) = with_clip();
    let derived = card_name(&state, clip);
    rename(&mut state, clip, "   ");
    assert_eq!(
        card_name(&state, clip),
        derived,
        "whitespace is not a name the card should show"
    );
}

#[test]
fn renaming_to_the_same_name_costs_no_undo_entry() {
    let (mut state, clip) = with_clip();
    rename(&mut state, clip, "Take 1");
    let depth = state.history_depth();
    rename(&mut state, clip, "Take 1");
    assert_eq!(
        state.history_depth(),
        depth,
        "an unchanged draft cancels rather than dispatching"
    );
}

#[test]
fn the_name_survives_save_and_open() {
    let dir = crate::scratch_pool::scratch("clip-name");
    let path = dir.join("named.aria");
    let (mut state, clip) = with_clip();
    rename(&mut state, clip, "Hero shot");
    state.project_path = Some(path.clone());
    state.save_project().expect("save");
    state.new_project().expect("new");
    state.open_project(path).expect("open");
    let named = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.name.to_string())
        .collect::<Vec<_>>();
    assert!(
        named.iter().any(|name| name == "Hero shot"),
        "the name is part of the project: {named:?}"
    );
}

#[test]
fn a_split_carries_the_name_to_both_halves() {
    let (mut state, clip) = with_clip();
    rename(&mut state, clip, "Take 4");
    // Land the playhead inside the clip and cut it: both halves are the same
    // source, so both keep the name the operator gave it.
    state.set_playhead(60);
    state.select_clip(clip);
    state.split_at_playhead().expect("split");
    let names: Vec<String> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.name.to_string())
        .collect();
    assert_eq!(
        names.iter().filter(|name| *name == "Take 4").count(),
        2,
        "both halves keep the name: {names:?}"
    );
}
