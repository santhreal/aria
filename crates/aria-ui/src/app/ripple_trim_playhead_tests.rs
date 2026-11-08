//! Q / W: Ripple Trim Previous Edit to Playhead and Next Edit to Playhead.
//!
//! # Why
//!
//! Q removes the span from the previous cut on the targeted lanes to the
//! playhead, and W the span from the playhead to the next cut, from every
//! targeted lane at once, closing the gap. Trimming one clip under the
//! playhead instead (the top lane's, with every lane targeted) slid that lane
//! alone and left the lanes under it out of step. These tests cut the two
//! lanes of a pair at different ticks, so the previous cut differs by lane, and
//! read every lane after Q and W with both lanes targeted and with one.
//!
//! An untargeted lane with its sync lock set rides Extract, Q and W: it loses
//! the same stretch, as a ripple delete's riding lanes do. Closing the span
//! on the targeted lanes alone left it out of step with them. Lift closes
//! nothing, so it leaves that lane alone, and a locked lane stays.
//!
//! Not caught here: Lift and Extract share the removal and `linked_cut_tests`
//! drives them through linked clips.

use aria_models::TrackId;

use super::AppState;
use super::premiere_tests::with_sequence;

/// One lane pair of three 48-tick clips, Video0 cut at 34 and Audio0 at 30:
/// cuts at 0, 34, 48, 96 and 144 on Video0 and 0, 30, 48, 96 and 144 on
/// Audio0. The playhead at 40.
fn staggered() -> AppState {
    let mut state = with_sequence(1, 3);
    for (name, tick) in [("Video0", 34), ("Audio0", 30)] {
        state
            .split_at_tick(lane(&state, name), tick, false)
            .expect("stagger the cuts");
    }
    state.set_playhead(40);
    state
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

/// Every clip span on `name`, in timeline order.
fn spans(state: &AppState, name: &str) -> Vec<(i64, i64)> {
    let id = lane(state, name).raw();
    let mut spans: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == id)
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
}

const VIDEO: [(i64, i64); 4] = [(0, 34), (34, 48), (48, 96), (96, 144)];
const AUDIO: [(i64, i64); 4] = [(0, 30), (30, 48), (48, 96), (96, 144)];

#[test]
fn q_removes_from_the_nearest_cut_on_any_targeted_lane_from_every_targeted_lane() {
    let mut state = staggered();

    state.ripple_trim_to_playhead(true).expect("Q");

    // Video0's cut at 34 is the nearest before the playhead: [34, 40) goes
    // from both lanes and everything after it moves 6 earlier.
    assert_eq!(
        spans(&state, "Video0"),
        [(0, 34), (34, 42), (42, 90), (90, 138)]
    );
    assert_eq!(
        spans(&state, "Audio0"),
        [(0, 30), (30, 34), (34, 42), (42, 90), (90, 138)]
    );
    assert_eq!(
        state.playhead.value, 34,
        "the playhead lands on the span's start"
    );

    assert_eq!(state.next_undo_label().as_deref(), Some("Ripple trim"));
    assert!(state.undo(), "Q is one undo step");
    assert_eq!(spans(&state, "Video0"), VIDEO);
    assert_eq!(spans(&state, "Audio0"), AUDIO);
}

#[test]
fn q_with_a_lane_untargeted_reaches_the_targeted_lanes_cut_and_leaves_that_lane() {
    let mut state = staggered();
    // Out of sync lock too, so no ripple rule moves it with the others.
    let video = lane(&state, "Video0");
    state.toggle_track_targeted(video).expect("untarget Video0");
    state.toggle_track_sync_lock(video).expect("unlock Video0");

    state.ripple_trim_to_playhead(true).expect("Q");

    assert_eq!(spans(&state, "Video0"), VIDEO, "an untargeted lane is left");
    assert_eq!(
        spans(&state, "Audio0"),
        [(0, 30), (30, 38), (38, 86), (86, 134)],
        "Audio0's own cut at 30 starts the span"
    );
    assert_eq!(state.playhead.value, 30);
}

