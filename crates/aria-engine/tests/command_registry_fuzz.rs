//! WHY: Closes the class of edit-engine panics, arithmetic overflows, partial
//! state mutations on refused edits, silent no-ops, and undo/redo invariant
//! breaks across `EditCommand` dispatch. Drives every `EditCommand` variant in
//! `edit_specs()` at run time through `Engine::apply` with seeded valid and
//! hostile inputs (`i64::MIN`/`i64::MAX`, negative ticks, zero/negative
//! durations, invalid rates, stale ids, locked tracks, non-finite floats).
//! Does not catch GPU shader compilation or real-file container demuxing.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use aria_commands::{
    Command, CommandRng, EditCommand, EditOutcome, edit_specs, every_fuzzed_edit_command,
    random_edit_command,
};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    ClipId, Generator, MediaId, MediaSource, MulticamAngle, Project, Rational, RationalTime,
    TimeRange, Timeline, TrackId, TrackKind, TrackOutput,
};

const RATE: Rational = Rational::FPS_24;
const MAX_COMMAND_TIME: Duration = Duration::from_secs(2);

fn rt(ticks: i64) -> RationalTime {
    RationalTime::new(ticks, RATE)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, RATE)
}

fn add_track(engine: &mut Engine, kind: TrackKind, name: &str) -> TrackId {
    match engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind,
            name: name.into(),
            index: None,
            pinned: false,
        }))
        .expect("add track")
    {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => id,
        other => panic!("expected CreatedTrack, got {other:?}"),
    }
}

fn add_media_clip(
    engine: &mut Engine,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
) -> ClipId {
    match engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media,
            source,
            start: at,
        }))
        .expect("add media clip")
    {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

fn add_generated(
    engine: &mut Engine,
    track: TrackId,
    generator: Generator,
    timeline: TimeRange,
) -> ClipId {
    match engine
        .apply(Command::Edit(EditCommand::AddGenerated {
            track,
            generator,
            timeline,
        }))
        .expect("add generated clip")
    {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

fn build_seeded_engine() -> (Engine, TrackId, TrackId, ClipId, ClipId) {
    let mut project = Project::new("fuzz_project", RATE);
    let video_id = project.add_media(MediaSource::new(
        "/synthetic/clip_a.mp4",
        1920,
        1080,
        RATE,
        240,
        true,
    ));
    let audio_id = project.add_media(MediaSource::new(
        "/synthetic/audio_a.wav",
        0,
        0,
        RATE,
        240,
        true,
    ));
    let mut engine =
        Engine::with_project(EngineConfig { undo_limit: 256 }, project).expect("engine");

    let v1 = add_track(&mut engine, TrackKind::Video, "V1");
    let v2 = add_track(&mut engine, TrackKind::Video, "V2");
    let a1 = add_track(&mut engine, TrackKind::Audio, "A1");
    let _t1 = add_track(&mut engine, TrackKind::Text, "T1");
    let _fx = add_track(&mut engine, TrackKind::Adjustment, "FX");
    let _flt = add_track(&mut engine, TrackKind::Filter, "FLT");
    let _bus = add_track(&mut engine, TrackKind::Submix, "BUS");

    let clip_v1_a = add_media_clip(&mut engine, v1, video_id, tr(0, 48), rt(0));
    let clip_v1_b = add_media_clip(&mut engine, v1, video_id, tr(48, 48), rt(48));
    let _clip_v2 = add_media_clip(&mut engine, v2, video_id, tr(0, 48), rt(24));
    let clip_a1 = add_media_clip(&mut engine, a1, audio_id, tr(0, 48), rt(0));

    engine
        .apply(Command::Edit(EditCommand::LinkClips {
            clips: vec![clip_v1_a, clip_a1],
        }))
        .expect("link v1_a and a1");
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: clip_v1_a,
            transition_id: "dip_to_black".into(),
        }))
        .expect("add transition");
    engine
        .apply(Command::Edit(EditCommand::CreateBin {
            name: "Rushes".into(),
            parent: None,
        }))
        .expect("create bin");
    engine
        .apply(Command::Edit(EditCommand::AddMarker {
            at: rt(24),
            name: "Beat".into(),
            color: None,
        }))
        .expect("add marker");
    engine
        .apply(Command::Edit(EditCommand::CreateMulticamGroup {
            name: "Multicam".into(),
            angles: vec![MulticamAngle::new(video_id, 0)],
        }))
        .expect("create multicam");

    (engine, v1, a1, clip_v1_a, clip_v1_b)
}

