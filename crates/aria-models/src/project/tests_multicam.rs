//! Tests for multicam camera groups, angle switching, cut lists, and persistence.
//!
//! Defect class: "multicam appears nowhere in the workspace. There is no
//! multi-angle clip, no angle switching, no synchronised camera group."
//!
//! Closed by proving:
//! - a group is created from several media with sync offsets;
//! - the active angle at a tick is the last switch at or before it;
//! - a switch at an existing tick replaces rather than duplicates;
//! - an invalid group is refused with the named ModelError rule;
//! - the cut list round-trips through save_to_string / load_from_string;
//! - an older project file without multicam loads unchanged.

use super::*;
use crate::error::ModelError;
use crate::ids::{MediaId, MulticamGroupId};
use crate::multicam::{MulticamAngle, MulticamCut, MulticamGroup};

#[test]
fn multicam_group_created_from_several_media_with_sync_offsets() {
    let mut project = Project::new("multicam_test", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let m3 = project.add_media(sample_media(R24, 240));

    let angles = vec![
        MulticamAngle::new(m1, 0).with_name("Angle 1 (Wide)"),
        MulticamAngle::new(m2, 12).with_name("Angle 2 (Close-up)"),
        MulticamAngle::new(m3, -6).with_name("Angle 3 (Overhead)"),
    ];

    let group_id = project
        .create_multicam_group("Concert Multicam", angles)
        .expect("valid multicam group creation");

    assert_eq!(project.multicam_group_count(), 1);
    let group = project.multicam_group(group_id).expect("group exists");
    assert_eq!(group.name, "Concert Multicam");
    assert_eq!(group.angles.len(), 3);
    assert_eq!(group.angles[0].media, m1);
    assert_eq!(group.angles[0].sync_offset, 0);
    assert_eq!(group.angles[0].name.as_deref(), Some("Angle 1 (Wide)"));
    assert_eq!(group.angles[1].media, m2);
    assert_eq!(group.angles[1].sync_offset, 12);
    assert_eq!(group.angles[2].media, m3);
    assert_eq!(group.angles[2].sync_offset, -6);
    assert!(group.cuts.is_empty());
}

#[test]
fn multicam_active_angle_is_last_switch_at_or_before_tick() {
    let mut project = Project::new("multicam_active_angle", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let m3 = project.add_media(sample_media(R24, 240));

    let angles = vec![
        MulticamAngle::new(m1, 0),
        MulticamAngle::new(m2, 0),
        MulticamAngle::new(m3, 0),
    ];

    let group_id = project
        .create_multicam_group("Stage", angles)
        .expect("create group");

    // Before any switch: active angle defaults to angle 0 (m1)
    assert_eq!(project.active_multicam_angle_at(group_id, 0).unwrap(), 0);
    assert_eq!(project.active_multicam_angle_at(group_id, 50).unwrap(), 0);
    assert_eq!(project.active_multicam_media_at(group_id, 50).unwrap(), m1);

    // Switch to angle 1 at tick 24, switch to angle 2 at tick 72, switch to angle 0 at tick 120
    project.switch_multicam_angle(group_id, 24, 1).unwrap();
    project.switch_multicam_angle(group_id, 72, 2).unwrap();
    project.switch_multicam_angle(group_id, 120, 0).unwrap();

    // Ticks before first cut (0..23) -> angle 0
    assert_eq!(project.active_multicam_angle_at(group_id, 0).unwrap(), 0);
    assert_eq!(project.active_multicam_angle_at(group_id, 23).unwrap(), 0);
    assert_eq!(project.active_multicam_media_at(group_id, 23).unwrap(), m1);

    // Ticks at and after tick 24 up to tick 71 -> angle 1 (m2)
    assert_eq!(project.active_multicam_angle_at(group_id, 24).unwrap(), 1);
    assert_eq!(project.active_multicam_angle_at(group_id, 50).unwrap(), 1);
    assert_eq!(project.active_multicam_angle_at(group_id, 71).unwrap(), 1);
    assert_eq!(project.active_multicam_media_at(group_id, 50).unwrap(), m2);

    // Ticks at and after tick 72 up to tick 119 -> angle 2 (m3)
    assert_eq!(project.active_multicam_angle_at(group_id, 72).unwrap(), 2);
    assert_eq!(project.active_multicam_angle_at(group_id, 100).unwrap(), 2);
    assert_eq!(project.active_multicam_angle_at(group_id, 119).unwrap(), 2);
    assert_eq!(project.active_multicam_media_at(group_id, 100).unwrap(), m3);

    // Ticks at and after tick 120 -> angle 0 (m1)
    assert_eq!(project.active_multicam_angle_at(group_id, 120).unwrap(), 0);
    assert_eq!(project.active_multicam_angle_at(group_id, 200).unwrap(), 0);
    assert_eq!(project.active_multicam_media_at(group_id, 200).unwrap(), m1);
}

#[test]
fn multicam_switch_at_existing_tick_replaces_rather_than_duplicates() {
    let mut project = Project::new("multicam_replace_switch", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let m3 = project.add_media(sample_media(R24, 240));

    let angles = vec![
        MulticamAngle::new(m1, 0),
        MulticamAngle::new(m2, 0),
        MulticamAngle::new(m3, 0),
    ];

    let group_id = project
        .create_multicam_group("Studio", angles)
        .expect("create group");

    // Insert cuts at 30 and 60
    project.switch_multicam_angle(group_id, 30, 1).unwrap();
    project.switch_multicam_angle(group_id, 60, 2).unwrap();

    let group = project.multicam_group(group_id).unwrap();
    assert_eq!(group.cuts.len(), 2);
    assert_eq!(group.cuts[0], MulticamCut::new(30, 1));
    assert_eq!(group.cuts[1], MulticamCut::new(60, 2));

    // Re-switching at tick 30 with angle 2 replaces angle 1 without adding a new cut
    project.switch_multicam_angle(group_id, 30, 2).unwrap();

    let group = project.multicam_group(group_id).unwrap();
    assert_eq!(group.cuts.len(), 2, "must not duplicate cut at tick 30");
    assert_eq!(group.cuts[0], MulticamCut::new(30, 2));
    assert_eq!(group.cuts[1], MulticamCut::new(60, 2));
    assert_eq!(project.active_multicam_angle_at(group_id, 30).unwrap(), 2);
}

#[test]
fn multicam_invalid_group_is_refused_with_named_rules() {
    let mut project = Project::new("multicam_errors", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let unknown_media = MediaId::from_raw(9999);

    // Rule 1: Empty angles
    let err = project
        .create_multicam_group("Empty", Vec::new())
        .unwrap_err();
    assert_eq!(err, ModelError::EmptyMulticamGroup);

    // Rule 2: Unknown media
    let err = project
        .create_multicam_group(
            "Unknown",
            vec![
                MulticamAngle::new(m1, 0),
                MulticamAngle::new(unknown_media, 0),
            ],
        )
        .unwrap_err();
    assert_eq!(err, ModelError::UnknownMedia(unknown_media));

    // Rule 3: Duplicate media in angles
    let err = project
        .create_multicam_group(
            "Duplicate",
            vec![MulticamAngle::new(m1, 0), MulticamAngle::new(m1, 10)],
        )
        .unwrap_err();
    assert_eq!(err, ModelError::DuplicateMulticamMedia(m1));

    // Rule 4: Angle index out of bounds on switch
    let group_id = project
        .create_multicam_group(
            "Valid",
            vec![MulticamAngle::new(m1, 0), MulticamAngle::new(m2, 0)],
        )
        .unwrap();
    let err = project.switch_multicam_angle(group_id, 10, 5).unwrap_err();
    assert_eq!(
        err,
        ModelError::MulticamAngleOutOfBounds { index: 5, total: 2 }
    );

    // Rule 5: Unknown multicam group ID
    let unknown_group = MulticamGroupId::from_raw(8888);
    let err = project
        .switch_multicam_angle(unknown_group, 10, 0)
        .unwrap_err();
    assert_eq!(err, ModelError::UnknownMulticamGroup(unknown_group));
}

#[test]
fn multicam_cut_list_and_groups_survive_save_and_reload() {
    let mut project = Project::new("multicam_persistence", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let m3 = project.add_media(sample_media(R24, 240));

    let angles = vec![
        MulticamAngle::new(m1, 0).with_name("Cam A"),
        MulticamAngle::new(m2, 15).with_name("Cam B"),
        MulticamAngle::new(m3, -10).with_name("Cam C"),
    ];

    let group_id = project
        .create_multicam_group("Multi Cam Project", angles)
        .expect("create group");

    project.switch_multicam_angle(group_id, 24, 1).unwrap();
    project.switch_multicam_angle(group_id, 48, 2).unwrap();
    project.switch_multicam_angle(group_id, 96, 0).unwrap();

    let track = project.add_track(TrackKind::Video, "V1");
    let clip_id = project
        .add_multicam_clip(track, group_id, tr(0, 120), rt(0))
        .expect("add multicam clip");

    assert_eq!(
        project.clip(clip_id).unwrap().multicam_group(),
        Some(group_id)
    );

    // Save to string and reload
    let json = project.save_to_string().expect("save_to_string");
    assert!(
        json.contains("multicam_groups"),
        "persisted json must serialize multicam_groups"
    );

    let loaded = Project::load_from_string(&json).expect("load_from_string");
    assert_eq!(loaded.multicam_group_count(), 1);

    let loaded_group = loaded
        .multicam_group(group_id)
        .expect("loaded group exists");
    assert_eq!(loaded_group.name, "Multi Cam Project");
    assert_eq!(loaded_group.angles.len(), 3);
    assert_eq!(loaded_group.angles[0].name.as_deref(), Some("Cam A"));
    assert_eq!(loaded_group.angles[1].sync_offset, 15);
    assert_eq!(loaded_group.angles[2].sync_offset, -10);

    assert_eq!(loaded_group.cuts.len(), 3);
    assert_eq!(loaded_group.cuts[0], MulticamCut::new(24, 1));
    assert_eq!(loaded_group.cuts[1], MulticamCut::new(48, 2));
    assert_eq!(loaded_group.cuts[2], MulticamCut::new(96, 0));

    // Verify active angle calculation on loaded project
    assert_eq!(loaded.active_multicam_angle_at(group_id, 0).unwrap(), 0);
    assert_eq!(loaded.active_multicam_angle_at(group_id, 30).unwrap(), 1);
    assert_eq!(loaded.active_multicam_angle_at(group_id, 50).unwrap(), 2);
    assert_eq!(loaded.active_multicam_angle_at(group_id, 100).unwrap(), 0);

    // Verify loaded clip on track
    let loaded_clip = loaded.clip(clip_id).expect("loaded clip exists");
    assert_eq!(loaded_clip.multicam_group(), Some(group_id));
}

#[test]
fn older_project_file_without_multicam_loads_with_empty_groups() {
    let mut project = Project::new("legacy_proj", R24);
    let m1 = project.add_media(sample_media(R24, 120));
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, m1, tr(0, 60), rt(0)).unwrap();

    let json = project.save_to_string().expect("save");
    assert!(
        !json.contains("multicam_groups"),
        "empty multicam_groups must be skipped during serialization"
    );

    let loaded = Project::load_from_string(&json).expect("load older file");
    assert_eq!(loaded.multicam_group_count(), 0);
    assert!(loaded.multicam_group_iter().next().is_none());
}

/// One owner answers which camera is live, so the media, the sync offset and the
/// renderer's decode always come off the same angle.
///
/// Defect class closed: the question "which angle is live at this tick" was
/// answered by two lookups in a row (a cut-list index, then an `angles.get`),
/// written out separately in the model and in the resolver. A group whose cut
/// list named an angle it does not hold would answer one of those questions
/// differently from another: a media of `None` beside a sync offset read off a
/// neighbouring camera.
///
/// What this does not catch: whether the renderer calls `live_at` at all. The
/// resolver's own suite covers which frame a cut puts on screen.
#[test]
fn every_reader_of_a_live_camera_reads_the_same_angle() {
    let mut project = Project::new("multicam_one_owner", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let group_id = project
        .create_multicam_group(
            "Rig",
            vec![MulticamAngle::new(m1, 0), MulticamAngle::new(m2, 12)],
        )
        .expect("create group");
    project.switch_multicam_angle(group_id, 24, 1).unwrap();
    let group = project.multicam_group(group_id).expect("group exists");

    // Before the first cut, on it, and past it: three readers, one angle.
    for at in [0, 23, 24, 100] {
        let live = group.live_at(at).expect("an in-bounds cut names an angle");
        assert_eq!(
            group.active_media_at(at),
            Some(live.media),
            "the media at {at} came off another angle"
        );
        assert_eq!(
            group.active_sync_offset_at(at),
            live.sync_offset,
            "the sync offset at {at} came off another angle"
        );
        assert_eq!(
            group.angles[group.active_angle_at(at)].media,
            live.media,
            "the index at {at} points somewhere else"
        );
    }

    // A cut naming an angle the group does not hold is invalid, and every
    // reader falls dead together rather than one of them reading a neighbour.
    let mut broken =
        MulticamGroup::new("Broken", vec![MulticamAngle::new(m1, 48)]).expect("one angle is valid");
    broken.cuts.push(MulticamCut::new(200, 7));
    assert!(
        broken.validate().is_err(),
        "a cut past the angle list must be refused"
    );
    assert!(broken.live_at(200).is_none());
    assert_eq!(broken.active_media_at(200), None);
    assert_eq!(
        broken.active_sync_offset_at(200),
        0,
        "an offset must not be read off an angle nobody is on"
    );
}

#[test]
fn moving_an_angle_returns_the_offset_it_replaced() {
    let mut project = Project::new("multicam_offset", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let m2 = project.add_media(sample_media(R24, 240));
    let group_id = project
        .create_multicam_group(
            "Rig",
            vec![MulticamAngle::new(m1, 0), MulticamAngle::new(m2, 12)],
        )
        .expect("create group");
    // The previous value is what an undo restores, so it comes back from the
    // call rather than being read separately: two reads could disagree.
    assert_eq!(
        project.set_multicam_angle_offset(group_id, 1, -30),
        Ok(12),
        "setting an offset answers with the one it replaced"
    );
    assert_eq!(
        project
            .multicam_group(group_id)
            .expect("group exists")
            .angles[1]
            .sync_offset,
        -30,
        "a negative offset is a camera that started early, not an error"
    );
    assert_eq!(
        project.set_multicam_angle_offset(group_id, 1, -30),
        Ok(-30),
        "moving an angle to where it already is is not a refusal"
    );
}

#[test]
fn moving_an_angle_the_group_does_not_hold_is_refused_by_name() {
    let mut project = Project::new("multicam_offset_bounds", R24);
    let m1 = project.add_media(sample_media(R24, 240));
    let group_id = project
        .create_multicam_group("Rig", vec![MulticamAngle::new(m1, 0)])
        .expect("create group");
    assert_eq!(
        project.set_multicam_angle_offset(group_id, 3, 10),
        Err(ModelError::MulticamAngleOutOfBounds { index: 3, total: 1 }),
        "the refusal names the count the group actually holds"
    );
    assert_eq!(
        project.set_multicam_angle_offset(MulticamGroupId::from_raw(9_999), 0, 10),
        Err(ModelError::UnknownMulticamGroup(MulticamGroupId::from_raw(
            9_999
        ))),
        "a group id nothing holds is refused rather than created"
    );
}
