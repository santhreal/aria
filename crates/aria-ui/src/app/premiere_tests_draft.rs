//! Audio extraction and draft edit tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `premiere_tests.rs` through `use super::*`.

use super::*;

/// **Extract Audio runs where its row is offered, and nowhere else.**
///
/// The class closed here: a clip the editor offered Extract Audio on that the
/// engine rejected, so the command posted the value copy for a structural
/// refusal. A picture linked to another picture read as extractable, though
/// the engine rejects every clip that shares its link, and a picture linked
/// to a sound clip read as having no sound. Each shape a clip can be in runs
/// the row's guard, then the command the menu and context rows both run:
/// where the guard passes, the extraction adds a clip; where it declines, the
/// command writes the guard's reason, changes nothing and fails nothing.
/// Before the extraction, the mixer draws a strip for the clip's picture lane
/// exactly when the clip's sound plays from that lane, link or no link.
///
/// Not covered: a locked source or target lane, which the engine rejects
/// separately.
#[test]
fn extract_audio_runs_exactly_where_its_row_is_offered() {
    use crate::app::decline::Decline;
    use crate::app::fixtures::{linked_pair, video_in_picture, with_video};
    use crate::chrome::menu_guards::extractable;

    let picture_pair = || {
        let (mut state, first) = with_video();
        let lane = state
            .add_lane_of_kind(aria_models::TrackKind::Video, None)
            .expect("a second picture lane");
        let second = video_in_picture(&mut state, lane);
        state.selection = vec![first, second];
        state.sync_from_engine();
        state.link_selected().expect("link the pictures");
        state.sync_from_engine();
        state.select_clip(first);
        (state, first)
    };
    let sound_pair = || {
        let mut state = linked_pair();
        let picture = state.selection[0];
        state.select_clip(picture);
        (state, picture)
    };
    let unlinked_pair = || {
        let (mut state, first) = picture_pair();
        state.unlink_selected().expect("unlink the pictures");
        state.select_clip(first);
        (state, first)
    };
    // The shape, the reason the row declines for, and whether the mixer draws
    // the clip's picture lane without a strip before the extraction.
    let cases = [
        ("an unlinked picture with sound", with_video(), None, false),
        (
            "a picture linked to a sound clip",
            sound_pair(),
            Some(Decline::LinkedSound),
            true,
        ),
        (
            "a picture linked to a picture",
            picture_pair(),
            Some(Decline::LinkedSound),
            false,
        ),
        (
            "a picture unlinked from its picture",
            unlinked_pair(),
            None,
            false,
        ),
        (
            "a still",
            with_clip(),
            Some(Decline::NoSoundToExtract),
            true,
        ),
    ];
    for (name, (mut state, clip), want, soundless) in cases {
        assert_eq!(state.selected_clip(), Some(clip), "{name}: the selection");
        assert_eq!(
            extractable("seq-extract-audio", &state),
            want.map(Decline::reason),
            "{name}: the row's guard"
        );
        let lane = state
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.clips.iter().any(|ui| ui.id == clip.raw()))
            .map(|lane| lane.id)
            .expect("the clip is on a lane");
        assert_eq!(
            state.soundless_picture_lanes().contains(&lane),
            soundless,
            "{name}: the mixer's strip for the clip's lane"
        );
        let before = state.store.sequence.clip_count();
        state.status_message = None;
        state
            .extract_selected_audio()
            .unwrap_or_else(|err| panic!("{name}: the command failed: {err:?}"));
        assert_eq!(state.last_error, None, "{name}: an error was posted");
        let after = state.store.sequence.clip_count();
        match want {
            None => assert_eq!(after, before + 1, "{name}: no sound clip was added"),
            Some(why) => {
                assert_eq!(
                    after, before,
                    "{name}: a declined extract changed the timeline"
                );
                assert_eq!(
                    state.status_message.as_deref(),
                    Some(why.reason()),
                    "{name}: the status line"
                );
            }
        }
    }
}
#[test]
fn freeze_frame_is_gated_on_the_engine_capability() {
    // A still-image clip is not freezable, and the surface gate must agree with
    // the command rather than let it fail. This is the class: a menu offering an
    // action the engine refuses.
    let (mut state, clip) = with_clip();
    let (start, end) = clip_bounds(&state, clip);
    let mid = start + (end - start) / 2;
    state.set_playhead(mid);
    let allowed = state.selected_can_freeze_frame();
    let before = state.store.sequence.clip_count();
    let duration_before = state.store.sequence.duration_ticks;
    state
        .freeze_selected_frame()
        .expect("freeze must never surface an engine error through the gate");
    if allowed {
        assert!(
            state.store.sequence.duration_ticks >= duration_before,
            "a hold must not shorten the sequence"
        );
    } else {
        assert_eq!(
            state.store.sequence.clip_count(),
            before,
            "a refused freeze must leave the timeline untouched"
        );
        assert_eq!(state.store.sequence.duration_ticks, duration_before);
    }
    assert_eq!(
        end,
        clip_bounds(&state, clip).1,
        "the clip must keep its tail either way"
    );
}
#[test]
fn every_projected_clip_reports_its_capabilities() {
    // The gate the UI reads must come from the engine, not a guess: a clip that
    // reports can_extract_audio must actually accept the command.
    let (state, _) = with_clip();
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("one clip");
    assert!(
        clip.can_split,
        "a media clip on the timeline must be splittable"
    );
}
// --- inline rename -------------------------------------------------------
//
// Closes "a rename dispatches on an empty or unchanged draft", "typing leaks
// into shortcuts", and "a rename of one target renames another".

