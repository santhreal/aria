//! Drop targeting: 3 of the cases in [`super`].
//!
//! Split out of `track_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn an_unaimed_drop_lands_on_the_targeted_lane_of_its_kind() {
    let (mut state, clip) = with_clip();
    let main = rows(&state)[0];
    let media = fixture_media(&state, main, clip.raw());
    state.add_track(TrackKind::Video, "V2").expect("add V2");
    let upper = rows(&state)[0];
    assert_ne!(upper, main, "V2 sits above the main lane");

    // Untarget the top lane: the drop must fall through to the next targeted
    // video lane rather than landing on the topmost one.
    state
        .toggle_track_targeted(TrackId::from_raw(upper))
        .unwrap();
    state.edit_mode = EditMode::Overwrite;
    state
        .drop_media_on_timeline(media, None, 96)
        .expect("place with no lane aimed at");
    assert!(
        lane(&state, upper)
            .clips
            .iter()
            .all(|c| c.timeline_start_ticks != 96),
        "the untargeted lane took the drop"
    );
    assert!(
        lane(&state, main)
            .clips
            .iter()
            .any(|c| c.timeline_start_ticks == 96),
        "the targeted lane took it"
    );
}
#[test]
fn aiming_at_a_lane_outranks_targeting() {
    let (mut state, clip) = with_clip();
    let main = rows(&state)[0];
    let media = fixture_media(&state, main, clip.raw());
    state.add_track(TrackKind::Video, "V2").expect("add V2");
    let upper = rows(&state)[0];

    // The aimed lane is untargeted; a gesture that named a lane still wins,
    // because the operator was more specific than the flag.
    state
        .toggle_track_targeted(TrackId::from_raw(upper))
        .unwrap();
    state.edit_mode = EditMode::Overwrite;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(upper)), 96)
        .expect("place on the aimed lane");
    assert!(
        lane(&state, upper)
            .clips
            .iter()
            .any(|c| c.timeline_start_ticks == 96),
        "the aimed lane took the drop"
    );
}
#[test]
fn a_ripple_drop_moves_a_sync_locked_lane_and_leaves_a_pinned_one() {
    // Two video lanes rather than picture and sound: the fixture asset is a
    // still, so an audio lane could not hold it and the drop would be re-aimed.
    // What is being asserted is the ride-along, not the lane's kind.
    let (mut state, clip) = with_clip();
    let main = rows(&state)[0];
    let media = fixture_media(&state, main, clip.raw());
    state.add_track(TrackKind::Video, "V2").expect("add V2");
    let upper = rows(&state)[0];
    state.edit_mode = EditMode::Overwrite;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(upper)), 0)
        .expect("place on V2");
    let above = lane(&state, upper).clips[0].id;
    assert_eq!(clip_start(&state, upper, above), 0);

    // A ripple insert on the lower lane: V2 rides along because its sync lock
    // is set.
    state.edit_mode = EditMode::Ripple;
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(main)), 0)
        .expect("insert below");
    let rode = clip_start(&state, upper, above);
    assert!(rode > 0, "the sync-locked lane rode along");

    // Clear the lock and insert again: V2 stays where it is.
    state
        .toggle_track_sync_lock(TrackId::from_raw(upper))
        .unwrap();
    state
        .drop_media_on_timeline(media, Some(TrackId::from_raw(main)), 0)
        .expect("insert again");
    assert_eq!(
        clip_start(&state, upper, above),
        rode,
        "a pinned lane did not move"
    );
}
