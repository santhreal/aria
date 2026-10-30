//! `engine_roundtrip` tests covering scenario and undo. A `#[path]` child of
//! the `engine_roundtrip` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn prompt_sized_scenario_round_trips_and_unwinds() {
    let mut project = Project::new("agent-fixture", R24);
    let media = project
        .add_media(MediaSource::new(
            "/tmp/agent-roundtrip.mp4",
            1920,
            1080,
            R24,
            60 * 24,
            true,
        ))
        .raw();
    let mut engine = engine_with(project);

    // "Lay down 10s of footage, cut it at 4s, keep the tail trimmed to 4s,
    // ripple the head away, then add a styled title and link it."
    let video = created_track(apply(
        &mut engine,
        WireCommand::AddTrack(wire::AddTrack {
            kind: wire::WireTrackKind::Video,
            name: "V1".into(),
            index: None,
        }),
    ));
    let head = created_clip(apply(
        &mut engine,
        WireCommand::AddClip(wire::AddClip {
            track: video,
            media,
            source_start: 0.0,
            source_duration: 10.0,
            start: 0.0,
        }),
    ));
    let tail = created_clip(apply(
        &mut engine,
        WireCommand::SplitClip(wire::SplitClip {
            clip: head,
            at: 4.0,
        }),
    ));

    // Trim the tail from [4s, 10s) to [6s, 10s): head-trim advances the
    // source in-point by the same 2s.
    apply(
        &mut engine,
        WireCommand::TrimClip(wire::TrimClip {
            clip: tail,
            start: 6.0,
            duration: 4.0,
        }),
    );
    {
        let clip = engine
            .project()
            .clip(aria_models::ClipId::from_raw(tail))
            .unwrap();
        assert_eq!(clip.timeline.start.value, 144); // 6s * 24
        assert_eq!(clip.timeline.duration.value, 96); // 4s * 24
        assert_eq!(clip.source_range().unwrap().start.value, 144);
    }

    // Ripple the head away: the tail slides left by the head's 4s.
    apply(
        &mut engine,
        WireCommand::RippleDelete(wire::RippleDelete { clip: head }),
    );
    assert_eq!(
        engine
            .project()
            .clip(aria_models::ClipId::from_raw(tail))
            .unwrap()
            .timeline
            .start
            .value,
        48 // 6s - 4s = 2s * 24
    );

    let titles = created_track(apply(
        &mut engine,
        WireCommand::AddTrack(wire::AddTrack {
            kind: wire::WireTrackKind::Text,
            name: "Titles".into(),
            index: None,
        }),
    ));
    let title = created_clip(apply(
        &mut engine,
        WireCommand::AddGenerated(wire::AddGenerated {
            track: titles,
            generator: WireGenerator::Text {
                content: "INTRO".into(),
            },
            start: 0.0,
            duration: 3.0,
        }),
    ));
    apply(
        &mut engine,
        WireCommand::SetGenerator(wire::SetGenerator {
            clip: title,
            generator: WireGenerator::Text {
                content: "OUTRO".into(),
            },
        }),
    );
    apply(
        &mut engine,
        WireCommand::SetClipTransform(wire::SetClipTransform {
            clip: title,
            position_x: None,
            position_y: Some(0.3),
            anchor_x: None,
            anchor_y: None,
            scale: Some(wire::WireScale::Uniform(0.5)),
            rotation: None,
            opacity: None,
        }),
    );
    apply(
        &mut engine,
        WireCommand::LinkClips(wire::LinkClips {
            clips: vec![tail, title],
        }),
    );

    // The summary the model would see reflects all of it.
    let summary = summarize(engine.project());
    assert_eq!(summary.tracks.len(), 2);
    let v1 = &summary.tracks[0];
    assert_eq!(v1.clips.len(), 1);
    assert_eq!(v1.clips[0].id, tail);
    assert_eq!(v1.clips[0].start_seconds, 2.0);
    let t1 = &summary.tracks[1];
    assert_eq!(
        t1.clips[0].content,
        aria_ai::describe::ClipContent::Text {
            text: "OUTRO".into()
        }
    );
    assert_eq!(t1.clips[0].link, v1.clips[0].link);
    assert!(v1.clips[0].link.is_some());

    // Ten applied commands = ten history entries; a full unwind leaves
    // the timeline empty (every wire command is exactly as undoable as a
    // gesture).
    let mut undone = 0;
    while engine.undo() {
        undone += 1;
    }
    assert_eq!(undone, 10);
    assert_eq!(engine.project().timeline().track_count(), 0);
    assert_eq!(engine.project().timeline().clip_count(), 0);
}

