//! Cuts through linked clips: Add Edit, the razor, and Extract.
//!
//! # Why
//!
//! A link holds a picture and its sound together. A cut through both clips of
//! a pair leaves two pairs: the heads keep the link they had and the tails
//! share a new one, so a tail moved afterwards takes its sound with it. A tail
//! whose partner was not cut stays unlinked, since the partner runs on past the
//! cut. Every cut gesture goes through [`AppState::cut_clips_at`], so these
//! tests drive the Add Edit, razor and Extract entry points over two linked
//! pairs and read the projected links afterwards: one pair cannot tell "each
//! pair's tails" from "every tail".
//!
//! Not caught here: the engine's own cuts (the sync-locked lanes of a ripple
//! delete or a rippled insert); `aria-engine/tests/split_cuts.rs` drives those.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, TrackId};

use super::AppState;
use super::premiere_tests::with_sequence;

/// The workload's lanes, in the order [`two_pairs`] returns their clips.
const LANES: [&str; 4] = ["Video0", "Audio0", "Video1", "Audio1"];

/// Two lane pairs of three 48-tick clips, end to end from 0, and the first
/// clip of each lane in [`LANES`] order. Video0 and Audio0 are one link,
/// Video1 and Audio1 another.
fn two_pairs() -> (AppState, [ClipId; 4]) {
    let mut state = with_sequence(2, 3);
    let clips = LANES.map(|name| {
        let id = lane(&state, name).raw();
        state
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.id == id)
            .and_then(|track| {
                track
                    .clips
                    .iter()
                    .find(|clip| clip.timeline_start_ticks == 0)
            })
            .map(|clip| ClipId::from_raw(clip.id))
            .expect("each lane starts with a clip")
    });
    for pair in clips.chunks(2) {
        state
            .apply_command(Command::Edit(EditCommand::LinkClips {
                clips: pair.to_vec(),
            }))
            .expect("link the pair");
    }
    (state, clips)
}

fn lane(state: &AppState, name: &str) -> TrackId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.name.as_ref() == name)
        .map(|track| TrackId::from_raw(track.id))
        .expect("the workload names its lanes")
}

fn link(state: &AppState, clip: ClipId) -> Option<u64> {
    state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline")
        .linked_to
}

fn span(state: &AppState, clip: ClipId) -> (i64, i64) {
    let ui = state
        .ui_clip_ref(clip)
        .expect("the clip is on the timeline");
    (ui.timeline_start_ticks, ui.timeline_end_ticks)
}

/// The lane name and link of every clip that starts at `tick`, lanes in
/// [`LANES`] order.
fn starting_at(state: &AppState, tick: i64) -> Vec<(&'static str, Option<u64>)> {
    let mut found = Vec::new();
    for name in LANES {
        let id = lane(state, name).raw();
        for track in state.store.sequence.tracks.iter().filter(|t| t.id == id) {
            for clip in track.clips.iter() {
                if clip.timeline_start_ticks == tick {
                    found.push((name, clip.linked_to));
                }
            }
        }
    }
    found
}

/// The tails' links, after checking that the four tails start at `tick`, that
/// each pair's tails share a link, and that the two pairs do not.
fn paired_tails(state: &AppState, tick: i64) -> [u64; 2] {
    let tails = starting_at(state, tick);
    let names: Vec<&str> = tails.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, LANES, "{tails:?}");
    let first = tails[0].1.expect("the first pair's picture tail is linked");
    let second = tails[2]
        .1
        .expect("the second pair's picture tail is linked");
    assert_eq!(tails[1].1, Some(first), "the first pair's tails: {tails:?}");
    assert_eq!(
        tails[3].1,
        Some(second),
        "the second pair's tails: {tails:?}"
    );
    assert_ne!(first, second, "each pair's tails form their own link");
    [first, second]
}

#[test]
fn add_edit_through_two_linked_pairs_links_each_pairs_tails_and_one_undo_restores_the_pairs() {
    let (mut state, [v0, a0, v1, a1]) = two_pairs();
    let heads = [link(&state, v0), link(&state, v1)];
    assert!(heads[0].is_some() && heads[1].is_some() && heads[0] != heads[1]);

    state.set_playhead(24);
    state.add_edit(true).expect("Add Edit on every lane");

    let tails = paired_tails(&state, 24);
    for (clip, head) in [
        (v0, heads[0]),
        (a0, heads[0]),
        (v1, heads[1]),
        (a1, heads[1]),
    ] {
        assert_eq!(link(&state, clip), head, "a head keeps its link");
        assert_eq!(span(&state, clip), (0, 24));
    }
    assert!(
        !heads.contains(&Some(tails[0])) && !heads.contains(&Some(tails[1])),
        "a tail link is new: {tails:?} against {heads:?}"
    );

    assert!(state.undo(), "the cut is one undo step");
    assert!(
        starting_at(&state, 24).is_empty(),
        "undo takes the tails back"
    );
    for (clip, head) in [
        (v0, heads[0]),
        (a0, heads[0]),
        (v1, heads[1]),
        (a1, heads[1]),
    ] {
        assert_eq!(link(&state, clip), head, "undo leaves the original link");
        assert_eq!(span(&state, clip), (0, 48));
    }
}

#[test]
fn a_razor_on_one_lane_of_a_pair_leaves_the_cut_tail_unlinked() {
    let (mut state, [v0, a0, ..]) = two_pairs();
    let pair = link(&state, v0);

    state
        .split_at_tick(lane(&state, "Video0"), 24, false)
        .expect("razor on Video0");

    assert_eq!(
        starting_at(&state, 24),
        [("Video0", None)],
        "only the picture was cut, and its sound runs on past the cut"
    );
    assert_eq!(link(&state, v0), pair, "the head keeps the pair's link");
    assert_eq!(link(&state, a0), pair);
    assert_eq!(span(&state, a0), (0, 48), "the sound is not cut");
}

#[test]
fn a_razor_on_every_lane_pairs_the_tails_as_add_edit_does() {
    let (mut state, _) = two_pairs();

    state
        .split_at_tick(lane(&state, "Video0"), 24, true)
        .expect("razor on every lane");

    paired_tails(&state, 24);
}

#[test]
fn extract_across_two_linked_pairs_keeps_each_pairs_surviving_tails_linked() {
    let (mut state, [v0, a0, v1, a1]) = two_pairs();
    let heads = [link(&state, v0), link(&state, v1)];
    state.set_playhead(12);
    state.set_mark_in();
    state.set_playhead(36);
    state.set_mark_out();

    state.remove_marked_range(true).expect("Extract");

    assert_eq!(
        state.status_message.as_deref(),
        Some("Extracted 4 clips from In to Out")
    );
    // The tails were cut at Out and slid back onto In.
    paired_tails(&state, 12);
    for (clip, head) in [
        (v0, heads[0]),
        (a0, heads[0]),
        (v1, heads[1]),
        (a1, heads[1]),
    ] {
        assert_eq!(link(&state, clip), head, "a head keeps its link");
        assert_eq!(span(&state, clip), (0, 12));
    }

    assert_eq!(state.next_undo_label().as_deref(), Some("Extract"));
    assert!(state.undo(), "Extract is one undo step");
    for (clip, head) in [
        (v0, heads[0]),
        (a0, heads[0]),
        (v1, heads[1]),
        (a1, heads[1]),
    ] {
        assert_eq!(link(&state, clip), head, "undo leaves the original link");
        assert_eq!(span(&state, clip), (0, 48));
    }
}
