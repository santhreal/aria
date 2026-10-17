//! `SourceEdit` and `SetSourcePatch`: a Source tab's three- and four-point
//! edits, placed through source patching.
//!
//! The class these close: an edit that lands on two lanes has to open its span
//! on both, exactly once each, and leave the two clips linked so the picture
//! plays silent and its sound plays from the audio lane. So both ways of
//! opening a span (ripple and cover) are asserted on both lanes, a ripple is
//! asserted to move every sync-locked lane by the span once and no more, each
//! channel is placed alone, and every refusal is asserted to leave the project
//! byte-identical. The patch itself is asserted to round-trip through undo and
//! to refuse a lane of the wrong kind.
//!
//! Not covered: which lane a patch left on `Auto` resolves to. That is the
//! Source tab's choice before the command is sent, and is asserted with the UI.

use aria_commands::{Command, EditCommand, EditOutcome, Patch, SoundPlacement, SourceChannel};
use aria_engine::{ApplyOutcome, Engine, EngineConfig, EngineError};
use aria_models::{
    ClipId, MediaId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackId, TrackKind,
};

const FPS: Rational = Rational::FPS_24;

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

/// A sequence with one picture lane, one sound lane, and three sources: a
/// shot with sound, a silent shot, and a sound file.
struct Fixture {
    engine: Engine,
    shot: MediaId,
    silent: MediaId,
    sound: MediaId,
    v1: TrackId,
    a1: TrackId,
}

