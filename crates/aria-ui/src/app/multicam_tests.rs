//! UI reach tests for multicam camera groups, angle switching, and where each
//! camera sits on the group's clock.
//!
//! Defect class: "multicam appears nowhere in the workspace. There is no
//! multi-angle clip, no angle switching, no synchronised camera group." And,
//! after those shipped, the half of it nothing wrote: an angle's sync offset was
//! honoured by the resolver and serialised with the project while no control on
//! any surface could move one, so a group of cameras that did not start rolling
//! together cut to the wrong moment and the only fix was to rebuild the group.
//!
//! Closed by proving:
//! - the sidebar route creates a multicam source from selected pool items;
//! - the active angle can be switched at the playhead tick;
//! - an angle's offset moves by frames, and by measurement from the cameras'
//!   own sound, and lands in the project the resolver reads;
//! - a group that cannot be measured says why instead of moving a camera;
//! - undo restores the prior state for creation, switching, and syncing.

use std::path::PathBuf;

use aria_models::MulticamAngle;
use aria_plugin::PluginContext;

use super::AppState;

/// Two distinct pool entries. Import is path-deduplicated — the same file twice
/// is one pool item — so a multi-angle fixture needs two files.
fn fixtures() -> [PathBuf; 2] {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    [
        fixtures.join("texture.png"),
        fixtures.join("fixture-640x360-24fps-3s.mp4"),
    ]
}

/// Two pool items, and one of them on the timeline.
///
/// The clip is the point: `set_playhead` clamps to the sequence duration, so
/// on an empty sequence every seek lands on tick 0 and a cut made "at 48"
/// is really a cut at 0. The fixture without it proved nothing about when a
/// cut holds.
fn with_two_media() -> AppState {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("multicam"),
    )
    .expect("state");
    state.new_project().expect("project");
    for path in fixtures() {
        state.import_media(path).expect("import");
    }
    let first = aria_models::MediaId::from_raw(state.store.media[0].id);
    state
        .drop_media_on_timeline(first, None, 0)
        .expect("a clip so the sequence has a duration to scrub");
    state
}

/// The angle live under the playhead, which is what the sidebar draws. The
/// cut list is projected and resolved per read, so this follows a scrub.
fn live(state: &AppState) -> usize {
    state.multicam_groups()[0].active_angle_at(state.store.playhead_ticks)
}

#[test]
fn sidebar_route_creates_multicam_group_and_switches_angles() {
    let mut state = with_two_media();
    assert_eq!(state.store.media.len(), 2);
    assert!(
        state.store.sequence.duration_ticks > 48,
        "the seeks below are clamped to the duration, got {}",
        state.store.sequence.duration_ticks
    );
    let m1 = aria_models::MediaId::from_raw(state.store.media[0].id);
    let m2 = aria_models::MediaId::from_raw(state.store.media[1].id);

    // 1. Create a multicam group with 2 angles
    let angles = vec![
        MulticamAngle::new(m1, 0).with_name("Cam 1"),
        MulticamAngle::new(m2, 24).with_name("Cam 2"),
    ];

    let group_id = state
        .create_multicam_group("Concert Multicam", angles)
        .expect("create multicam group")
        .expect("group id returned");

    let groups = state.multicam_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, group_id.raw());
    assert_eq!(groups[0].name, "Concert Multicam");
    assert_eq!(groups[0].angles.len(), 2);

    // 2. Before any cut the first angle is live. The projection resolves it at
    // the playhead, which is the read the sidebar draws from.
    state.set_playhead(0);
    assert_eq!(live(&state), 0);

    // 3. Cut to angle 1 at tick 48.
    state.set_playhead(48);
    state
        .switch_multicam_angle(group_id, 1)
        .expect("switch active angle at playhead");
    assert_eq!(live(&state), 1);

    // A cut holds from its own tick onward and not before it.
    state.set_playhead(20);
    assert_eq!(live(&state), 0, "a cut at 48 is not live at tick 20");
    state.set_playhead(48);
    assert_eq!(live(&state), 1);

    // 4. Undo restores angle 0 under the same playhead.
    assert!(state.undo(), "undo switch angle");
    assert_eq!(live(&state), 0);

    // 5. Redo puts the cut back.
    assert!(state.redo(), "redo switch angle");
    assert_eq!(live(&state), 1);

    // 6. Undo the cut, then the group itself.
    assert!(state.undo(), "undo switch angle");
    assert!(state.undo(), "undo create multicam group");
    assert!(state.multicam_groups().is_empty());
}

