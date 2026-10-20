//! Defect class: Nested sequence creation, placement, navigation, cycle prevention, and render resolution.
//!
//! A sequence placed on a timeline renders as a composite clip referencing a child timeline.
//! Nested sequences support full undo/redo, recursive scene resolution, project serialization
//! roundtrips, and strict cycle detection preventing direct and transitive self-containment.
//!
//! A nested sequence clip is media-bearing, so only a lane that accepts media
//! accepts one. Nesting a selection that sits on a lane which refuses it is a
//! refusal that must leave the project exactly as it was: the check runs before
//! the sequence is registered and the selection removed.

mod common;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_engine::{ApplyOutcome, Engine, EngineConfig, EngineError};
use aria_models::{
    ClipId, ClipSource, Generator, MediaId, MediaSource, ModelError, Project, Rational, TrackId,
    TrackKind,
};
use aria_render::{LayerSource, ResolveOverrides, resolve_with};
use common::{add_generated, add_media_clip, add_track, rt, tr};

/// An engine over a project that already holds one media source, one video
/// lane, and two abutting-but-separated clips on it.
///
/// The pool and the source clips are fixture setup, not the behaviour under
/// test, so they are built on a `Project` and the engine is opened around it: a
/// live engine exposes no mutable project, because every change to one is a
/// command.
fn engine_with_two_clips() -> (Engine, MediaId, TrackId, ClipId, ClipId) {
    let mut project = Project::new("nested", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/media/take.mp4",
        1920,
        1080,
        Rational::FPS_24,
        480,
        true,
    ));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let c1 = project.add_clip(v1, media, tr(0, 20), rt(10)).expect("c1");
    let c2 = project.add_clip(v1, media, tr(0, 20), rt(30)).expect("c2");
    let engine = Engine::with_project(EngineConfig::default(), project).expect("engine");
    (engine, media, v1, c1, c2)
}

/// Clips on `track` of the root timeline, in start order.
fn root_clips(engine: &Engine, track: TrackId) -> Vec<ClipId> {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .clips_ordered()
        .iter()
        .map(|clip| clip.id)
        .collect()
}

#[test]
fn create_nested_sequence_replaces_clips_and_undo_redo_cycles() {
    let (mut engine, _media, v1, c1, c2) = engine_with_two_clips();
    assert_eq!(root_clips(&engine, v1), vec![c1, c2]);

    let outcome = engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Nest 1".to_string(),
            clips: vec![c1, c2],
        }))
        .expect("create nested sequence");
    let nested_clip_id = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created outcome, got {other:?}"),
    };

    // The selection is replaced by exactly one nested clip spanning [10, 50).
    assert_eq!(root_clips(&engine, v1), vec![nested_clip_id]);
    let clip = engine
        .project()
        .timeline()
        .clip(nested_clip_id)
        .expect("nested clip");
    assert_eq!(clip.timeline, tr(10, 40));
    let seq_id = match &clip.content {
        ClipSource::Sequence { sequence, source } => {
            assert_eq!(*source, tr(0, 40));
            *sequence
        }
        other => panic!("expected ClipSource::Sequence, got {other:?}"),
    };

    // The sequence holds one replicated lane, with the selection rebased to 0.
    let seq = engine.project().sequence(seq_id).expect("sequence");
    assert_eq!(seq.name, "Nest 1");
    assert_eq!(seq.timeline.order().len(), 1);
    let inner_lane = seq.timeline.order()[0];
    let inner = seq.timeline.track(inner_lane).expect("inner lane");
    let inner_clips = inner.clips_ordered();
    assert_eq!(inner_clips.len(), 2);
    assert_eq!(inner_clips[0].timeline, tr(0, 20));
    assert_eq!(inner_clips[1].timeline, tr(20, 20));

    // Undo restores the two original clips and drops the sequence.
    assert!(engine.undo(), "undo nested sequence creation");
    assert_eq!(root_clips(&engine, v1), vec![c1, c2]);
    assert!(engine.project().sequence(seq_id).is_none());

    // Redo restores the nested clip and the sequence.
    assert!(engine.redo(), "redo nested sequence creation");
    assert_eq!(root_clips(&engine, v1), vec![nested_clip_id]);
    assert!(engine.project().sequence(seq_id).is_some());
}