fn fixture(prepare: impl FnOnce(&mut Project, TrackId, TrackId, MediaId)) -> Fixture {
    let mut project = Project::new("source-edit", FPS);
    let shot = project.add_media(MediaSource::new(
        "/srv/shot.mp4",
        1920,
        1080,
        FPS,
        1_000,
        true,
    ));
    let silent = project.add_media(MediaSource::new(
        "/srv/silent.mp4",
        1920,
        1080,
        FPS,
        1_000,
        false,
    ));
    let sound = project.add_media(MediaSource::new("/srv/sound.wav", 0, 0, FPS, 1_000, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    prepare(&mut project, v1, a1, shot);
    let engine = Engine::with_project(EngineConfig { undo_limit: 32 }, project).expect("engine");
    Fixture {
        engine,
        shot,
        silent,
        sound,
        v1,
        a1,
    }
}

fn empty() -> Fixture {
    fixture(|_, _, _, _| {})
}

#[allow(clippy::too_many_arguments)]
fn source_edit(
    engine: &mut Engine,
    media: MediaId,
    source: TimeRange,
    at: i64,
    fill: Option<i64>,
    ripple: bool,
    track: Option<TrackId>,
    sound: SoundPlacement,
) -> Result<ClipId, EngineError> {
    match engine.apply(Command::Edit(EditCommand::SourceEdit {
        media,
        source,
        at: rt(at),
        fill: fill.map(rt),
        ripple,
        track,
        sound,
    }))? {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => Ok(id),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// Every clip on `lane` as `(start, end)`, in order.
fn spans(engine: &Engine, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = engine
        .project()
        .timeline()
        .track(lane)
        .expect("lane")
        .clips()
        .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick()))
        .collect();
    spans.sort_unstable();
    spans
}

fn only_clip(engine: &Engine, lane: TrackId) -> ClipId {
    let clips: Vec<ClipId> = engine
        .project()
        .timeline()
        .track(lane)
        .expect("lane")
        .clips()
        .map(|clip| clip.id)
        .collect();
    assert_eq!(clips.len(), 1, "one clip on {lane:?}");
    clips[0]
}

fn snapshot(engine: &Engine) -> serde_json::Value {
    serde_json::to_value(engine.project()).expect("project serialises")
}

#[test]
fn a_patched_edit_places_picture_and_sound_linked() {
    let Fixture {
        mut engine,
        shot,
        v1,
        a1,
        ..
    } = empty();

    let picture = source_edit(
        &mut engine,
        shot,
        tr(0, 48),
        24,
        None,
        false,
        Some(v1),
        SoundPlacement::Lane(a1),
    )
    .expect("edit");

    assert_eq!(spans(&engine, v1), vec![(24, 72)]);
    assert_eq!(spans(&engine, a1), vec![(24, 72)]);
    assert_eq!(
        only_clip(&engine, v1),
        picture,
        "the outcome names the picture"
    );
    let sound = only_clip(&engine, a1);
    let link = engine.project().clip(picture).expect("picture").link;
    assert!(link.is_some(), "picture and sound are linked");
    assert_eq!(engine.project().clip(sound).expect("sound").link, link);
    assert!(
        engine.project().timeline().detached_to_audio_lane(picture),
        "the picture plays silent; its sound plays from the audio lane"
    );
}

#[test]
fn a_fill_retimes_picture_and_sound_together() {
    for (source, fill, speed) in [(48, 96, 0.5), (96, 48, 2.0)] {
        let Fixture {
            mut engine,
            shot,
            v1,
            a1,
            ..
        } = empty();
        let picture = source_edit(
            &mut engine,
            shot,
            tr(0, source),
            10,
            Some(fill),
            false,
            Some(v1),
            SoundPlacement::Lane(a1),
        )
        .expect("fit");
        let sound = only_clip(&engine, a1);
        assert_eq!(
            spans(&engine, v1),
            vec![(10, 10 + fill)],
            "{source} into {fill}"
        );
        assert_eq!(
            spans(&engine, a1),
            vec![(10, 10 + fill)],
            "{source} into {fill}"
        );
        for clip in [picture, sound] {
            assert_eq!(engine.project().clip(clip).unwrap().speed.as_f64(), speed);
        }
    }
}

#[test]
fn an_overwrite_covers_both_lanes_and_moves_nothing_downstream() {
    let Fixture {
        mut engine,
        shot,
        v1,
        a1,
        ..
    } = fixture(|project, v1, a1, shot| {
        project
            .add_clip(v1, shot, tr(0, 100), rt(0))
            .expect("v1 bed");
        project
            .add_clip(a1, shot, tr(0, 100), rt(0))
            .expect("a1 bed");
    });

    source_edit(
        &mut engine,
        shot,
        tr(200, 20),
        40,
        None,
        false,
        Some(v1),
        SoundPlacement::Lane(a1),
    )
    .expect("edit");

    for lane in [v1, a1] {
        assert_eq!(
            spans(&engine, lane),
            vec![(0, 40), (40, 60), (60, 100)],
            "{lane:?}"
        );
    }
}

#[test]
fn a_ripple_opens_every_patched_and_sync_locked_lane_once() {
    let Fixture {
        mut engine,
        shot,
        v1,
        a1,
        ..
    } = fixture(|project, v1, a1, shot| {
        let v2 = project.add_track(TrackKind::Video, "V2");
        let pinned = project.add_track(TrackKind::Video, "V3");
        project
            .timeline_mut()
            .track_mut(pinned)
            .expect("pinned lane")
            .sync_lock = false;
        for lane in [v1, a1, v2, pinned] {
            project
                .add_clip(lane, shot, tr(0, 100), rt(0))
                .expect("bed");
        }
    });
    let lanes: Vec<(TrackId, bool)> = engine
        .project()
        .timeline()
        .tracks_ordered()
        .map(|track| (track.id, track.sync_lock))
        .collect();

    source_edit(
        &mut engine,
        shot,
        tr(200, 20),
        40,
        None,
        true,
        Some(v1),
        SoundPlacement::Lane(a1),
    )
    .expect("edit");

    for (lane, sync_lock) in lanes {
        let expected = if lane == v1 || lane == a1 {
            vec![(0, 40), (40, 60), (60, 120)]
        } else if sync_lock {
            vec![(0, 40), (60, 120)]
        } else {
            vec![(0, 100)]
        };
        assert_eq!(
            spans(&engine, lane),
            expected,
            "{lane:?} sync lock {sync_lock}"
        );
    }
}

#[test]
fn a_picture_placed_without_its_sound_is_silenced() {
    let Fixture {
        mut engine,
        shot,
        silent,
        v1,
        a1,
        ..
    } = empty();

    let picture = source_edit(
        &mut engine,
        shot,
        tr(0, 24),
        0,
        None,
        false,
        Some(v1),
        SoundPlacement::Off,
    )
    .expect("picture alone");
    assert_eq!(
        engine.project().clip(picture).unwrap().volume.constant(),
        Some(0.0)
    );
    assert!(spans(&engine, a1).is_empty());

    let quiet = source_edit(
        &mut engine,
        silent,
        tr(0, 24),
        48,
        None,
        false,
        Some(v1),
        SoundPlacement::Off,
    )
    .expect("a silent shot");
    assert_eq!(
        engine.project().clip(quiet).unwrap().volume.constant(),
        Some(1.0),
        "a shot with no sound keeps its gain"
    );
}

#[test]
fn a_sound_kept_in_the_picture_plays_from_it() {
    let Fixture {
        mut engine,
        shot,
        v1,
        a1,
        ..
    } = empty();

    let picture = source_edit(
        &mut engine,
        shot,
        tr(0, 24),
        0,
        None,
        false,
        Some(v1),
        SoundPlacement::InPicture,
    )
    .expect("picture with its sound");
    let placed = engine.project().clip(picture).unwrap();
    assert_eq!(
        placed.volume.constant(),
        Some(1.0),
        "the sound plays from the picture"
    );
    assert_eq!(placed.link, None, "no second clip to link to");
    assert!(!engine.project().timeline().detached_to_audio_lane(picture));
    assert!(spans(&engine, a1).is_empty());
}

#[test]
fn a_sound_placed_without_its_picture_lands_alone() {
    let Fixture {
        mut engine,
        shot,
        sound,
        v1,
        a1,
        ..
    } = empty();

    let from_shot = source_edit(
        &mut engine,
        shot,
        tr(0, 24),
        0,
        None,
        false,
        None,
        SoundPlacement::Lane(a1),
    )
    .expect("a shot's sound alone");
    let from_file = source_edit(
        &mut engine,
        sound,
        tr(0, 24),
        48,
        None,
        false,
        None,
        SoundPlacement::Lane(a1),
    )
    .expect("a sound file");

    assert!(spans(&engine, v1).is_empty());
    assert_eq!(spans(&engine, a1), vec![(0, 24), (48, 72)]);
    for clip in [from_shot, from_file] {
        assert_eq!(engine.project().clip(clip).unwrap().link, None);
    }
}

#[test]
fn a_refused_edit_changes_nothing() {
    let Fixture {
        mut engine,
        shot,
        silent,
        sound,
        v1,
        a1,
    } = fixture(|project, v1, a1, shot| {
        project
            .add_clip(v1, shot, tr(0, 100), rt(0))
            .expect("v1 bed");
        project
            .add_clip(a1, shot, tr(0, 100), rt(0))
            .expect("a1 bed");
    });
    let before = snapshot(&engine);

    use SoundPlacement::{InPicture, Lane, Off};
    let refused: [(&str, MediaId, Option<i64>, Option<TrackId>, SoundPlacement); 8] = [
        ("no lane patched", shot, None, None, Off),
        (
            "a sound kept in a picture left out",
            shot,
            None,
            None,
            InPicture,
        ),
        ("a sound file's picture", sound, None, Some(v1), Lane(a1)),
        ("a silent shot's sound", silent, None, Some(v1), Lane(a1)),
        ("the picture on a sound lane", shot, None, Some(a1), Off),
        ("the sound on a picture lane", shot, None, None, Lane(v1)),
        (
            "a sound on a lane that is gone",
            shot,
            None,
            Some(v1),
            Lane(TrackId::from_raw(9_999)),
        ),
        ("an empty span to fill", shot, Some(0), Some(v1), Lane(a1)),
    ];
    for (case, media, fill, track, sound) in refused {
        for ripple in [false, true] {
            let result = source_edit(
                &mut engine,
                media,
                tr(0, 24),
                40,
                fill,
                ripple,
                track,
                sound,
            );
            assert!(result.is_err(), "{case} (ripple {ripple}) is refused");
            assert_eq!(
                snapshot(&engine),
                before,
                "{case} (ripple {ripple}) leaves no trace"
            );
        }
    }
}

#[test]
fn one_undo_restores_both_lanes() {
    for ripple in [false, true] {
        let Fixture {
            mut engine,
            shot,
            v1,
            a1,
            ..
        } = fixture(|project, v1, a1, shot| {
            project
                .add_clip(v1, shot, tr(0, 100), rt(0))
                .expect("v1 bed");
            project
                .add_clip(a1, shot, tr(0, 100), rt(0))
                .expect("a1 bed");
        });
        let before = snapshot(&engine);

        source_edit(
            &mut engine,
            shot,
            tr(200, 30),
            40,
            Some(20),
            ripple,
            Some(v1),
            SoundPlacement::Lane(a1),
        )
        .expect("edit");
        let after = snapshot(&engine);
        assert_ne!(after, before);

        assert!(engine.undo(), "one command is one undo entry");
        assert_eq!(snapshot(&engine), before, "ripple {ripple}: undo");
        assert!(!engine.undo(), "the fixture itself added no history");
        assert!(engine.redo());
        assert_eq!(snapshot(&engine), after, "ripple {ripple}: redo");
    }
}

#[test]
fn a_patch_is_one_undo_step_and_refuses_the_wrong_lane() {
    let Fixture {
        mut engine, v1, a1, ..
    } = empty();
    let patch = |engine: &mut Engine, channel, patch| {
        engine.apply(Command::Edit(EditCommand::SetSourcePatch {
            channel,
            patch,
        }))
    };

    let outcome = patch(&mut engine, SourceChannel::Audio, Patch::Track(a1)).expect("patch");
    assert_eq!(
        outcome,
        ApplyOutcome::Edited(EditOutcome::UpdatedSourcePatch)
    );
    patch(&mut engine, SourceChannel::Video, Patch::Off).expect("patch off");
    let patching = engine.project().timeline().source_patching();
    assert_eq!(patching.get(SourceChannel::Audio), Patch::Track(a1));
    assert_eq!(patching.get(SourceChannel::Video), Patch::Off);

    assert!(patch(&mut engine, SourceChannel::Video, Patch::Track(a1)).is_err());
    assert!(patch(&mut engine, SourceChannel::Audio, Patch::Track(v1)).is_err());
    assert_eq!(engine.project().timeline().source_patching(), patching);

    assert!(engine.undo());
    assert_eq!(
        engine
            .project()
            .timeline()
            .source_patching()
            .get(SourceChannel::Video),
        Patch::Auto
    );
    assert!(engine.undo());
    assert!(engine.project().timeline().source_patching().is_default());
}