fn assert_timeline_invariants(timeline: &Timeline) {
    assert!(
        timeline.frame_rate.is_valid(),
        "invalid timeline frame_rate: {:?}",
        timeline.frame_rate
    );
    let order = timeline.order();
    let mut seen_tracks = BTreeSet::new();
    for &track_id in order {
        assert!(
            seen_tracks.insert(track_id),
            "duplicate track {track_id:?} in timeline order"
        );
        let track = timeline.track(track_id).expect("ordered track exists");
        assert!(
            track.gain.is_finite() && track.gain >= 0.0,
            "invalid track gain {} on {track_id:?}",
            track.gain
        );
        assert!(
            track.pan.is_finite() && (-1.0..=1.0).contains(&track.pan),
            "invalid track pan {} on {track_id:?}",
            track.pan
        );
        if let TrackOutput::Bus(bus) = track.output {
            assert_ne!(bus, track_id, "track {track_id:?} routes output to itself");
        }
        for send in &track.sends {
            assert!(
                send.level.is_finite() && send.level >= 0.0,
                "invalid send level {} on {track_id:?}",
                send.level
            );
            assert_ne!(send.to, track_id, "track {track_id:?} has self-send");
        }

        let clips: Vec<_> = track.clips_ordered().collect();
        for clip in &clips {
            assert!(
                clip.timeline.start.value >= 0,
                "negative clip start {} on clip {:?}",
                clip.timeline.start.value,
                clip.id
            );
            assert!(
                clip.timeline.duration.value > 0,
                "non-positive clip duration {} on clip {:?}",
                clip.timeline.duration.value,
                clip.id
            );
            assert_eq!(
                clip.timeline.start.rate, timeline.frame_rate,
                "clip {:?} start rate mismatch",
                clip.id
            );
            assert_eq!(
                clip.timeline.duration.rate, timeline.frame_rate,
                "clip {:?} duration rate mismatch",
                clip.id
            );
            let end = clip
                .timeline
                .end()
                .expect("clip start + duration does not overflow");
            assert!(end.value > clip.timeline.start.value);
            assert!(
                clip.speed.is_valid(),
                "invalid clip speed {:?} on {:?}",
                clip.speed,
                clip.id
            );
            clip.transform.validate().expect("valid clip transform");
            clip.crop.validate_shape().expect("valid clip crop");
        }
        for pair in clips.windows(2) {
            let left_end = pair[0].timeline.end().expect("left end").value;
            let right_start = pair[1].timeline.start.value;
            assert!(
                left_end <= right_start,
                "overlapping clips {:?} (..{left_end}) and {:?} ({right_start}..) on {track_id:?}",
                pair[0].id,
                pair[1].id
            );
        }
        for transition in track.transitions() {
            assert!(transition.duration > 0, "non-positive transition duration");
            let left = track
                .clip(transition.left)
                .expect("transition left clip exists on track");
            let right = track
                .clip(transition.right)
                .expect("transition right clip exists on track");
            assert_eq!(
                left.timeline.end().expect("left end").value,
                right.timeline.start.value,
                "transition sits on non-abutting clips {:?} and {:?}",
                transition.left,
                transition.right
            );
        }
    }
}

fn assert_project_invariants(project: &Project) {
    assert_timeline_invariants(project.timeline());
    assert_timeline_invariants(project.root_timeline());
    for seq in project.sequences() {
        assert_timeline_invariants(&seq.timeline);
        for clip in seq
            .timeline
            .tracks_ordered()
            .flat_map(|t| t.clips_ordered())
        {
            if let Some(child_seq) = clip.sequence() {
                assert!(
                    !project.would_create_sequence_cycle(Some(seq.id), child_seq),
                    "sequence cycle detected on {:?} -> {:?}",
                    seq.id,
                    child_seq
                );
            }
        }
    }
    for bin in project.bin_iter() {
        if let Some(parent) = bin.parent {
            assert!(
                project.bin(parent).is_some(),
                "bin {:?} references missing parent {:?}",
                bin.id,
                parent
            );
        }
        assert!(
            !project.bin_ancestors(bin.id).any(|step| step == bin.id),
            "bin cycle detected on {:?}",
            bin.id
        );
    }
}

