//! The offset between linked clips that place the same file.
//!
//! Closes the class where a linked pair slips out of sync without either bar
//! showing it: a move of one member, a slip of one member's source, and a pair
//! that only shares a link and not a file. Does not cover drawing the offset;
//! that is the timeline's.

use super::*;

/// One file placed on a picture lane and a sound lane at tick 100, from
/// source tick 10, linked.
fn linked_pair() -> (Project, ClipId, ClipId, TrackId) {
    let (mut project, media, video) = project_with_media(500);
    let audio = project.add_track(TrackKind::Audio, "A1");
    let picture = project.add_clip(video, media, tr(10, 48), rt(100)).unwrap();
    let sound = project.add_clip(audio, media, tr(10, 48), rt(100)).unwrap();
    project.link_clips(&[picture, sound]).unwrap();
    (project, picture, sound, audio)
}

#[test]
fn a_pair_in_sync_lists_no_offset() {
    let (project, _, _, _) = linked_pair();
    assert!(project.link_drifts().is_empty());
}

#[test]
fn moving_one_member_shows_the_offset_on_both_with_opposite_signs() {
    let (mut project, picture, sound, audio) = linked_pair();
    project.move_clip(sound, audio, rt(103)).unwrap();
    let drifts = project.link_drifts();
    assert_eq!(drifts.get(&sound), Some(&3));
    assert_eq!(drifts.get(&picture), Some(&-3));
    assert_eq!(drifts.len(), 2);

    project.move_clip(sound, audio, rt(100)).unwrap();
    assert!(project.link_drifts().is_empty(), "moved back into sync");
}

#[test]
fn slipping_one_member_shows_the_offset() {
    let (mut project, picture, sound, _) = linked_pair();
    project.slip_clip(sound, 2).unwrap();
    let drifts = project.link_drifts();
    let offset = drifts[&sound];
    assert_eq!(offset.abs(), 2, "{drifts:?}");
    assert_eq!(drifts[&picture], -offset);
}

#[test]
fn a_retimed_member_is_not_measured() {
    let (mut project, _, sound, audio) = linked_pair();
    project.move_clip(sound, audio, rt(103)).unwrap();
    project
        .set_clip_speed(sound, Rational::new(2, 1), false)
        .unwrap();
    assert!(project.link_drifts().is_empty());
}

#[test]
fn only_members_placing_the_same_file_are_compared() {
    let (mut project, media, video) = project_with_media(500);
    let other = project.add_media(MediaSource::new(
        "/tmp/other.mp4",
        1920,
        1080,
        R24,
        500,
        true,
    ));
    let audio = project.add_track(TrackKind::Audio, "A1");
    let picture = project.add_clip(video, media, tr(10, 48), rt(100)).unwrap();
    let sound = project.add_clip(audio, other, tr(0, 48), rt(300)).unwrap();
    project.link_clips(&[picture, sound]).unwrap();
    assert!(project.link_drifts().is_empty());

    let unlinked = project.add_clip(audio, media, tr(10, 48), rt(200)).unwrap();
    assert!(
        project.link_drifts().is_empty(),
        "{unlinked:?} is not linked"
    );
}
