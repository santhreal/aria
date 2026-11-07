//! WHY: Closes the class of undo/redo state drift, lost transition junctions,
//! unrefreshed track ordering on clip restoration, and save/load persistence
//! non-idempotence across `Engine` edit sequences. Uses the shared seeded
//! `aria_commands::fuzz` generator across every `edit_specs()` command variant
//! and multi-step edit chains to assert:
//! (a) `save_to_file -> load_from_file -> save_to_file` is byte-identical,
//! (b) `apply -> undo -> redo` and full-history walk-back/walk-forward restore
//!     the exact `Project` and serialized `.aria` JSON bytes, and
//! (c) model invariants hold after every edit, undo, redo, and reload.
//! Does not cover GPU rendering or container decoding.

mod common;

use aria_commands::{
    Command, CommandRng, EditCommand, ParentKeep, edit_specs, every_fuzzed_edit_command,
    random_edit_command,
};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    ClipSource, Generator, MediaSource, MulticamAngle, Project, Rational, Timeline, TrackId,
    TrackKind,
};
use common::{import_asset, load_project, rt, small_video_asset, temp_engine, tr};

#[test]
fn undo_import_removes_media() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    assert!(engine.project().media(media_id).is_some());

    assert!(engine.undo());
    assert!(engine.project().media(media_id).is_none());

    assert!(engine.redo());
    assert!(engine.project().media(media_id).is_some());
}

#[test]
fn undo_import_after_clip_removed() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media: media_id,
            source: tr(0, 48),
            start: rt(0),
        }))
        .expect("add clip");

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().clip_count(), 0);

    assert!(engine.undo());
    assert_eq!(engine.project().timeline().track_count(), 0);

    assert!(engine.undo());
    assert!(engine.project().media(media_id).is_none());
}

fn assert_timeline_invariants(project: &Project, timeline: &Timeline) {
    assert!(timeline.frame_rate.is_valid());
    let mut total_clips = 0usize;
    // The catalog's rank, not a copy of it: a non-main video lane shares the
    // stack zone with sticker, effect, filter and adjustment lanes, so any of
    // them may sit below one.
    let mut prev_zone = 0u8;
    let mut video_tracks = Vec::new();
    let mut main_tracks = Vec::new();

    for &track_id in timeline.order() {
        let track = timeline.track(track_id).expect("ordered track exists");
        let zone = track.kind.spec().zone_rank(track.main);
        assert!(
            zone >= prev_zone,
            "lane zone order violated on {track_id:?}"
        );
        prev_zone = zone;
        if track.kind == TrackKind::Video {
            video_tracks.push(track_id);
        }
        if track.main {
            main_tracks.push(track_id);
        }

        let clips: Vec<_> = track.clips_ordered().collect();
        assert_eq!(clips.len(), track.len());
        total_clips += clips.len();
        for clip in &clips {
            assert_eq!(timeline.track_of(clip.id), Some(track_id));
            assert!(track.kind.accepts_clip(clip));
            assert!(clip.timeline.start.value >= 0);
            assert!(clip.timeline.duration.value > 0);
            assert_eq!(clip.timeline.start.rate, timeline.frame_rate);
            assert_eq!(clip.timeline.duration.rate, timeline.frame_rate);
            clip.timeline.end().expect("clip timeline end valid");
            if let ClipSource::Media { media, source } = &clip.content {
                let asset = project.media(*media).expect("media exists");
                let src_end = source.end().expect("source end valid").value;
                if !asset.is_image && !clip.freeze_frame {
                    assert!(src_end <= asset.duration.value);
                }
            }
        }
        for pair in clips.windows(2) {
            let left_end = pair[0].timeline.end().unwrap().value;
            let right_start = pair[1].timeline.start.value;
            assert!(left_end <= right_start);
        }
        for t in track.transitions() {
            assert!(t.duration > 0);
            let l = track.clip(t.left).expect("transition left exists");
            let r = track.clip(t.right).expect("transition right exists");
            assert_eq!(l.timeline.end_tick(), r.timeline.start.value);
            assert!(
                t.duration
                    <= t.alignment
                        .junction_limit(l.timeline.duration.value, r.timeline.duration.value)
            );
        }
    }
    assert_eq!(total_clips, timeline.clip_count());
    if video_tracks.is_empty() {
        assert!(main_tracks.is_empty());
    } else {
        assert_eq!(main_tracks, vec![video_tracks[0]]);
    }
}