fn dispatch_and_check(engine: &mut Engine, cmd: EditCommand) -> bool {
    let spec = cmd.spec();
    let gen_before = engine.history_generation();
    let undo_before = engine.history_depth();
    let t0 = Instant::now();
    let result = engine.apply(Command::Edit(cmd));
    let elapsed = t0.elapsed();
    assert!(
        elapsed < MAX_COMMAND_TIME,
        "command {} exceeded time budget: {:?}",
        spec.id,
        elapsed
    );

    let applied = match result {
        Ok(outcome) => {
            assert!(
                matches!(outcome, ApplyOutcome::Edited(_)),
                "expected Edited outcome for {}, got {outcome:?}",
                spec.id
            );
            assert_eq!(
                engine.history_generation(),
                gen_before + 1,
                "{} returned Ok without recording a history entry (silent no-op)",
                spec.id
            );
            true
        }
        Err(err) => {
            let msg = err.to_string();
            assert!(
                !msg.trim().is_empty(),
                "{} failed with an empty error message",
                spec.id
            );
            assert!(
                !err.kind().is_empty(),
                "{} failed with an empty error kind",
                spec.id
            );
            assert_eq!(
                engine.history_generation(),
                gen_before,
                "{} advanced history_generation on refusal: {msg}",
                spec.id
            );
            assert_eq!(
                engine.history_depth(),
                undo_before,
                "{} pushed an undo entry on refusal: {msg}",
                spec.id
            );
            false
        }
    };

    assert_project_invariants(engine.project());
    applied
}

#[test]
fn every_edit_command_variant_is_exercised_under_hostile_and_seeded_inputs() {
    let expected: BTreeSet<&'static str> = edit_specs().iter().map(|s| s.id).collect();
    let mut exercised: BTreeSet<&'static str> = BTreeSet::new();

    for seed in 0..12u64 {
        let mut rng = CommandRng::new(seed);
        let (mut engine, _v1, _a1, _c1, _c2) = build_seeded_engine();

        // Phase A: normal + hostile fuzzed sweep across every variant.
        for cmd in every_fuzzed_edit_command(&mut rng, engine.project()) {
            exercised.insert(cmd.spec().id);
            let applied = dispatch_and_check(&mut engine, cmd);
            if applied && rng.next_bool() {
                assert!(engine.undo());
                assert_project_invariants(engine.project());
                assert!(engine.redo());
                assert_project_invariants(engine.project());
            }
        }

        // Phase B: lock every track on the active timeline and sweep again so
        // every guarded command's locked-lane refusal path is exercised.
        let track_ids: Vec<TrackId> = engine.project().timeline().order().to_vec();
        for track in track_ids {
            let _ = engine.apply(Command::Edit(EditCommand::SetTrackLocked {
                track,
                locked: true,
            }));
        }
        for cmd in every_fuzzed_edit_command(&mut rng, engine.project()) {
            dispatch_and_check(&mut engine, cmd);
        }
    }

    assert_eq!(
        exercised, expected,
        "fuzz suite did not exercise every EditCommand variant in edit_specs()"
    );
}

#[test]
fn random_edit_command_sequences_preserve_invariants_and_undo_redo() {
    for seed in 100..112u64 {
        let mut rng = CommandRng::new(seed);
        let (mut engine, _v1, _a1, _c1, _c2) = build_seeded_engine();

        for step in 0..120usize {
            let cmd = random_edit_command(&mut rng, engine.project());
            let applied = dispatch_and_check(&mut engine, cmd);

            if applied && step % 5 == 0 {
                assert!(engine.undo(), "undo failed at seed {seed} step {step}");
                assert_project_invariants(engine.project());
                assert!(engine.redo(), "redo failed at seed {seed} step {step}");
                assert_project_invariants(engine.project());
            }
            if step % 17 == 0 && engine.can_undo() {
                assert!(engine.undo());
                assert_project_invariants(engine.project());
            }
            if step % 23 == 0 && engine.can_redo() {
                assert!(engine.redo());
                assert_project_invariants(engine.project());
            }
        }

        // Walk the remaining undo stack all the way back and forward.
        let mut undone = 0usize;
        while engine.can_undo() && undone < 32 {
            assert!(engine.undo());
            assert_project_invariants(engine.project());
            undone += 1;
        }
        while engine.can_redo() && undone > 0 {
            assert!(engine.redo());
            assert_project_invariants(engine.project());
            undone -= 1;
        }
    }
}