#[test]
fn nesting_onto_a_lane_that_refuses_a_sequence_leaves_the_project_untouched() {
    let mut engine = Engine::with_project(
        EngineConfig::default(),
        Project::new("nested refusal", Rational::FPS_24),
    )
    .expect("engine");

    // A sticker lane takes generated stickers but no media-bearing clip, and a
    // nested sequence clip is media-bearing.
    let stickers = add_track(&mut engine, TrackKind::Sticker, "S1");
    let solid = add_generated(
        &mut engine,
        stickers,
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        tr(0, 24),
    );

    let err = engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Refused".to_string(),
            clips: vec![solid],
        }))
        .expect_err("a sticker lane refuses a nested sequence clip");
    match err {
        EngineError::Model(ModelError::IncompatibleTrackKind { track, kind }) => {
            assert_eq!(track, stickers);
            assert_eq!(kind, TrackKind::Sticker);
        }
        other => panic!("expected IncompatibleTrackKind, got {other:?}"),
    }

    // The refusal cost the project nothing: the clip is still on its lane and
    // no sequence was registered.
    assert_eq!(root_clips(&engine, stickers), vec![solid]);
    assert_eq!(engine.project().sequences().count(), 0);
}

#[test]
fn place_nested_sequence_and_undo_redo() {
    let mut project = Project::new("place nested", Rational::FPS_24);
    let seq_id = project.add_sequence(aria_models::Sequence::new("Lower Thirds", Rational::FPS_24));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");

    let outcome = engine
        .apply(Command::Edit(EditCommand::PlaceNestedSequence {
            track: v1,
            sequence: seq_id,
            source: tr(0, 100),
            start: rt(24),
        }))
        .expect("place nested sequence");
    let placed = match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected Created outcome, got {other:?}"),
    };

    let clip = engine
        .project()
        .timeline()
        .clip(placed)
        .expect("placed clip");
    assert_eq!(clip.timeline, tr(24, 100));
    assert!(clip.is_sequence());
    assert_eq!(clip.sequence(), Some(seq_id));

    assert!(engine.undo(), "undo place nested sequence");
    assert!(engine.project().timeline().clip(placed).is_none());

    assert!(engine.redo(), "redo place nested sequence");
    assert!(engine.project().timeline().clip(placed).is_some());
}

#[test]
fn open_sequence_navigates_and_undoes() {
    let mut project = Project::new("navigate", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/media/take.mp4",
        1920,
        1080,
        Rational::FPS_24,
        480,
        true,
    ));
    let seq_id = project.add_sequence(aria_models::Sequence::new("Scene 2", Rational::FPS_24));
    project.add_track(TrackKind::Video, "V1");
    let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");

    assert_eq!(engine.active_sequence(), None);

    let outcome = engine
        .apply(Command::Edit(EditCommand::OpenSequence {
            sequence: Some(seq_id),
        }))
        .expect("open sequence");
    assert_eq!(outcome, ApplyOutcome::Edited(EditOutcome::OpenedSequence));
    assert_eq!(engine.active_sequence(), Some(seq_id));

    // Edit commands now land inside the active sequence, not on the root.
    let nested_lane = add_track(&mut engine, TrackKind::Video, "Nested V1");
    let nested_clip = add_media_clip(&mut engine, nested_lane, media, tr(0, 50), rt(0));

    let seq = engine.project().sequence(seq_id).expect("sequence");
    assert!(seq.timeline.clip(nested_clip).is_some());
    assert!(
        engine.project().root_timeline().clip(nested_clip).is_none(),
        "an edit inside a sequence reached the root timeline"
    );

    // Back to the root timeline: the lane added inside the sequence stayed there.
    engine.open_sequence(None).expect("switch to root timeline");
    assert_eq!(engine.active_sequence(), None);
    assert_eq!(
        engine.project().timeline().order().len(),
        1,
        "the lane added inside the sequence followed the view back out"
    );

    assert!(engine.undo(), "undo switch to root");
    assert_eq!(engine.active_sequence(), Some(seq_id));

    assert!(engine.undo(), "undo add clip inside the sequence");
    let seq = engine.project().sequence(seq_id).expect("sequence");
    assert!(seq.timeline.clip(nested_clip).is_none());
}