fn assert_project_invariants(project: &Project) {
    assert_timeline_invariants(project, project.timeline());
    for seq in project.sequences() {
        assert_timeline_invariants(project, &seq.timeline);
        for clip in seq
            .timeline
            .tracks_ordered()
            .flat_map(|t| t.clips_ordered())
        {
            if let Some(child_seq) = clip.sequence() {
                assert!(!project.would_create_sequence_cycle(Some(seq.id), child_seq));
            }
        }
    }
}

fn assert_save_load_idempotent(project: &Project, dir: &std::path::Path) {
    let path_a = dir.join("engine_a.aria");
    let path_b = dir.join("engine_b.aria");
    project.save_to_file(&path_a).expect("first save");
    let loaded = Project::load_from_file(&path_a).expect("load");
    assert_project_invariants(&loaded);
    loaded.save_to_file(&path_b).expect("second save");
    assert_eq!(
        std::fs::read(&path_a).unwrap(),
        std::fs::read(&path_b).unwrap(),
        "save -> load -> save bytes drifted"
    );
}

fn seeded_engine(dir: &std::path::Path) -> (Engine, TrackId) {
    let mut seed_project = Project::new("seed", Rational::FPS_24);
    let v_media = seed_project.add_media(MediaSource::new(
        "/synthetic/v.mp4",
        1920,
        1080,
        Rational::FPS_24,
        360,
        true,
    ));
    let a_media = seed_project.add_media(MediaSource::new(
        "/synthetic/a.wav",
        0,
        0,
        Rational::FPS_24,
        360,
        true,
    ));
    let seed_path = dir.join("seed_init.aria");
    seed_project.save_to_file(&seed_path).expect("save seed");

    let mut engine = Engine::new(EngineConfig { undo_limit: 512 }).expect("engine");
    load_project(&mut engine, &seed_path);

    let v1 = common::add_track(&mut engine, TrackKind::Video, "V1");
    let v2 = common::add_track(&mut engine, TrackKind::Video, "V2");
    let a1 = common::add_track(&mut engine, TrackKind::Audio, "A1");
    let t1 = common::add_track(&mut engine, TrackKind::Text, "T1");
    let bus = common::add_track(&mut engine, TrackKind::Submix, "Bus");

    let c_v1_a = common::add_media_clip(&mut engine, v1, v_media, tr(0, 72), rt(0));
    let _c_v1_b = common::add_media_clip(&mut engine, v1, v_media, tr(72, 72), rt(72));
    let _c_v2 = common::add_media_clip(&mut engine, v2, v_media, tr(0, 48), rt(24));
    let c_a1 = common::add_media_clip(&mut engine, a1, a_media, tr(0, 72), rt(0));
    let c_t1 = common::add_generated(&mut engine, t1, Generator::text("Card"), tr(0, 48));

    engine
        .apply(Command::Edit(EditCommand::LinkClips {
            clips: vec![c_v1_a, c_a1],
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::SetClipParent {
            clip: c_t1,
            parent: Some(c_v1_a),
            at: rt(0),
            keep: ParentKeep::Place,
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: c_v1_a,
            transition_id: "crossfade".into(),
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::SetTrackOutput {
            track: a1,
            to: Some(bus),
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::AddSend {
            track: v1,
            to: bus,
            level: 1.0,
            pre_fader: false,
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::CreateBin {
            name: "Footage".into(),
            parent: None,
        }))
        .unwrap();
    engine
        .apply(Command::Edit(EditCommand::CreateMulticamGroup {
            name: "Cams".into(),
            angles: vec![MulticamAngle::new(v_media, 0)],
        }))
        .unwrap();
    (engine, v1)
}

#[test]
fn generative_edit_sequences_preserve_undo_redo_exact_bytes_and_save_load_idempotence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec_count = edit_specs().len();

    for seed in 0..4u64 {
        let mut rng = CommandRng::new(seed + 100);
        let (mut engine, _) = seeded_engine(dir.path());
        assert_project_invariants(engine.project());
        assert_save_load_idempotent(engine.project(), dir.path());

        let mut history: Vec<String> = vec![engine.project().save_to_string().unwrap()];

        let mut commands = every_fuzzed_edit_command(&mut rng, engine.project());
        assert_eq!(commands.len(), spec_count);
        for _ in 0..48 {
            commands.push(random_edit_command(&mut rng, engine.project()));
        }

        for cmd in commands {
            let id = cmd.spec().id;
            let before_json = history.last().unwrap().clone();
            match engine.apply(Command::Edit(cmd)) {
                Ok(outcome) => {
                    assert!(matches!(outcome, ApplyOutcome::Edited(_)));
                    assert_project_invariants(engine.project());
                    let after_json = engine.project().save_to_string().unwrap();

                    // Immediate undo -> redo exact restoration check.
                    assert!(engine.undo(), "undo failed after {id}");
                    assert_project_invariants(engine.project());
                    assert_eq!(
                        engine.project().save_to_string().unwrap(),
                        before_json,
                        "undo JSON drifted after {id} (seed {seed})"
                    );

                    assert!(engine.redo(), "redo failed after {id}");
                    assert_project_invariants(engine.project());
                    assert_eq!(
                        engine.project().save_to_string().unwrap(),
                        after_json,
                        "redo JSON drifted after {id} (seed {seed})"
                    );

                    history.push(after_json);
                }
                Err(_) => {
                    assert_project_invariants(engine.project());
                    assert_eq!(
                        engine.project().save_to_string().unwrap(),
                        before_json,
                        "refused {id} mutated project JSON (seed {seed})"
                    );
                }
            }
        }

        assert_save_load_idempotent(engine.project(), dir.path());

        // Walk undo all the way back to the seeded state, checking every step.
        for expected_json in history.iter().rev().skip(1) {
            assert!(engine.undo());
            assert_project_invariants(engine.project());
            assert_eq!(&engine.project().save_to_string().unwrap(), expected_json);
        }
        // Walk redo all the way back to the tip, checking every step.
        for expected_json in history.iter().skip(1) {
            assert!(engine.redo());
            assert_project_invariants(engine.project());
            assert_eq!(&engine.project().save_to_string().unwrap(), expected_json);
        }
    }
}

#[test]
fn undo_redo_restores_transition_pruned_when_abutting_clip_shrinks_below_junction_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut engine, v1) = seeded_engine(dir.path());
    let clips: Vec<_> = engine
        .project()
        .timeline()
        .track(v1)
        .unwrap()
        .clips_ordered()
        .map(|c| c.id)
        .collect();
    let (left, right) = (clips[0], clips[1]);
    assert!(engine.project().transition_at(left).is_some());
    let before_json = engine.project().save_to_string().unwrap();

    // Shrink `right` (`[72, 144)`) to `[72, 80)` (8 ticks < 24-tick transition):
    // `left` and `right` still abut at tick 72, yet `junction_limit(72, 8) == 8 < 24`,
    // so the transition is pruned on apply, restored on undo, and re-pruned on redo.
    engine
        .apply(Command::Edit(EditCommand::TrimClip {
            clip: right,
            timeline: tr(72, 8),
        }))
        .unwrap();
    assert!(engine.project().transition_at(left).is_none());
    assert_project_invariants(engine.project());
    let after_json = engine.project().save_to_string().unwrap();

    assert!(engine.undo());
    assert_eq!(engine.project().transition_at(left).unwrap().duration, 24);
    assert_eq!(engine.project().save_to_string().unwrap(), before_json);
    assert_project_invariants(engine.project());

    assert!(engine.redo());
    assert!(engine.project().transition_at(left).is_none());
    assert_eq!(engine.project().save_to_string().unwrap(), after_json);
    assert_project_invariants(engine.project());
}