#[test]
fn w_removes_to_the_next_cut_on_any_targeted_lane_and_leaves_the_playhead() {
    let mut state = staggered();

    state.ripple_trim_to_playhead(false).expect("W");

    assert_eq!(
        spans(&state, "Video0"),
        [(0, 34), (34, 40), (40, 88), (88, 136)]
    );
    assert_eq!(
        spans(&state, "Audio0"),
        [(0, 30), (30, 40), (40, 88), (88, 136)]
    );
    assert_eq!(state.playhead.value, 40);

    assert!(state.undo(), "W is one undo step");
    assert_eq!(spans(&state, "Video0"), VIDEO);
    assert_eq!(spans(&state, "Audio0"), AUDIO);
}

#[test]
fn q_at_the_start_and_w_at_the_end_change_nothing_and_say_why() {
    let mut state = staggered();
    for (at, head, why) in [
        (0, true, "No earlier edit on a targeted track"),
        (144, false, "No later edit on a targeted track"),
    ] {
        state.set_playhead(at);
        state.ripple_trim_to_playhead(head).expect("inert trim");
        assert_eq!(state.status_message.as_deref(), Some(why));
        assert_eq!(spans(&state, "Video0"), VIDEO);
        assert_eq!(spans(&state, "Audio0"), AUDIO);
    }
}

/// The removal each verb makes with Video0 alone targeted and the playhead at
/// 40: Q, Extract over In 34 to Out 39, and Lift over the same marks take
/// Video0's [34, 40), and W takes [40, 48).
fn remove(state: &mut AppState, verb: &str) {
    match verb {
        "Q" => state.ripple_trim_to_playhead(true).expect("Q"),
        "W" => state.ripple_trim_to_playhead(false).expect("W"),
        _ => {
            state.set_playhead(34);
            state.set_mark_in();
            state.set_playhead(39);
            state.set_mark_out();
            state
                .remove_marked_range(verb == "Extract")
                .expect("remove the marked range");
        }
    }
}

#[test]
fn an_untargeted_sync_locked_lane_loses_the_span_extract_q_and_w_close() {
    // (verb, Video0 after, Audio0 after with its sync lock set).
    let cases: [(&str, &[(i64, i64)], &[(i64, i64)]); 4] = [
        (
            "Q",
            &[(0, 34), (34, 42), (42, 90), (90, 138)],
            &[(0, 30), (30, 34), (34, 42), (42, 90), (90, 138)],
        ),
        (
            "Extract",
            &[(0, 34), (34, 42), (42, 90), (90, 138)],
            &[(0, 30), (30, 34), (34, 42), (42, 90), (90, 138)],
        ),
        (
            "W",
            &[(0, 34), (34, 40), (40, 88), (88, 136)],
            &[(0, 30), (30, 40), (40, 88), (88, 136)],
        ),
        ("Lift", &[(0, 34), (40, 48), (48, 96), (96, 144)], &AUDIO),
    ];
    for (verb, video_after, audio_after) in cases {
        for locked in [false, true] {
            let mut state = staggered();
            let audio = lane(&state, "Audio0");
            state.toggle_track_targeted(audio).expect("untarget Audio0");
            if locked {
                state.set_track_locked(audio, true).expect("lock Audio0");
            }
            let case = format!("{verb} with Audio0 locked {locked}");

            remove(&mut state, verb);

            assert_eq!(spans(&state, "Video0"), video_after, "{case}: Video0");
            let audio_after = if locked { &AUDIO[..] } else { audio_after };
            assert_eq!(spans(&state, "Audio0"), audio_after, "{case}: Audio0");
            assert!(state.undo(), "{case}: one undo step");
            assert_eq!(spans(&state, "Video0"), VIDEO, "{case}: undo Video0");
            assert_eq!(spans(&state, "Audio0"), AUDIO, "{case}: undo Audio0");
        }
    }
}