/// A group of two angles, both at zero, with sound the sync row can measure.
///
/// The second envelope is the first shifted by 40 peaks, which at 40 peaks per
/// second and 24 frames per second is 24 frames: a camera that started a second
/// late, the case the row exists for.
///
/// The envelope is pseudo-random rather than a repeating tick, because a
/// repeating sound has no single answer and the row refuses it by name. A
/// fixture that repeated was the first thing this test caught.
fn with_sound() -> (AppState, aria_models::MulticamGroupId) {
    let mut state = with_two_media();
    let m1 = aria_models::MediaId::from_raw(state.store.media[0].id);
    let m2 = aria_models::MediaId::from_raw(state.store.media[1].id);
    let group = state
        .create_multicam_group(
            "Concert",
            vec![
                MulticamAngle::new(m1, 0).with_name("Cam 1"),
                MulticamAngle::new(m2, 0).with_name("Cam 2"),
            ],
        )
        .expect("create multicam group")
        .expect("group id returned");
    let peaks: Vec<f32> = {
        let mut seed = 0x5eedu32;
        (0..1_200)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 16) as f32 / 65_535.0
            })
            .collect()
    };
    let shifted = peaks[40..].to_vec();
    for (id, peaks) in [m1.raw(), m2.raw()].into_iter().zip([peaks, shifted]) {
        state.waveforms.insert(
            id,
            crate::waveforms::Waveform {
                peaks: std::sync::Arc::new(peaks),
                per_second: 40.0,
            },
        );
    }
    (state, group)
}

/// Every angle's offset as the projection carries it.
fn offsets(state: &AppState) -> Vec<i64> {
    state.multicam_groups()[0]
        .angles
        .iter()
        .map(|angle| angle.sync_offset)
        .collect()
}

#[test]
fn an_angle_offset_moves_by_frames_and_undoes() {
    let (mut state, group) = with_sound();
    assert_eq!(offsets(&state), vec![0, 0]);
    state
        .nudge_multicam_angle_offset(group, 1, 6)
        .expect("a nudge is an edit");
    assert_eq!(offsets(&state), vec![0, 6]);
    // A second nudge reads the offset the first one left, so nudging is
    // cumulative rather than absolute.
    state
        .nudge_multicam_angle_offset(group, 1, -2)
        .expect("a nudge is an edit");
    assert_eq!(offsets(&state), vec![0, 4]);
    assert!(state.undo(), "undo the second nudge");
    assert_eq!(offsets(&state), vec![0, 6]);
    assert!(state.undo(), "undo the first nudge");
    assert_eq!(offsets(&state), vec![0, 0]);
}

#[test]
fn a_nudge_on_an_angle_that_is_not_there_does_nothing() {
    let (mut state, group) = with_sound();
    state
        .nudge_multicam_angle_offset(group, 9, 1)
        .expect("an angle the group does not hold is not an error");
    assert_eq!(offsets(&state), vec![0, 0]);
}

#[test]
fn syncing_by_sound_places_the_late_camera_and_undoes_in_one_step() {
    let (mut state, group) = with_sound();
    state
        .sync_multicam_group_by_audio(group)
        .expect("two cameras with sound sync");
    // 40 peaks at 40 per second is a second, and a second at 24 fps is 24
    // frames. The first camera is the clock and does not move.
    assert_eq!(offsets(&state), vec![0, 24]);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Synced 1 angle by sound")
    );
    assert!(
        state.undo(),
        "a sync is one history entry however many cameras moved"
    );
    assert_eq!(offsets(&state), vec![0, 0]);
}

#[test]
fn syncing_places_every_camera_against_the_first() {
    let (mut state, group) = with_sound();
    // The clock camera is already offset, and syncing keeps it there: the group
    // is placed against its first camera, not dragged back to zero.
    state
        .nudge_multicam_angle_offset(group, 0, 100)
        .expect("a nudge is an edit");
    state
        .sync_multicam_group_by_audio(group)
        .expect("two cameras with sound sync");
    assert_eq!(offsets(&state), vec![100, 124]);
}

#[test]
fn a_group_with_no_sound_read_yet_says_so() {
    let (mut state, group) = with_sound();
    state.waveforms.clear();
    state
        .sync_multicam_group_by_audio(group)
        .expect("declining is not a failure");
    assert_eq!(offsets(&state), vec![0, 0], "no camera moved");
    assert_eq!(
        state.status_message.as_deref(),
        Some("The angles' sound is still being read")
    );
}

/// **The camera list is one list per projection, not one per read.**
///
/// The strip cannot hold a borrow of the store across the frame, so it used to
/// copy every group and every angle in it on every frame the pool sidebar was
/// open. It is a handle now: two reads with nothing in between are the same
/// list, and an edit replaces it.
#[test]
fn the_camera_list_is_one_list_per_projection() {
    let mut state = with_two_media();
    let empty = state.multicam_groups();
    assert!(empty.is_empty(), "the fixture starts with no group");
    assert!(
        std::sync::Arc::ptr_eq(&empty, &state.multicam_groups()),
        "a second read copied the list"
    );
    let m1 = aria_models::MediaId::from_raw(state.store.media[0].id);
    let m2 = aria_models::MediaId::from_raw(state.store.media[1].id);
    state
        .create_multicam_group(
            "Two cameras",
            vec![MulticamAngle::new(m1, 0), MulticamAngle::new(m2, 0)],
        )
        .expect("a group is built")
        .expect("a group id comes back");
    let built = state.multicam_groups();
    assert!(
        !std::sync::Arc::ptr_eq(&empty, &built),
        "building a group left the strip drawing the list from before it"
    );
    assert_eq!(built.len(), 1);
    assert!(
        std::sync::Arc::ptr_eq(&built, &state.multicam_groups()),
        "a second read copied the list"
    );
}
