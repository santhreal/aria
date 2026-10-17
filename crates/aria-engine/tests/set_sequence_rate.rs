//! Contract tests for sequence frame rate resampling, undo/redo, marker tracking,
//! invalid rate rejection, save/load roundtripping, and footage rate mismatch reporting.
//!
//! Closes the defect class where a sequence rate was locked at creation, audio layout was
//! not modeled, and rate mismatches against imported footage went undetected.

mod common;

use aria_commands::{Command, EditCommand};
use aria_core::{Rational, TimeRange};
use aria_engine::{Engine, EngineConfig};
use aria_models::{AudioLayout, Generator, MediaSource, Project, TimeDisplay};

use common::{add_generated, add_track, rt, tr};

#[test]
fn a_rate_change_moves_clip_placements_to_equivalent_wall_clock_time() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    // The lane comes from the generator's own catalog row: a colour matte lives
    // on the graphics lane, and a fixture that names a lane by hand goes stale
    // the moment that row moves.
    let matte = Generator::SolidColor {
        rgba: [255, 0, 0, 255],
    };
    let track = add_track(&mut engine, aria_models::generator_spec(&matte).lane, "G1");

    // Place a 2-second clip starting at 1.0s (ticks 24..72 at 24fps).
    let clip_id = add_generated(&mut engine, track, matte, tr(24, 48));

    let clip = engine.project().timeline().clip(clip_id).unwrap();
    assert_eq!(clip.timeline.start.value, 24);
    assert_eq!(clip.timeline.duration.value, 48);
    assert_eq!(clip.timeline.start.seconds(), 1.0);
    assert_eq!(clip.timeline.duration.seconds(), 2.0);

    // Resample to 30fps.
    engine
        .apply(Command::Edit(EditCommand::SetSequenceRate {
            frame_rate: Rational::FPS_30,
        }))
        .expect("resample to 30fps");

    assert_eq!(engine.project().timeline().frame_rate, Rational::FPS_30);
    let clip = engine.project().timeline().clip(clip_id).unwrap();
    assert_eq!(clip.timeline.start.value, 30);
    assert_eq!(clip.timeline.duration.value, 60);
    assert_eq!(clip.timeline.start.seconds(), 1.0);
    assert_eq!(clip.timeline.duration.seconds(), 2.0);
}

#[test]
fn undo_restores_every_tick_and_keyframe_exactly() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    let matte = Generator::SolidColor {
        rgba: [0, 255, 0, 255],
    };
    let track = add_track(&mut engine, aria_models::generator_spec(&matte).lane, "G1");

    let clip_id = add_generated(&mut engine, track, matte, tr(0, 48));

    // Through the shipped commands, because a rate change has to carry whatever
    // an operator could have put on the clip — and a test that reaches past the
    // command surface to plant it proves nothing about the path that ships.
    engine
        .apply(Command::Edit(EditCommand::SetParamKeyframe {
            clip: clip_id,
            param: aria_models::ClipParam::Opacity,
            at: rt(24),
            value: aria_models::ParamValue::Scalar(0.5),
            easing: aria_models::Easing::Linear,
            tangents: None,
        }))
        .expect("keyframe the opacity");

    // Change rate to 30fps.
    engine
        .apply(Command::Edit(EditCommand::SetSequenceRate {
            frame_rate: Rational::FPS_30,
        }))
        .expect("set rate 30fps");

    let resampled = engine.project().timeline().clip(clip_id).unwrap();
    assert_eq!(resampled.timeline.duration.value, 60);
    assert_eq!(resampled.transform.opacity.keyframes()[0].tick, 30);

    // Undo the rate change restores every tick exactly.
    assert!(engine.undo());
    assert_eq!(engine.project().timeline().frame_rate, Rational::FPS_24);
    let restored = engine.project().timeline().clip(clip_id).unwrap();
    assert_eq!(restored.timeline.start.value, 0);
    assert_eq!(restored.timeline.duration.value, 48);
    assert_eq!(restored.transform.opacity.keyframes()[0].tick, 24);

    // Redo reapplies the resampled state.
    assert!(engine.redo());
    assert_eq!(engine.project().timeline().frame_rate, Rational::FPS_30);
    let redone = engine.project().timeline().clip(clip_id).unwrap();
    assert_eq!(redone.timeline.duration.value, 60);
    assert_eq!(redone.transform.opacity.keyframes()[0].tick, 30);
}

#[test]
fn markers_follow_sequence_rate_change() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine
        .apply(Command::Edit(EditCommand::AddMarker {
            at: rt(48),
            name: "Cue".into(),
            color: Some(aria_models::MarkerColor::Red),
        }))
        .expect("add the marker");

    assert_eq!(engine.project().timeline().markers()[0].tick.value, 48);

    engine
        .apply(Command::Edit(EditCommand::SetSequenceRate {
            frame_rate: Rational::FPS_30,
        }))
        .expect("change to 30fps");

    let marker = &engine.project().timeline().markers()[0];
    assert_eq!(marker.tick.value, 60);
    assert_eq!(marker.tick.rate, Rational::FPS_30);

    assert!(engine.undo());
    let marker = &engine.project().timeline().markers()[0];
    assert_eq!(marker.tick.value, 48);
    assert_eq!(marker.tick.rate, Rational::FPS_24);
}

