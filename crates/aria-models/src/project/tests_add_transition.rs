//! `add_transition` cases.

use super::*;
use crate::clip::Shape;
use std::path::Path;

#[test]
fn text_background_radius_keyframe_through_project_api() {
    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Text, "T1");
    let with_bg = project
        .add_generated(
            track,
            Generator::Text {
                content: "Card".into(),
                style: crate::TextStyle {
                    background: Some(crate::TextBackground::default()),
                    ..Default::default()
                },
            },
            tr(100, 48),
        )
        .unwrap();
    let radius = ClipParam::Text {
        param: crate::TextParam::BackgroundRadius,
    };
    project
        .set_param_keyframe(
            with_bg,
            radius,
            rt(100),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            with_bg,
            radius,
            rt(140),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    let ClipSource::Generated(Generator::Text { style, .. }) =
        &project.clip(with_bg).unwrap().content
    else {
        panic!("expected text");
    };
    assert!((style.background.as_ref().unwrap().radius.sample(20) - 0.5).abs() < 1e-5);

    assert!(matches!(
        project.set_param_keyframe(
            with_bg,
            radius,
            rt(120),
            ParamValue::Scalar(1.5),
            Easing::Linear,
            None
        ),
        Err(ModelError::InvalidParam(_))
    ));

    let bare = project
        .add_generated(track, Generator::text("Bare"), tr(200, 48))
        .unwrap();
    assert!(matches!(
        project.set_param_keyframe(
            bare,
            radius,
            rt(200),
            ParamValue::Scalar(0.5),
            Easing::Linear,
            None
        ),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn add_generated_rejects_invalid_shape() {
    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let bad = Generator::shape(Shape::Polygon { sides: 2 }, [255, 255, 255, 255]);
    assert!(matches!(
        project.add_generated(track, bad, tr(0, 48)),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn add_transition_links_abutting_pair() {
    let (mut project, left, right, track) = project_with_abutting_pair();
    project.add_transition(left, "crossfade").unwrap();
    let t = project
        .timeline()
        .track(track)
        .unwrap()
        .transition_at(left)
        .unwrap();
    assert_eq!(t.right, right);
    assert_eq!(t.transition_id, "crossfade");
    assert_eq!(t.duration, crate::transition::DEFAULT_TRANSITION_TICKS);
}

#[test]
fn add_transition_rejects_unknown_id_and_non_abutting() {
    let (mut project, left, _right, _track) = project_with_abutting_pair();
    assert!(matches!(
        project.add_transition(left, "warp_speed"),
        Err(ModelError::InvalidParam(_))
    ));
    // A lone clip with no right neighbor cannot take a transition.
    let track = project.add_track(TrackKind::Sticker, "S2");
    let lone = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 255, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    assert!(matches!(
        project.add_transition(lone, "crossfade"),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn add_transition_rejects_canvas_pass_lanes() {
    // Effect/filter/adjustment segments resolve to canvas-wide passes,
    // which the renderer can't nest inside a transition — the model
    // refuses the junction outright.
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let left = project
        .add_generated(track, Generator::Adjustment, tr(0, 24))
        .unwrap();
    project
        .add_generated(track, Generator::Adjustment, tr(24, 24))
        .unwrap();
    assert!(matches!(
        project.add_transition(left, "crossfade"),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(!project.has_transitions());
}

#[test]
fn set_and_remove_transition_duration() {
    let (mut project, left, _right, track) = project_with_abutting_pair();
    project.add_transition(left, "wipe_left").unwrap();
    project.set_transition_duration(left, 12).unwrap();
    assert_eq!(
        project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .unwrap()
            .duration,
        12
    );
    project.remove_transition(left).unwrap();
    assert!(
        project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .is_none()
    );
    assert!(matches!(
        project.remove_transition(left),
        Err(ModelError::InvalidParam(_))
    ));
}
#[test]
fn transition_alignment_windows_limits_and_serde_round_trip() {
    use crate::transition::{Transition, TransitionAlignment, transition_window};

    let cut = 48;
    let duration = 24;
    assert_eq!(
        transition_window(TransitionAlignment::Center, duration, cut),
        (36, 60)
    );
    assert_eq!(
        transition_window(TransitionAlignment::Start, duration, cut),
        (48, 72)
    );
    assert_eq!(
        transition_window(TransitionAlignment::End, duration, cut),
        (24, 48)
    );

    // Old serialized transitions without `alignment` load as Center and round-trip
    // byte-identically without writing an `alignment` field.
    let legacy_json = r#"{"left":1,"right":2,"transition_id":"crossfade","duration":24}"#;
    let loaded: Transition = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(loaded.alignment, TransitionAlignment::Center);
    assert_eq!(loaded.window(cut), (36, 60));
    assert_eq!(serde_json::to_string(&loaded).unwrap(), legacy_json);

    for (align, expected_key, expected_window) in [
        (TransitionAlignment::Start, "start", (48, 72)),
        (TransitionAlignment::End, "end", (24, 48)),
    ] {
        let mut moved = loaded.clone();
        moved.alignment = align;
        assert_eq!(moved.window(cut), expected_window);
        let text = serde_json::to_string(&moved).unwrap();
        assert!(
            text.contains(&format!(r#""alignment":"{expected_key}""#)),
            "{text}"
        );
        let back: Transition = serde_json::from_str(&text).unwrap();
        assert_eq!(back, moved);
    }

    // Asymmetric clip lengths: left is 16 ticks, right is 48 ticks.
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 16),
        )
        .unwrap();
    let _right = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(16, 48),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 16).unwrap();
    assert_eq!(project.transition_duration_limit(left).unwrap(), 16);

    // Start alignment sits in `right` (48 ticks), so a 32-tick window fits there
    // and is refused when switched to Center or End (where `left` is 16 ticks).
    project
        .set_transition_alignment(left, TransitionAlignment::Start)
        .unwrap();
    assert_eq!(project.transition_duration_limit(left).unwrap(), 48);
    project.set_transition_duration(left, 32).unwrap();
    assert_eq!(project.transition_at(left).unwrap().window(16), (16, 48));
    assert!(matches!(
        project.set_transition_alignment(left, TransitionAlignment::Center),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        project.set_transition_alignment(left, TransitionAlignment::End),
        Err(ModelError::InvalidParam(_))
    ));

    project.set_transition_duration(left, 16).unwrap();
    project
        .set_transition_alignment(left, TransitionAlignment::End)
        .unwrap();
    assert_eq!(project.transition_at(left).unwrap().window(16), (0, 16));
}

#[test]
fn prune_drops_transition_when_junction_breaks() {
    let (mut project, left, _right, track) = project_with_abutting_pair();
    project.add_transition(left, "slide").unwrap();
    assert!(project.has_transitions());

    // Structural mutation prunes the invalid junction atomically.
    project.move_clip(left, track, rt(100)).unwrap();
    assert!(!project.has_transitions());
    // Idempotent: an explicit prune finds nothing left to do.
    assert!(!project.prune_dead_transitions());
}

#[test]
fn transitions_snapshot_round_trips() {
    let (mut project, left, _right, _track) = project_with_abutting_pair();
    project.add_transition(left, "dip_to_black").unwrap();
    let snapshot = project.transitions_snapshot();

    project.remove_transition(left).unwrap();
    assert!(!project.has_transitions());

    project.restore_transitions(snapshot);
    assert!(project.has_transitions());
}

/// Two abutting media clips on an audio lane; returns `(project, left, right)`.
fn audio_cut() -> (Project, ClipId, ClipId) {
    let mut project = Project::new("test", R24);
    let media = project.add_media(sample_media(R24, 240));
    let track = project.add_track(TrackKind::Audio, "A1");
    let left = project.add_clip(track, media, tr(24, 48), rt(0)).unwrap();
    let right = project.add_clip(track, media, tr(96, 48), rt(48)).unwrap();
    (project, left, right)
}

/// A cut on an audio lane takes every audio transition and no picture one, and
/// a picture lane the reverse; each refusal names the other set, so the
/// message says what to use instead.
#[test]
fn each_lane_takes_only_its_own_kind_of_transition() {
    let (mut project, left, right) = audio_cut();
    for transition in crate::AudioTransition::ALL {
        project.add_transition(left, transition.id()).unwrap();
        let track = project.timeline().track_of(left).unwrap();
        let stored = project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .unwrap()
            .clone();
        assert_eq!(stored.transition_id, transition.id());
        assert_eq!(stored.right, right);
    }
    let refused = project.add_transition(left, "crossfade").unwrap_err();
    assert_eq!(
        refused.to_string(),
        ModelError::InvalidParam(
            "Cross Fade is a picture transition; a cut on an audio lane takes Constant Power, \
             Constant Gain or Exponential Fade"
                .into()
        )
        .to_string()
    );

    let (mut picture, left, _right, _track) = project_with_abutting_pair();
    for transition in crate::AudioTransition::ALL {
        let refused = picture.add_transition(left, transition.id()).unwrap_err();
        assert_eq!(
            refused.to_string(),
            ModelError::InvalidParam(format!(
                "{} is an audio transition; it goes on a cut between clips on an audio lane",
                transition.label()
            ))
            .to_string()
        );
    }
    assert!(!picture.has_transitions());
}

// --- Project::new -----------------------------------------------------

#[test]
fn new_creates_empty_project_at_frame_rate() {
    let project = Project::new("my edit", R24);
    assert_eq!(project.schema, ProjectSchema::current());
    assert_eq!(project.metadata, ProjectMetadata::default());
    assert_eq!(project.name, "my edit");
    assert_eq!(project.timeline().frame_rate, R24);
    assert_eq!(project.media_count(), 0);
    assert_eq!(project.timeline().track_count(), 0);
    assert_eq!(project.timeline().clip_count(), 0);
    assert!(project.id.raw() >= 1);
}

#[test]
fn new_accepts_string_name() {
    let owned = Project::new(String::from("owned"), R24);
    assert_eq!(owned.name, "owned");
}

// --- media pool -------------------------------------------------------

#[test]
fn add_media_returns_id_and_lookup_works() {
    let mut project = Project::new("test", R24);
    let media = sample_media(R24, 500);
    let id = project.add_media(media.clone());

    assert_eq!(project.media_count(), 1);
    assert_eq!(project.media(id).unwrap().path(), media.path());
}

#[test]
fn media_iter_visits_all_sources() {
    let mut project = Project::new("test", R24);
    project.add_media(sample_media(R24, 10));
    project.add_media(sample_media(R24, 20));
    let durations: Vec<i64> = project.media_iter().map(|m| m.duration.value).collect();
    assert_eq!(durations.len(), 2);
    assert!(durations.contains(&10));
    assert!(durations.contains(&20));
}

#[test]
fn find_media_by_path_matches_same_file() {
    let mut project = Project::new("test", R24);
    let id = project.add_media(sample_media(R24, 10));
    assert_eq!(
        project.find_media_by_path(Path::new("/tmp/sample.mp4")),
        Some(id)
    );
    assert_eq!(
        project.find_media_by_path(Path::new("/tmp/other.mp4")),
        None
    );
}

#[test]
fn remove_media_unknown_errors() {
    let mut project = Project::new("test", R24);
    let missing = MediaId::next();
    assert_eq!(
        project.remove_media(missing),
        Err(ModelError::UnknownMedia(missing))
    );
}

// --- pool folders -----------------------------------------------------
//
// The class these close: a bin holds nothing, so every relationship in the pool
// tree is a field on something else. Deleting or moving a bin is therefore a
// chance to orphan media or to make a subtree unreachable, and both are asserted
// rather than assumed.

#[test]
fn a_bin_nests_and_reports_its_ancestors_nearest_first() {
    let mut project = Project::new("bins", R24);
    let top = project.add_bin("Footage", None).unwrap();
    let mid = project.add_bin("Day 1", Some(top)).unwrap();
    let leaf = project.add_bin("A cam", Some(mid)).unwrap();
    assert_eq!(project.bin_count(), 3);
    assert_eq!(
        project.bin_ancestors(leaf).collect::<Vec<_>>(),
        vec![mid, top]
    );
    assert_eq!(project.bin_ancestors(top).collect::<Vec<_>>(), Vec::new());
    assert_eq!(project.bin(leaf).unwrap().name, "A cam");
}

#[test]
fn a_bin_under_a_parent_that_does_not_exist_is_refused() {
    let mut project = Project::new("bins", R24);
    let missing = BinId::next();
    assert_eq!(
        project.add_bin("Orphan", Some(missing)),
        Err(ModelError::UnknownBin(missing))
    );
    assert_eq!(project.bin_count(), 0);
}

#[test]
fn a_bin_cannot_be_moved_inside_itself_or_its_own_subtree() {
    let mut project = Project::new("bins", R24);
    let top = project.add_bin("Footage", None).unwrap();
    let mid = project.add_bin("Day 1", Some(top)).unwrap();
    let leaf = project.add_bin("A cam", Some(mid)).unwrap();
    assert_eq!(
        project.move_bin(top, Some(top)),
        Err(ModelError::BinCycle(top))
    );
    assert_eq!(
        project.move_bin(top, Some(leaf)),
        Err(ModelError::BinCycle(top)),
        "a move into a descendant would cut the subtree off from the root"
    );
    // The legal directions still work, including back to the top level.
    project.move_bin(leaf, Some(top)).unwrap();
    assert_eq!(project.bin(leaf).unwrap().parent, Some(top));
    project.move_bin(leaf, None).unwrap();
    assert_eq!(project.bin(leaf).unwrap().parent, None);
    assert_eq!(project.bin(mid).unwrap().parent, Some(top));
}

#[test]
fn deleting_a_bin_promotes_what_was_inside_it_rather_than_losing_it() {
    let mut project = Project::new("bins", R24);
    let top = project.add_bin("Footage", None).unwrap();
    let mid = project.add_bin("Day 1", Some(top)).unwrap();
    let leaf = project.add_bin("A cam", Some(mid)).unwrap();
    let media = project.add_media(sample_media(R24, 100));
    project.move_media_to_bin(media, Some(mid)).unwrap();

    let removed = project.remove_bin(mid).unwrap();
    assert_eq!(removed.id, mid);
    assert_eq!(project.bin(mid), None);
    assert_eq!(
        project.bin(leaf).unwrap().parent,
        Some(top),
        "the child bin moved up to the deleted bin's parent"
    );
    assert_eq!(
        project.media(media).unwrap().bin,
        Some(top),
        "the source moved up with it: a folder never takes media with it"
    );

    // Deleting a top-level bin leaves its contents at the pool root.
    project.remove_bin(top).unwrap();
    assert_eq!(project.media(media).unwrap().bin, None);
    assert_eq!(project.bin(leaf).unwrap().parent, None);
}

#[test]
fn an_unknown_bin_is_named_rather_than_ignored() {
    let mut project = Project::new("bins", R24);
    let media = project.add_media(sample_media(R24, 100));
    let missing = BinId::next();
    assert_eq!(
        project.remove_bin(missing),
        Err(ModelError::UnknownBin(missing))
    );
    assert_eq!(
        project.rename_bin(missing, "x"),
        Err(ModelError::UnknownBin(missing))
    );
    assert_eq!(
        project.move_bin(missing, None),
        Err(ModelError::UnknownBin(missing))
    );
    assert_eq!(
        project.move_media_to_bin(media, Some(missing)),
        Err(ModelError::UnknownBin(missing))
    );
    let absent = MediaId::next();
    assert_eq!(
        project.move_media_to_bin(absent, None),
        Err(ModelError::UnknownMedia(absent))
    );
}

#[test]
fn moving_media_reports_the_bin_it_came_from_so_an_undo_can_put_it_back() {
    let mut project = Project::new("bins", R24);
    let one = project.add_bin("One", None).unwrap();
    let two = project.add_bin("Two", None).unwrap();
    let media = project.add_media(sample_media(R24, 100));
    assert_eq!(project.move_media_to_bin(media, Some(one)).unwrap(), None);
    assert_eq!(
        project.move_media_to_bin(media, Some(two)).unwrap(),
        Some(one)
    );
    assert_eq!(project.move_media_to_bin(media, None).unwrap(), Some(two));
    assert_eq!(project.media(media).unwrap().bin, None);
}

#[test]
fn a_restored_bin_keeps_its_own_id() {
    // An undo must put the bin back, not a new bin with the same name: media
    // still names the old id.
    let mut project = Project::new("bins", R24);
    let top = project.add_bin("Footage", None).unwrap();
    let media = project.add_media(sample_media(R24, 100));
    project.move_media_to_bin(media, Some(top)).unwrap();
    let removed = project.remove_bin(top).unwrap();
    project.restore_bin(removed).unwrap();
    project.move_media_to_bin(media, Some(top)).unwrap();
    assert_eq!(project.bin(top).unwrap().name, "Footage");
    assert_eq!(project.media(media).unwrap().bin, Some(top));
}
