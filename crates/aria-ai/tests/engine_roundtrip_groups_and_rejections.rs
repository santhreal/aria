//! `engine_roundtrip` tests covering groups and rejections. Split out as a pure move to stay
//! under the 500-line ceiling; a `#[path]` child of the `engine_roundtrip` test binary, so
//! `use super::*` reaches its private fixtures unchanged.

use super::*;

#[test]
fn duplicate_clip_wire_preserves_properties_and_round_trips_one_edit() {
    let mut project = Project::new("agent duplicate", R24);
    let source_track = project.add_track(TrackKind::Sticker, "Source");
    let destination_track = project.add_track(TrackKind::Sticker, "Copies");
    let source = project
        .add_generated(
            source_track,
            Generator::SolidColor {
                rgba: [17, 34, 51, 255],
            },
            TimeRange::at_rate(0, 48, R24),
        )
        .unwrap();
    let companion = project
        .add_generated(
            source_track,
            Generator::SolidColor {
                rgba: [51, 34, 17, 255],
            },
            TimeRange::at_rate(48, 48, R24),
        )
        .unwrap();
    let link = LinkId::next();
    for id in [source, companion] {
        project.timeline_mut().clip_mut(id).unwrap().link = Some(link);
    }
    project
        .set_param_keyframe(
            source,
            ClipParam::Scale,
            RationalTime::new(24, R24),
            ParamValue::Scalar(1.5),
            Easing::EaseOut,
            None,
        )
        .unwrap();
    project.add_effect(source, "gaussian_blur").unwrap();
    project.set_effect_param(source, 0, 0, 9.0).unwrap();
    project
        .timeline_mut()
        .clip_mut(source)
        .unwrap()
        .animation_combo = Some(AnimationRef::new("pulse"));
    let source_before = project.clip(source).unwrap().clone();
    let companion_before = project.clip(companion).unwrap().clone();
    let mut engine = engine_with(project);

    let outcome = apply(
        &mut engine,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip: source.raw(),
            to_track: destination_track.raw(),
            start: 5.0,
        }),
    );
    let duplicate = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };
    assert_ne!(duplicate, source);

    let duplicate_before_undo = engine.project().clip(duplicate).unwrap().clone();
    let mut expected = source_before.clone();
    expected.id = duplicate;
    expected.timeline.start = RationalTime::new(120, R24);
    expected.link = None;
    assert_eq!(duplicate_before_undo, expected);
    assert_eq!(
        engine.project().timeline().track_of(duplicate),
        Some(destination_track)
    );
    assert_eq!(engine.project().clip(source).unwrap(), &source_before);
    assert_eq!(engine.project().clip(companion).unwrap(), &companion_before);

    assert!(engine.undo());
    assert!(engine.project().clip(duplicate).is_none());
    assert_eq!(engine.project().clip(source).unwrap(), &source_before);
    assert!(
        !engine.undo(),
        "the duplicate command was exactly one history entry"
    );

    assert!(engine.redo());
    assert_eq!(
        engine.project().clip(duplicate).unwrap(),
        &duplicate_before_undo,
        "redo restores the same id and complete clip state"
    );
}

#[test]
fn unlink_one_member_dissolves_complete_group_and_undoes_once() {
    let mut project = Project::new("agent-unlink", R24);
    let track = project.add_track(TrackKind::Sticker, "Overlays");
    let clips = [
        project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 0, 0, 255],
                },
                TimeRange::at_rate(0, 24, R24),
            )
            .unwrap(),
        project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [0, 255, 0, 255],
                },
                TimeRange::at_rate(24, 24, R24),
            )
            .unwrap(),
        project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [0, 0, 255, 255],
                },
                TimeRange::at_rate(48, 24, R24),
            )
            .unwrap(),
    ];
    let link = LinkId::next();
    for clip in clips {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }
    let mut engine = engine_with(project);

    let outcome = apply(
        &mut engine,
        WireCommand::UnlinkClips(wire::UnlinkClips {
            clips: vec![clips[1].raw()],
        }),
    );
    assert!(matches!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::Updated(id)) if id == clips[1]
    ));
    for clip in clips {
        assert_eq!(engine.project().clip(clip).unwrap().link, None);
    }

    assert!(engine.undo(), "the unlink is one history step");
    for clip in clips {
        assert_eq!(engine.project().clip(clip).unwrap().link, Some(link));
    }
    assert!(!engine.undo(), "the fixture itself created no history");

    assert!(engine.redo());
    for clip in clips {
        assert_eq!(engine.project().clip(clip).unwrap().link, None);
    }
}

#[test]
fn engine_rejections_leave_state_untouched() {
    let mut project = Project::new("agent-rejects", R24);
    let media = project
        .add_media(MediaSource::new(
            "/tmp/agent-rejects.mp4",
            1920,
            1080,
            R24,
            20 * 24,
            true,
        ))
        .raw();
    let mut engine = engine_with(project);

    let video = created_track(apply(
        &mut engine,
        WireCommand::AddTrack(wire::AddTrack {
            kind: wire::WireTrackKind::Video,
            name: "V1".into(),
            index: None,
        }),
    ));
    created_clip(apply(
        &mut engine,
        WireCommand::AddClip(wire::AddClip {
            track: video,
            media,
            source_start: 0.0,
            source_duration: 5.0,
            start: 0.0,
        }),
    ));

    // Validation passes (overlap is the engine's call), the engine rejects,
    // and nothing changed — the loop feeds this error back to the model.
    let overlapping = validate(
        &WireCommand::AddClip(wire::AddClip {
            track: video,
            media,
            source_start: 0.0,
            source_duration: 5.0,
            start: 2.0,
        }),
        engine.project(),
    )
    .expect("overlap is not validation's call");
    let Lowered::Edit(overlapping) = overlapping else {
        panic!("add_clip is an edit");
    };
    let before_clips = engine.project().timeline().clip_count();
    assert!(engine.apply(overlapping).is_err());
    assert_eq!(engine.project().timeline().clip_count(), before_clips);

    // And a failed apply must not have pushed an undo entry: one undo
    // removes the clip, the next removes the track, then history is empty.
    assert!(engine.undo());
    assert!(engine.undo());
    assert!(!engine.undo());
}

#[test]
fn validate_is_pure_against_engine_state() {
    let engine = engine_with(Project::new("empty", R24));

    let rejection = validate(
        &WireCommand::RemoveClip(wire::RemoveClip { clip: 1 }),
        engine.project(),
    )
    .unwrap_err();
    assert!(rejection.message.contains("does not exist"));

    // Lowering never mutates: the project is untouched by validation.
    assert_eq!(engine.project().timeline().track_count(), 0);

    // Round-trip a serialized plan entry (the dry-run path).
    let plan = serde_json::json!({
        "command": "add_track", "kind": "video", "name": "V1"
    });
    let wire: WireCommand = serde_json::from_value(plan).unwrap();
    let lowered = validate(&wire, engine.project()).unwrap();
    assert!(matches!(lowered, Lowered::Edit(Command::Edit(_))));
}