#[test]
fn move_effect_lowers_applies_and_undoes_exactly() {
    let mut project = Project::new("agent effect order", R24);
    let track = project.add_track(TrackKind::Text, "Titles");
    let clip = project
        .add_generated(
            track,
            Generator::text("TITLE"),
            TimeRange::at_rate(0, 48, R24),
        )
        .unwrap();
    for effect in ["gaussian_blur", "glitch", "vignette"] {
        project.add_effect(clip, effect).unwrap();
    }
    project.set_effect_param(clip, 0, 0, 12.0).unwrap();
    for (tick, value, easing) in [(0, 0.2, Easing::EaseIn), (24, 0.8, Easing::EaseOut)] {
        project
            .set_param_keyframe(
                clip,
                ClipParam::Effect {
                    effect: 1,
                    param: 0,
                },
                RationalTime::new(tick, R24),
                ParamValue::Scalar(value),
                easing,
                None,
            )
            .unwrap();
    }
    project.set_effect_param(clip, 2, 0, 0.75).unwrap();
    let before = project.clip(clip).unwrap().effects.clone();
    let expected = vec![before[1].clone(), before[2].clone(), before[0].clone()];
    let mut engine = engine_with(project);

    let outcome = apply(
        &mut engine,
        WireCommand::MoveEffect(wire::MoveEffect {
            clip: clip.raw(),
            from_index: 0,
            to_index: 2,
        }),
    );
    assert!(matches!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::Updated(id)) if id == clip
    ));
    assert_eq!(engine.project().clip(clip).unwrap().effects, expected);

    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip).unwrap().effects, before);
    assert!(engine.redo());
    assert_eq!(engine.project().clip(clip).unwrap().effects, expected);
}

#[test]
fn extract_audio_lowers_applies_and_undoes_as_one_edit() {
    let mut project = Project::new("agent extract", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/agent-extract.mp4",
        1920,
        1080,
        R24,
        480,
        true,
    ));
    let video_track = project.add_track(TrackKind::Video, "V1");
    let audio_track = project.add_track(TrackKind::Audio, "A1");
    let video = project
        .add_clip(
            video_track,
            media,
            TimeRange::at_rate(24, 96, R24),
            RationalTime::new(48, R24),
        )
        .unwrap();
    let mut engine = engine_with(project);

    let outcome = apply(
        &mut engine,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video.raw(),
            track: audio_track.raw(),
        }),
    );
    let audio = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    };
    let snapshot = engine.project().clip(audio).unwrap().clone();
    assert_eq!(
        snapshot.timeline,
        engine.project().clip(video).unwrap().timeline
    );
    assert_eq!(snapshot.audio_role, Some(AudioRole::Extracted));
    assert_eq!(
        engine.project().timeline().track_of(audio),
        Some(audio_track)
    );
    assert!(!engine.project().timeline().carries_own_audio(video));

    assert!(engine.undo());
    assert!(engine.project().clip(audio).is_none());
    assert!(
        engine
            .project()
            .timeline()
            .track(audio_track)
            .is_some_and(|track| track.is_empty())
    );
    assert!(engine.project().timeline().carries_own_audio(video));
    assert!(
        !engine.undo(),
        "the extraction was exactly one history entry"
    );

    assert!(engine.redo());
    assert_eq!(engine.project().clip(audio).unwrap(), &snapshot);
}
