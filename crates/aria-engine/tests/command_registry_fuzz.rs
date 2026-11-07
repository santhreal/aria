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

/// Where a project came from, which sets two rules: an edit never makes an
/// empty clip or an overlap, and a file may hold either (the model's layout
/// tests pin that both load; an edit separates an overlap).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Made {
    ByEdit,
    FromFile,
}

/// The rules every sequence holds, for a project `made` as stated.
fn assert_timeline_invariants(timeline: &Timeline, made: Made) {
    let shortest = i64::from(made == Made::ByEdit);
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
                clip.timeline.duration.value >= shortest,
                "clip duration {} under {shortest} on clip {:?}",
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
            assert_eq!(
                end.value - clip.timeline.start.value,
                clip.timeline.duration.value
            );
            assert!(
                clip.speed.is_valid(),
                "invalid clip speed {:?} on {:?}",
                clip.speed,
                clip.id
            );
            clip.transform.validate().expect("valid clip transform");
            clip.crop.validate_shape().expect("valid clip crop");
        }
        for pair in clips.windows(2).filter(|_| made == Made::ByEdit) {
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
    assert_sequence_invariants(project, Made::ByEdit);
}

fn assert_sequence_invariants(project: &Project, made: Made) {
    assert_timeline_invariants(project.timeline(), made);
    assert_timeline_invariants(project.root_timeline(), made);
    for seq in project.sequences() {
        assert_timeline_invariants(&seq.timeline, made);
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

/// The project as its saved text: the state undo, redo and a refusal are
/// compared by.
fn saved(engine: &Engine, id: &str) -> String {
    engine
        .project()
        .save_to_string()
        .unwrap_or_else(|err| panic!("{id} left a project that does not save: {err}"))
}

/// The first line where two saved projects differ, with its neighbours, so a
/// failure prints the field that changed instead of two whole files.
fn first_difference(left: &str, right: &str) -> String {
    let (l, r): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
    let at = l
        .iter()
        .zip(&r)
        .position(|(a, b)| a != b)
        .unwrap_or(l.len().min(r.len()));
    let from = at.saturating_sub(3);
    format!(
        "line {}:\n  expected: {:?}\n  found:    {:?}",
        at + 1,
        l.get(from..(at + 3).min(l.len())).unwrap_or_default(),
        r.get(from..(at + 3).min(r.len())).unwrap_or_default(),
    )
}

/// Applies `cmd` and checks the state it leaves: an edit undoes to the saved
/// text it started from and redoes to the text it made, a refusal changes
/// nothing, and every state saves, opens, and saves again to the same text.
fn dispatch_and_check(engine: &mut Engine, cmd: EditCommand) -> bool {
    let spec = cmd.spec();
    let before = saved(engine, spec.id);
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
    let after = saved(engine, spec.id);
    if applied {
        assert!(engine.undo(), "{} applied but does not undo", spec.id);
        let undone = saved(engine, spec.id);
        assert!(
            undone == before,
            "undo of {} left a project other than the one it edited, {}",
            spec.id,
            first_difference(&before, &undone)
        );
        assert!(engine.redo(), "{} undid but does not redo", spec.id);
        let redone = saved(engine, spec.id);
        assert!(
            redone == after,
            "redo of {} left a project other than the one it made, {}",
            spec.id,
            first_difference(&after, &redone)
        );
    } else {
        assert!(
            after == before,
            "{} was refused but changed the project, {}",
            spec.id,
            first_difference(&before, &after)
        );
    }
    // Every state an edit leaves has to open again, as itself: a value the
    // save writes and the load rejects, such as a non-finite float written as
    // `null`, leaves a project that cannot be reopened, and one the load reads
    // differently changes on the next save.
    let reopened = Project::load_from_string(&after)
        .unwrap_or_else(|err| {
            panic!(
                "{} left a project that saves but does not open: {err}",
                spec.id
            )
        })
        .save_to_string()
        .unwrap_or_else(|err| {
            panic!(
                "{} reopened to a project that does not save: {err}",
                spec.id
            )
        });
    assert!(
        reopened == after,
        "{} left a project that reopens as another, {}",
        spec.id,
        first_difference(&after, &reopened)
    );
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

/// The JSON pointer of every value under `value`, `at` included.
fn pointers(value: &serde_json::Value, at: String, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let key = key.replace('~', "~0").replace('/', "~1");
                pointers(child, format!("{at}/{key}"), out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                pointers(child, format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
    out.push(at);
}

/// The message of a caught panic.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

/// The rule a hand-edited or damaged file breaks, or `None` when it opens as
/// a project that keeps the rules a file may hold and saves and reopens as
/// itself, or is refused with a message, within the command time budget.
fn load_fault(text: &str) -> Option<String> {
    let t0 = Instant::now();
    let loaded = Project::load_from_string(text);
    if t0.elapsed() >= MAX_COMMAND_TIME {
        return Some(format!("loading took {:?}", t0.elapsed()));
    }
    let project = match loaded {
        Ok(project) => project,
        Err(err) if err.to_string().trim().is_empty() => {
            return Some("refused with an empty message".to_owned());
        }
        Err(_) => return None,
    };
    assert_sequence_invariants(&project, Made::FromFile);
    let saved = match project.save_to_string() {
        Ok(saved) => saved,
        Err(err) => return Some(format!("opened but does not save: {err}")),
    };
    let reopened = match Project::load_from_string(&saved) {
        Ok(reopened) => reopened,
        Err(err) => {
            return Some(format!(
                "opened and saved, and the save does not open: {err}"
            ));
        }
    };
    let again = reopened.save_to_string().expect("a reopened project saves");
    (again != saved).then(|| {
        format!(
            "reopens as another project, {}",
            first_difference(&saved, &again)
        )
    })
}

/// WHY: the loader is the one entry for a file from disk, and a file edited
/// by hand, by a script, or cut short holds values no edit writes. Every value
/// of a project the fuzzer built is replaced in turn by each hostile value,
/// and every key and element is removed in turn; each file must open as a
/// project that saves and reopens as itself, or be refused. Values are drawn
/// from the saved file at run time, so a field a new version adds is in the
/// census. Does not catch a wrong value the loader accepts as valid, or a
/// field the fuzzed project leaves at its default, which a save omits.
#[test]
fn every_value_of_a_project_file_replaced_or_removed_opens_or_is_refused() {
    let mut rng = CommandRng::new(7);
    let (mut engine, _v1, _a1, _c1, _c2) = build_seeded_engine();
    for _ in 0..60 {
        let cmd = random_edit_command(&mut rng, engine.project());
        let _ = engine.apply(Command::Edit(cmd));
    }
    let text = saved(&engine, "the fuzzed project");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("a saved project is JSON");
    let hostile = [
        serde_json::Value::Null,
        serde_json::json!(true),
        serde_json::json!(-1),
        serde_json::json!(0),
        serde_json::json!(i64::MAX),
        serde_json::json!(u64::MAX),
        serde_json::json!(-1.0e308),
        serde_json::json!(0.5),
        serde_json::json!(""),
        serde_json::json!([]),
        serde_json::json!({}),
    ];
    let mut all = Vec::new();
    pointers(&doc, String::new(), &mut all);
    assert!(
        all.len() > 200,
        "the fuzzed project holds {} values",
        all.len()
    );
    let mut faults = Vec::new();
    let mut check = |edited: &serde_json::Value, what: String| {
        let text = edited.to_string();
        let fault = std::panic::catch_unwind(|| load_fault(&text))
            .unwrap_or_else(|payload| Some(format!("panicked: {}", panic_text(&*payload))));
        if let Some(fault) = fault {
            faults.push(format!("{what}: {fault}"));
        }
    };
    for pointer in all.iter().filter(|p| !p.is_empty()) {
        for value in &hostile {
            let mut edited = doc.clone();
            *edited.pointer_mut(pointer).expect("a collected pointer") = value.clone();
            check(&edited, format!("{pointer} = {value}"));
        }
        let (parent, last) = pointer.rsplit_once('/').expect("a pointer below the root");
        let mut edited = doc.clone();
        match edited.pointer_mut(parent).expect("a collected parent") {
            serde_json::Value::Object(map) => {
                map.remove(&last.replace("~1", "/").replace("~0", "~"));
            }
            serde_json::Value::Array(items) => {
                items.remove(last.parse::<usize>().expect("an array index"));
            }
            _ => unreachable!("a parent holds children"),
        }
        check(&edited, format!("{pointer} removed"));
    }
    assert!(
        faults.is_empty(),
        "{} damaged files break a rule; the first 60:\n{}",
        faults.len(),
        faults[..faults.len().min(60)].join("\n")
    );
}

#[path = "command_payload_census.rs"]
mod payload;
