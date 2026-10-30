//! `freeze_frame` tests covering insertion and undo. A `#[path]` child of the
//! `freeze_frame` test binary; the tests reach their fixtures through
//! `super::*`.

use super::*;

#[test]
fn inserts_at_start_middle_and_end_while_preserving_existing_gap() {
    for (at, expected_count) in [(10, 3), (30, 4), (60, 3)] {
        let fixture = video_fixture(80);
        let source_before = fixture.project.clip(fixture.source).unwrap().clone();
        let held_at = if at == 60 { 59 } else { at };
        let expected_source = source_before.source_time_at(rt(held_at)).unwrap().unwrap();
        let mut engine = engine(fixture.project);

        let freeze = apply_freeze(&mut engine, fixture.source, rt(at), rt(6));
        let frozen = engine.project().clip(freeze).unwrap();
        assert!(frozen.freeze_frame);
        assert_eq!(frozen.timeline, tr(at, 6));
        assert_eq!(
            frozen.source_range(),
            Some(TimeRange::at_rate(expected_source.value, 1, FPS))
        );
        for tick in at..at + 6 {
            assert_eq!(
                frozen.source_time_at(rt(tick)).unwrap(),
                Some(expected_source)
            );
        }

        let layout = track_layout(&engine, fixture.track);
        assert_eq!(layout.len(), expected_count);
        assert_eq!(
            engine.project().clip(fixture.next).unwrap().start().value,
            86,
            "the later clip shifts and the original 20-tick gap survives"
        );
        match at {
            10 => {
                assert_eq!(
                    layout,
                    vec![
                        (freeze, 10, 6),
                        (fixture.source, 16, 50),
                        (fixture.next, 86, 20),
                    ]
                );
            }
            30 => {
                let tail = layout
                    .iter()
                    .find(|(id, start, _)| {
                        *id != freeze
                            && *id != fixture.source
                            && *id != fixture.next
                            && *start == 36
                    })
                    .expect("split tail")
                    .0;
                assert_eq!(
                    layout,
                    vec![
                        (fixture.source, 10, 20),
                        (freeze, 30, 6),
                        (tail, 36, 30),
                        (fixture.next, 86, 20),
                    ]
                );
            }
            60 => {
                assert_eq!(
                    layout,
                    vec![
                        (fixture.source, 10, 50),
                        (freeze, 60, 6),
                        (fixture.next, 86, 20),
                    ]
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn middle_freeze_is_one_undo_step_and_redo_restores_stable_ids() {
    let mut fixture = video_fixture(80);
    let orphan = LinkId::next();
    fixture
        .project
        .timeline_mut()
        .clip_mut(fixture.source)
        .unwrap()
        .link = Some(orphan);
    let before = snapshot(&fixture.project);
    let mut engine = engine(fixture.project);

    let freeze = apply_freeze(&mut engine, fixture.source, rt(30), rt(7));
    let tail = track_layout(&engine, fixture.track)
        .into_iter()
        .map(|(id, _, _)| id)
        .find(|id| *id != fixture.source && *id != fixture.next && *id != freeze)
        .expect("split tail");
    let freeze_after = engine.project().clip(freeze).unwrap().clone();
    let tail_after = engine.project().clip(tail).unwrap().clone();
    let after = snapshot(engine.project());
    assert_eq!(engine.project().clip(fixture.source).unwrap().link, None);

    assert!(engine.undo());
    assert_eq!(snapshot(engine.project()), before);
    assert_eq!(
        engine.project().clip(fixture.source).unwrap().link,
        Some(orphan)
    );
    assert!(
        !engine.undo(),
        "the command records exactly one history entry"
    );

    assert!(engine.redo());
    assert_eq!(snapshot(engine.project()), after);
    assert_eq!(engine.project().clip(freeze).unwrap(), &freeze_after);
    assert_eq!(engine.project().clip(tail).unwrap(), &tail_after);
}

#[test]
fn reverse_and_speed_curve_choose_the_pre_mutation_held_frame() {
    // Constant-speed reverse.
    let mut project = Project::new("reverse freeze", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/reverse.mp4",
        1920,
        1080,
        FPS,
        1_000,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let source = project.add_clip(track, media, tr(100, 120), rt(0)).unwrap();
    project
        .set_clip_speed(source, Rational::new(2, 1), true)
        .unwrap();
    let at = rt(20);
    let expected = project
        .clip(source)
        .unwrap()
        .source_time_at(at)
        .unwrap()
        .unwrap();
    let mut reverse_engine = engine(project);
    let frozen = apply_freeze(&mut reverse_engine, source, at, rt(5));
    assert_eq!(
        reverse_engine
            .project()
            .clip(frozen)
            .unwrap()
            .source_range(),
        Some(TimeRange::at_rate(expected.value, 1, FPS))
    );

    // Integrated speed ramp.
    let mut project = Project::new("curve freeze", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/curve.mp4",
        1920,
        1080,
        FPS,
        1_000,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let source = project.add_clip(track, media, tr(200, 120), rt(0)).unwrap();
    project
        .set_clip_speed_curve(
            source,
            Some(Param::Keyframed {
                keyframes: vec![
                    Keyframe {
                        tick: 0,
                        value: 0.5,
                        easing: Easing::Linear,
                        tangents: None,
                    },
                    Keyframe {
                        tick: 1_000,
                        value: 2.0,
                        easing: Easing::Linear,
                        tangents: None,
                    },
                ],
            }),
        )
        .unwrap();
    let at = rt(70);
    let source_before = project.clip(source).unwrap();
    let expected = source_before.source_time_at(at).unwrap().unwrap();
    assert_ne!(
        source_before.source_time_at(rt(69)).unwrap(),
        Some(expected),
        "chosen split must advance a source frame"
    );
    let mut curve_engine = engine(project);
    let frozen = apply_freeze(&mut curve_engine, source, at, rt(8));
    assert_eq!(
        curve_engine.project().clip(frozen).unwrap().source_range(),
        Some(TimeRange::at_rate(expected.value, 1, FPS))
    );
}

#[test]
fn mixed_rate_freeze_holds_one_native_source_frame() {
    let source_rate = Rational::FPS_30;
    let mut project = Project::new("mixed-rate freeze", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/mixed-rate.mp4",
        1920,
        1080,
        source_rate,
        300,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let source = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(30, 150, source_rate),
            rt(0),
        )
        .unwrap();
    let at = rt(48);
    let expected = project
        .clip(source)
        .unwrap()
        .source_time_at(at)
        .unwrap()
        .unwrap();
    let mut engine = engine(project);

    let frozen = apply_freeze(&mut engine, source, at, rt(12));
    let frozen = engine.project().clip(frozen).unwrap();

    assert_eq!(expected.rate, source_rate);
    assert_eq!(
        frozen.source_range(),
        Some(TimeRange::at_rate(expected.value, 1, source_rate))
    );
    for tick in at.value..at.value + 12 {
        assert_eq!(
            frozen.source_time_at(rt(tick)).unwrap(),
            Some(expected),
            "held frame changed at timeline tick {tick}"
        );
    }
}

#[test]
fn structural_transition_pruning_undo_and_redo_match_other_edits() {
    let mut fixture = video_fixture(60);
    fixture
        .project
        .add_transition(fixture.source, "crossfade")
        .unwrap();
    let mut engine = engine(fixture.project);

    let freeze = apply_freeze(&mut engine, fixture.source, rt(60), rt(5));
    assert!(
        engine
            .project()
            .timeline()
            .track(fixture.track)
            .unwrap()
            .transition_at(fixture.source)
            .is_none()
    );
    assert!(engine.project().clip(freeze).is_some());

    assert!(engine.undo());
    let transition = engine
        .project()
        .timeline()
        .track(fixture.track)
        .unwrap()
        .transition_at(fixture.source)
        .expect("undo restores source junction");
    assert_eq!(transition.right, fixture.next);
    assert!(engine.project().clip(freeze).is_none());

    assert!(engine.redo());
    assert!(
        engine
            .project()
            .timeline()
            .track(fixture.track)
            .unwrap()
            .transition_at(fixture.source)
            .is_none()
    );
    assert!(engine.project().clip(freeze).is_some());
}