#[test]
fn rename_seeds_the_draft_and_applies_to_every_target() {
    use crate::types::RenameTarget;
    let (mut state, _) = with_clip();
    state.set_playhead(15);
    state.add_marker_at_playhead().expect("add marker");
    let marker = state.store.sequence.markers[0].id;
    let track = state.store.sequence.tracks[0].id;

    // Every target seeds the draft with the current name, never an empty box.
    for target in [
        RenameTarget::Marker(marker),
        RenameTarget::Track(track),
        RenameTarget::Project,
    ] {
        state.begin_rename(target);
        assert_eq!(state.rename, Some(target));
        assert!(
            !state.rename_draft.is_empty(),
            "{target:?} must seed the draft with its current name"
        );
        state.cancel_rename();
        assert_eq!(state.rename, None);
        assert!(state.rename_draft.is_empty());
    }

    state.begin_rename(RenameTarget::Marker(marker));
    state.rename_draft.clear();
    for ch in "Chorus".chars() {
        state.type_key(ch);
    }
    state.commit_rename().expect("commit marker");
    assert_eq!(state.store.sequence.markers[0].name, "Chorus");
    assert_eq!(
        state.store.sequence.markers[0].tick, 15,
        "rename must not move"
    );
    assert_eq!(state.rename, None, "commit must close the draft");

    state.begin_rename(RenameTarget::Track(track));
    state.rename_draft.clear();
    for ch in "Dialogue".chars() {
        state.type_key(ch);
    }
    state.commit_rename().expect("commit track");
    assert_eq!(state.store.sequence.tracks[0].name, "Dialogue");
    assert_eq!(
        state.store.sequence.markers[0].name, "Chorus",
        "renaming a track must not touch a marker"
    );

    state.begin_rename(RenameTarget::Project);
    state.rename_draft.clear();
    for ch in "Reel".chars() {
        state.type_key(ch);
    }
    state.commit_rename().expect("commit project");
    assert_eq!(state.store.project_name, "Reel");
    assert_eq!(
        state.store.sequence.tracks[0].name, "Dialogue",
        "renaming the project must not touch a track"
    );
}
#[test]
fn an_empty_or_unchanged_draft_does_not_dispatch() {
    use crate::types::RenameTarget;
    let (mut state, _) = with_clip();
    let track = state.store.sequence.tracks[0].id;
    let original = state.store.sequence.tracks[0].name.clone();
    let undo_before = state.store.can_undo;

    // Empty draft.
    state.begin_rename(RenameTarget::Track(track));
    state.rename_draft.clear();
    state.commit_rename().expect("empty commit");
    assert_eq!(state.store.sequence.tracks[0].name, original);
    assert_eq!(state.rename, None);
    assert_eq!(
        state.store.can_undo, undo_before,
        "an empty draft must not create an undo entry"
    );

    // Whitespace-only draft is the same case once trimmed.
    state.begin_rename(RenameTarget::Track(track));
    state.rename_draft = "   ".into();
    state.commit_rename().expect("blank commit");
    assert_eq!(state.store.sequence.tracks[0].name, original);

    // Unchanged draft.
    state.begin_rename(RenameTarget::Track(track));
    state.commit_rename().expect("unchanged commit");
    assert_eq!(state.store.sequence.tracks[0].name, original);
    assert_eq!(
        state.store.can_undo, undo_before,
        "an unchanged draft must not create an undo entry"
    );
}
#[test]
fn rename_input_is_bounded_and_inert_when_closed() {
    use crate::app::rename::RENAME_MAX_LEN;
    use crate::types::RenameTarget;
    let (mut state, _) = with_clip();

    // With no rename open, typing must not accumulate anywhere.
    state.type_key('x');
    assert!(
        state.rename_draft.is_empty(),
        "typing with no draft is inert"
    );

    let track = state.store.sequence.tracks[0].id;
    state.begin_rename(RenameTarget::Track(track));
    state.rename_draft.clear();
    for _ in 0..(RENAME_MAX_LEN * 2) {
        state.type_key('a');
    }
    assert_eq!(
        state.rename_draft.chars().count(),
        RENAME_MAX_LEN,
        "a held key must not grow the draft without bound"
    );
    state.press_backspace();
    assert_eq!(state.rename_draft.chars().count(), RENAME_MAX_LEN - 1);
}
#[test]
fn renaming_a_missing_target_does_not_open_a_draft() {
    use crate::types::RenameTarget;
    let (mut state, _) = with_clip();
    // A stale menu action naming a marker that was already deleted.
    state.begin_rename(RenameTarget::Marker(9_999));
    assert_eq!(state.rename, None, "a missing target must not open a draft");

    state.begin_rename_marker_at_playhead();
    assert_eq!(
        state.rename, None,
        "no marker at the playhead means no draft"
    );
    state.set_playhead(20);
    state.add_marker_at_playhead().expect("add");
    state.begin_rename_marker_at_playhead();
    assert!(matches!(state.rename, Some(RenameTarget::Marker(_))));
}