#[test]
fn direct_and_transitive_cycle_prevention() {
    let mut project = Project::new("cycles", Rational::FPS_24);
    let root_v1 = project.add_track(TrackKind::Video, "Root V1");
    let seq_a = project.add_sequence(aria_models::Sequence::new("Seq A", Rational::FPS_24));
    let seq_b = project.add_sequence(aria_models::Sequence::new("Seq B", Rational::FPS_24));
    let seq_c = project.add_sequence(aria_models::Sequence::new("Seq C", Rational::FPS_24));
    let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");

    engine.open_sequence(Some(seq_a)).expect("open A");
    let a_v1 = add_track(&mut engine, TrackKind::Video, "A V1");

    // Direct cycle: A inside A.
    let err = place(&mut engine, a_v1, seq_a).expect_err("A inside A is a cycle");
    assert_cycle(err, seq_a);

    place(&mut engine, a_v1, seq_b).expect("place B in A");

    engine.open_sequence(Some(seq_b)).expect("open B");
    let b_v1 = add_track(&mut engine, TrackKind::Video, "B V1");

    // Mutual cycle: A inside B, when A already contains B.
    let err = place(&mut engine, b_v1, seq_a).expect_err("A inside B is a cycle");
    assert_cycle(err, seq_a);

    place(&mut engine, b_v1, seq_c).expect("place C in B");

    engine.open_sequence(Some(seq_c)).expect("open C");
    let c_v1 = add_track(&mut engine, TrackKind::Video, "C V1");

    // Transitive cycle: A inside C, when A -> B -> C.
    let err = place(&mut engine, c_v1, seq_a).expect_err("A inside C is a cycle");
    assert_cycle(err, seq_a);

    // The root contains nothing yet, so Root -> A -> B -> C is acyclic.
    engine.open_sequence(None).expect("return to root");
    place(&mut engine, root_v1, seq_a).expect("place A on root");
}

fn place(
    engine: &mut Engine,
    track: TrackId,
    sequence: aria_models::SequenceId,
) -> Result<ApplyOutcome, EngineError> {
    engine.apply(Command::Edit(EditCommand::PlaceNestedSequence {
        track,
        sequence,
        source: tr(0, 100),
        start: rt(0),
    }))
}

fn assert_cycle(err: EngineError, placed: aria_models::SequenceId) {
    match err {
        EngineError::Model(ModelError::SequenceCycle(sequence)) => assert_eq!(sequence, placed),
        other => panic!("expected SequenceCycle, got {other:?}"),
    }
}

#[test]
fn project_file_serialization_roundtrip_with_nested_sequences() {
    let (mut engine, _media, v1, c1, _c2) = engine_with_two_clips();

    engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Persistent Nest".to_string(),
            clips: vec![c1],
        }))
        .expect("create nested sequence");

    let json = engine.project().save_to_string().expect("save_to_string");
    let loaded = Project::load_from_string(&json).expect("load_from_string");

    assert_eq!(loaded.sequences().count(), 1);
    let seq = loaded.sequences().next().expect("one sequence");
    assert_eq!(seq.name, "Persistent Nest");
    assert_eq!(seq.timeline.order().len(), 1);

    let root = loaded.timeline().track(v1).expect("root lane");
    let nested: Vec<&aria_models::Clip> = root
        .clips_ordered()
        .filter(|clip| clip.is_sequence())
        .collect();
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].sequence(), Some(seq.id));
}

#[test]
fn scene_resolution_renders_nested_sequence() {
    let (mut engine, media, _v1, c1, c2) = engine_with_two_clips();

    engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Render Nest".to_string(),
            clips: vec![c1, c2],
        }))
        .expect("create nested sequence");

    // Root tick 15 is 5 ticks into the nested clip, which is 5 ticks into the
    // first clip inside the sequence: the recursion has to carry both offsets.
    let scene = resolve_with(engine.project(), rt(15), ResolveOverrides::default())
        .expect("resolve the nested scene");
    assert_eq!(scene.layers.len(), 1);
    let LayerSource::Nest { scene: nest, .. } = &scene.layers[0].source else {
        panic!(
            "expected the nested sequence's frame, got {:?}",
            scene.layers[0].source
        );
    };
    assert_eq!(nest.layers.len(), 1);
    match &nest.layers[0].source {
        LayerSource::Media {
            media: resolved,
            source_time,
        } => {
            assert_eq!(*resolved, media);
            assert_eq!(*source_time, rt(5));
        }
        other => panic!("expected a media layer inside the nest, got {other:?}"),
    }
}