#[test]
fn shift_clips_rejects_i64_min_delta_and_end_tick_overflow_atomically() {
    let mut engine = Engine::with_project(
        EngineConfig { undo_limit: 64 },
        Project::new("shift_overflow", RATE),
    )
    .expect("engine");
    let track = add_track(&mut engine, TrackKind::Adjustment, "FX");
    // Empty track with delta == i64::MIN must not panic when negating delta for the inverse.
    let err = engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(0),
            delta: rt(i64::MIN),
        }))
        .expect_err("i64::MIN delta must be rejected");
    assert!(!err.to_string().is_empty());

    // Two clips where clip 1's start + delta fits i64, but clip 1's end_tick or clip 2's start + delta overflows.
    let c1 = add_generated(&mut engine, track, Generator::Adjustment, tr(10, 30));
    let c2 = add_generated(&mut engine, track, Generator::Adjustment, tr(100, 20));

    let err = engine
        .apply(Command::Edit(EditCommand::ShiftClips {
            track,
            from: rt(0),
            delta: rt(i64::MAX - 20),
        }))
        .expect_err("shift overflowing end_tick must be rejected");
    assert!(!err.to_string().is_empty());
    assert_eq!(
        engine.project().clip(c1).expect("c1").start().value,
        10,
        "c1 must remain unshifted when shift is rejected"
    );
    assert_eq!(
        engine.project().clip(c2).expect("c2").start().value,
        100,
        "c2 must remain unshifted when shift is rejected"
    );
    assert_project_invariants(engine.project());
}

#[test]
fn move_overwrite_and_ripple_move_reject_extreme_ticks_without_overflow() {
    let (mut engine, v1, a1, clip_v1_a, clip_v1_b) = build_seeded_engine();
    let video_id = engine
        .project()
        .clip(clip_v1_a)
        .and_then(|c| c.media())
        .expect("video media");

    let invalid_dur_source = TimeRange::at_rate(0, 24, Rational::new(0, 1));
    let hostile_commands = [
        EditCommand::MoveClip {
            clip: clip_v1_a,
            to_track: v1,
            start: rt(i64::MAX),
        },
        EditCommand::MoveClip {
            clip: clip_v1_b,
            to_track: v1,
            start: rt(i64::MIN),
        },
        EditCommand::OverwriteMoveClip {
            clip: clip_v1_a,
            to_track: v1,
            start: rt(i64::MAX),
        },
        EditCommand::OverwriteMoveClip {
            clip: clip_v1_b,
            to_track: v1,
            start: rt(i64::MIN),
        },
        EditCommand::RippleMoveClip {
            clip: clip_v1_a,
            to_track: v1,
            start: rt(i64::MAX),
        },
        EditCommand::RippleMoveClip {
            clip: clip_v1_b,
            to_track: v1,
            start: rt(i64::MIN),
        },
        EditCommand::OverwriteClip {
            track: v1,
            media: video_id,
            source: tr(0, 24),
            at: rt(i64::MAX),
        },
        EditCommand::OverwriteClip {
            track: v1,
            media: video_id,
            source: invalid_dur_source,
            at: rt(0),
        },
        EditCommand::RippleInsert {
            track: v1,
            media: video_id,
            source: tr(0, 24),
            at: rt(i64::MAX),
        },
        EditCommand::RippleInsert {
            track: v1,
            media: video_id,
            source: invalid_dur_source,
            at: rt(0),
        },
        EditCommand::FitToFill {
            track: v1,
            media: video_id,
            source: tr(0, 48),
            at: rt(i64::MAX - 10),
            duration: rt(24),
            ripple: false,
        },
        EditCommand::SourceEdit {
            media: video_id,
            source: invalid_dur_source,
            at: rt(0),
            fill: None,
            ripple: false,
            track: Some(v1),
            sound: aria_models::SoundPlacement::Off,
        },
        EditCommand::TrimClip {
            clip: clip_v1_b,
            timeline: tr(i64::MIN, i64::MAX),
        },
    ];

    for cmd in hostile_commands {
        let gen_before = engine.history_generation();
        let err = engine
            .apply(Command::Edit(cmd.clone()))
            .expect_err("extreme tick command must be rejected");
        assert!(!err.to_string().is_empty());
        assert_eq!(engine.history_generation(), gen_before);
        assert!(engine.project().clip(clip_v1_a).is_some());
        assert!(engine.project().clip(clip_v1_b).is_some());
        assert_eq!(engine.project().timeline().track_of(clip_v1_a), Some(v1));
        assert!(engine.project().timeline().track(a1).is_some());
        assert_project_invariants(engine.project());
    }
}