#[test]
fn invalid_rate_is_refused_with_message_naming_the_rule() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");

    let invalid_cases = [
        Rational::new(0, 1),
        Rational::new(-24, 1),
        Rational::new(24, 0),
        Rational::new(24, -1),
    ];

    for bad in invalid_cases {
        let err = engine
            .apply(Command::Edit(EditCommand::SetSequenceRate {
                frame_rate: bad,
            }))
            .expect_err("invalid frame rate must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("positive numerator and denominator"),
            "error message '{msg}' must name the rule"
        );
    }
}

#[test]
fn round_trip_through_save_load_keeps_new_rate_and_audio_layout() {
    let mut project = Project::new("test_project", Rational::FPS_24);
    let matte = Generator::SolidColor {
        rgba: [10, 20, 30, 255],
    };
    let track = project.add_track(aria_models::generator_spec(&matte).lane, "G1");
    project
        .add_generated(track, matte, TimeRange::at_rate(0, 24, Rational::FPS_24))
        .unwrap();

    project.resample_frame_rate(Rational::FPS_60).unwrap();
    project.set_audio_layout(AudioLayout::Surround51);

    let saved = project.save_to_string().expect("save project");
    let loaded = Project::load_from_string(&saved).expect("load project");

    assert_eq!(loaded.timeline().frame_rate, Rational::FPS_60);
    assert_eq!(loaded.audio_layout(), AudioLayout::Surround51);
    let clip = loaded
        .timeline()
        .tracks_ordered()
        .next()
        .unwrap()
        .clips()
        .next()
        .unwrap();
    assert_eq!(clip.timeline.duration.value, 60);
    assert_eq!(clip.timeline.start.rate, Rational::FPS_60);
}

#[test]
fn mismatch_report_names_pools_largest_rate() {
    let mut project = Project::new("mismatch_test", Rational::FPS_24);

    // No media => no mismatch.
    assert_eq!(project.largest_source_frame_rate(), None);
    assert_eq!(project.frame_rate_mismatch(), None);

    // 1080p @ 30fps
    project.add_media(MediaSource::new(
        "/media/clip1.mp4",
        1920,
        1080,
        Rational::FPS_30,
        300,
        true,
    ));

    // 4K @ 60fps (largest video by area)
    project.add_media(MediaSource::new(
        "/media/clip2_4k.mp4",
        3840,
        2160,
        Rational::FPS_60,
        600,
        true,
    ));

    // Audio-only (ignored for video mismatch calculation)
    project.add_media(MediaSource::new(
        "/media/audio.mp3",
        0,
        0,
        Rational::new(1000, 1),
        1000,
        true,
    ));

    assert_eq!(
        project.largest_source_frame_rate(),
        Some(Rational::FPS_60),
        "4K source is largest in pool"
    );
    assert_eq!(
        project.frame_rate_mismatch(),
        Some(Rational::FPS_60),
        "sequence at 24fps differs from largest footage at 60fps"
    );

    // Changing sequence to 60fps clears the mismatch.
    project.resample_frame_rate(Rational::FPS_60).unwrap();
    assert_eq!(project.frame_rate_mismatch(), None);
}

#[test]
fn audio_layout_edit_applies_and_undo_restores_previous() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    assert_eq!(engine.project().audio_layout(), AudioLayout::Stereo);

    engine
        .apply(Command::Edit(EditCommand::SetAudioLayout {
            layout: AudioLayout::Surround51,
        }))
        .expect("set audio layout 5.1");

    assert_eq!(engine.project().audio_layout(), AudioLayout::Surround51);

    assert!(engine.undo());
    assert_eq!(engine.project().audio_layout(), AudioLayout::Stereo);

    assert!(engine.redo());
    assert_eq!(engine.project().audio_layout(), AudioLayout::Surround51);
}

/// The ruler's time display is one undoable unit and survives a save. A
/// timecode sequence saves without the field, so a file written before the
/// field existed and a timecode file read back the same way.
#[test]
fn time_display_edit_applies_undoes_and_survives_a_save() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    assert_eq!(engine.project().time_display(), TimeDisplay::Timecode);

    for display in [TimeDisplay::Frames, TimeDisplay::FeetFrames] {
        engine
            .apply(Command::Edit(EditCommand::SetTimeDisplay { display }))
            .expect("set time display");
        assert_eq!(engine.project().time_display(), display);
    }

    assert!(engine.undo());
    assert_eq!(engine.project().time_display(), TimeDisplay::Frames);
    assert!(engine.undo());
    assert_eq!(engine.project().time_display(), TimeDisplay::Timecode);
    assert!(engine.redo());
    assert_eq!(engine.project().time_display(), TimeDisplay::Frames);

    let saved = engine.project().save_to_string().expect("save project");
    let loaded = Project::load_from_string(&saved).expect("load project");
    assert_eq!(loaded.time_display(), TimeDisplay::Frames);

    let mut timecode = Project::new("timecode", Rational::FPS_24);
    timecode.set_time_display(TimeDisplay::Timecode);
    let saved = timecode.save_to_string().expect("save project");
    assert!(
        !saved.contains("time_display"),
        "a timecode sequence saves without the field"
    );
    let loaded = Project::load_from_string(&saved).expect("load project");
    assert_eq!(loaded.time_display(), TimeDisplay::Timecode);
}
