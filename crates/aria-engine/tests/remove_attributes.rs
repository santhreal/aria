//! Edit > Remove Attributes through the engine: one command is one undo entry,
//! undo brings back every removed group and the duration a cleared ramp
//! re-derived, and a refusal leaves the clip and the history as they were.
//!
//! Closes the class where a multi-group reset lands as several history entries
//! (so Undo brings back only part of it) or where a refused removal leaves part
//! of the groups reset. Does not cover which groups each attribute holds; that
//! is `aria-models`' `tests_attributes.rs`.

mod common;

use aria_commands::{Command, EditCommand};
use aria_engine::Engine;
use aria_models::{
    ClipAttribute, ClipId, ClipParam, MediaId, MediaSource, ParamValue, Project, Rational, TrackId,
    TrackKind, speed_preset,
};
use common::{rt, tr};

fn engine_with_clip() -> (Engine, ClipId, TrackId, MediaId) {
    let mut project = Project::new("remove attributes", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/media/take.mp4",
        1920,
        1080,
        Rational::FPS_24,
        1000,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(track, media, tr(0, 100), rt(0))
        .expect("add clip");
    project
        .set_param_constant(clip, ClipParam::Position, ParamValue::Vec2([0.25, 0.0]))
        .expect("move");
    project
        .set_clip_speed_curve(clip, Some(speed_preset("montage").expect("preset")))
        .expect("ramp");
    project.add_effect(clip, "gaussian_blur").expect("effect");
    let engine = Engine::with_project(aria_engine::EngineConfig { undo_limit: 32 }, project)
        .expect("engine");
    (engine, clip, track, media)
}

#[test]
fn removing_groups_is_one_undo_step_that_restores_them_all() {
    let (mut engine, clip, _, _) = engine_with_clip();
    let before = engine.project().clip(clip).expect("clip").clone();
    assert!(before.has_speed_curve());
    engine
        .apply(Command::Edit(EditCommand::RemoveAttributes {
            clip,
            attributes: vec![
                ClipAttribute::Motion,
                ClipAttribute::TimeRemapping,
                ClipAttribute::Effects,
            ],
        }))
        .expect("remove");
    let after = engine.project().clip(clip).expect("clip");
    for attribute in [
        ClipAttribute::Motion,
        ClipAttribute::TimeRemapping,
        ClipAttribute::Effects,
    ] {
        assert!(!attribute.is_set(after), "{attribute:?} survived");
    }
    assert_eq!(
        after.timeline,
        tr(0, 100),
        "the cleared ramp re-derived the length"
    );

    assert!(engine.undo());
    assert_eq!(engine.project().clip(clip), Some(&before));
    assert!(!engine.can_undo(), "the removal was more than one step");
    assert!(engine.redo());
    assert!(!ClipAttribute::Effects.is_set(engine.project().clip(clip).expect("clip")));
}

#[test]
fn a_refused_removal_changes_nothing() {
    let (mut engine, clip, track, media) = engine_with_clip();
    let end = engine
        .project()
        .clip(clip)
        .expect("clip")
        .timeline
        .end()
        .expect("end");
    engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media,
            source: tr(500, 50),
            start: end,
        }))
        .expect("neighbor");
    let before = engine.project().clip(clip).expect("clip").clone();
    let refused = engine.apply(Command::Edit(EditCommand::RemoveAttributes {
        clip,
        attributes: vec![ClipAttribute::Effects, ClipAttribute::TimeRemapping],
    }));
    assert!(
        refused.is_err(),
        "the cleared ramp would overlap the neighbor"
    );
    assert_eq!(engine.project().clip(clip), Some(&before));
    assert!(engine.undo(), "the neighbor is still the last step");
    assert_eq!(engine.project().timeline().clip_count(), 1);
}