#[test]
fn create_nested_sequence_deduplicates_ids_and_refuses_unselected_gap_overlap_atomically() {
    let (mut engine, v1, _a1, clip_a, clip_b) = build_seeded_engine();
    let video_id = engine
        .project()
        .clip(clip_a)
        .and_then(|c| c.media())
        .expect("video media");
    let clip_c = add_media_clip(&mut engine, v1, video_id, tr(0, 24), rt(96));
    // Selecting [clip_a, clip_c] spans [0, 120) across unselected clip_b [48, 96).
    // Must refuse with Overlap before mutating sequences or removing clip_a / clip_c.
    let gen_before = engine.history_generation();
    let err = engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Gapped".into(),
            clips: vec![clip_a, clip_c],
        }))
        .expect_err("nesting non-contiguous clips across an unselected middle clip must fail");
    assert!(!err.to_string().is_empty());
    assert_eq!(engine.history_generation(), gen_before);
    assert_eq!(engine.project().sequence_count(), 0);
    assert!(engine.project().clip(clip_a).is_some());
    assert!(engine.project().clip(clip_b).is_some());
    assert!(engine.project().clip(clip_c).is_some());

    // Duplicate clip IDs [clip_c, clip_c] must deduplicate cleanly and round-trip undo/redo.
    let outcome = engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "DedupNest".into(),
            clips: vec![clip_c, clip_c],
        }))
        .expect("duplicate clip IDs in selection should deduplicate");
    let ApplyOutcome::Edited(EditOutcome::Created(nested_id)) = outcome else {
        panic!("expected Created outcome, got {outcome:?}");
    };
    assert!(engine.project().timeline().clip(nested_id).is_some());
    assert!(engine.project().timeline().clip(clip_c).is_none());
    assert_eq!(engine.project().sequence_count(), 1);
    assert_project_invariants(engine.project());

    assert!(engine.undo());
    assert!(engine.project().timeline().clip(clip_c).is_some());
    assert_eq!(engine.project().sequence_count(), 0);
    assert_project_invariants(engine.project());

    assert!(engine.redo());
    assert!(engine.project().timeline().clip(nested_id).is_some());
    assert!(engine.project().timeline().clip(clip_c).is_none());
    assert_eq!(engine.project().sequence_count(), 1);
    assert_project_invariants(engine.project());
}

#[test]
fn set_send_with_no_fields_still_validates_send_index() {
    let (mut engine, _v1, a1, _c1, _c2) = build_seeded_engine();
    let gen_before = engine.history_generation();
    let err = engine
        .apply(Command::Edit(EditCommand::SetSend {
            track: a1,
            index: 99,
            level: None,
            pre_fader: None,
        }))
        .expect_err("out-of-range send index with empty patch must fail");
    assert!(!err.to_string().is_empty());
    assert_eq!(engine.history_generation(), gen_before);
}

#[test]
fn set_sequence_rate_rejects_collapsing_or_overflowing_rate_atomically() {
    let (mut engine, _v1, _a1, clip_v1_a, clip_v1_b) = build_seeded_engine();
    let gen_before = engine.history_generation();
    // At 1/1000 fps, [0, 48) and [48, 96) both round to tick 0 and clamp to [0, 1),
    // which would leave two overlapping clips on V1 if not refused and rolled back.
    let err = engine
        .apply(Command::Edit(EditCommand::SetSequenceRate {
            frame_rate: Rational::new(1, 1000),
        }))
        .expect_err("rate that collapses adjacent clips into an overlap must fail");
    assert!(!err.to_string().is_empty());
    assert_eq!(engine.history_generation(), gen_before);
    assert_eq!(engine.project().timeline().frame_rate, RATE);
    assert_eq!(
        engine.project().clip(clip_v1_a).expect("a").timeline,
        tr(0, 48)
    );
    assert_eq!(
        engine.project().clip(clip_v1_b).expect("b").timeline,
        tr(48, 48)
    );
    assert_project_invariants(engine.project());
}