/// New Sequence: the open timeline's rate, canvas, and audio layout, one video
/// and one audio lane, opened for editing. Undo removes it and reopens what was
/// open; redo brings it back under the same id.
///
/// Closes the class where a created sequence is stored but not opened (Ctrl+N
/// then edits the timeline that was just left), or opened but left behind
/// by undo as an orphan in the store.
#[test]
fn create_sequence_opens_an_empty_sequence_and_undo_redo_cycles() {
    let mut project = Project::new("new-sequence", Rational::FPS_24);
    let canvas = aria_models::CanvasSettings {
        resolution: Some([1280, 720]),
        ..Default::default()
    };
    project.timeline_mut().set_canvas(canvas);
    project
        .timeline_mut()
        .set_audio_layout(aria_models::AudioLayout::Mono);
    let root_lane = project.add_track(TrackKind::Video, "V1");
    let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");
    let depth = engine.history_depth();

    let outcome = engine
        .apply(Command::Edit(EditCommand::CreateSequence {
            name: "  Scene 3  ".into(),
        }))
        .expect("create a sequence");
    assert_eq!(outcome, ApplyOutcome::Edited(EditOutcome::OpenedSequence));
    assert_eq!(engine.history_depth(), depth + 1, "one undo entry");
    let id = engine.active_sequence().expect("the new sequence is open");
    let sequence = engine.project().sequence(id).expect("stored");
    assert_eq!(sequence.name, "Scene 3");
    assert_eq!(sequence.timeline.frame_rate, Rational::FPS_24);
    assert_eq!(sequence.timeline.canvas(), canvas);
    assert_eq!(
        sequence.timeline.audio_layout(),
        aria_models::AudioLayout::Mono
    );
    let lanes: Vec<(TrackKind, String)> = sequence
        .timeline
        .tracks_ordered()
        .map(|lane| (lane.kind, lane.name.clone()))
        .collect();
    // Bottom to top: the model stacks audio lanes below picture lanes.
    assert_eq!(
        lanes,
        vec![
            (TrackKind::Audio, "A1".to_string()),
            (TrackKind::Video, "V1".to_string()),
        ]
    );
    assert_eq!(engine.project().root_timeline().order(), &[root_lane]);

    assert!(engine.undo());
    assert_eq!(engine.active_sequence(), None, "undo reopens the root");
    assert!(
        engine.project().sequence(id).is_none(),
        "undo left the sequence in the store"
    );

    assert!(engine.redo());
    assert_eq!(engine.active_sequence(), Some(id));
    assert_eq!(
        engine.project().sequence(id).map(|s| s.name.as_str()),
        Some("Scene 3")
    );
}

/// Created from inside another sequence, undo returns to that sequence rather
/// than the root, and a blank name is stored as `Sequence`.
#[test]
fn create_sequence_inside_a_sequence_undoes_back_to_it() {
    let mut project = Project::new("new-sequence", Rational::FPS_24);
    let outer = project.add_sequence(aria_models::Sequence::new("Outer", Rational::FPS_24));
    let mut engine = Engine::with_project(EngineConfig::default(), project).expect("engine");
    engine
        .open_sequence(Some(outer))
        .expect("open the outer sequence");

    engine
        .apply(Command::Edit(EditCommand::CreateSequence {
            name: "   ".into(),
        }))
        .expect("create a sequence");
    let id = engine.active_sequence().expect("open");
    assert_ne!(id, outer);
    assert_eq!(
        engine.project().sequence(id).map(|s| s.name.as_str()),
        Some("Sequence")
    );

    assert!(engine.undo());
    assert_eq!(engine.active_sequence(), Some(outer));
    assert_eq!(engine.project().sequence_count(), 1);
}
