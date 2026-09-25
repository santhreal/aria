//! `set_clip_2` cases.

use super::*;

#[test]
fn set_clip_beats_rejects_generated_clips() {
    let mut project = Project::new("test", R24);
    let track = project.add_track(TrackKind::Text, "T");
    let clip = project
        .add_generated(track, Generator::text("hi"), tr(0, 48))
        .unwrap();
    assert!(matches!(
        project.set_clip_beats(clip, vec![1, 2]),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn split_keeps_beats_on_both_halves() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    project.set_clip_beats(clip, vec![10, 60, 90]).unwrap();

    let right = project.split_clip(clip, rt(50)).unwrap();
    // Both halves keep the full source-tick list; each shows only the
    // beats inside its own window via `beat_timeline_ticks`.
    assert_eq!(project.clip(clip).unwrap().beats, vec![10, 60, 90]);
    assert_eq!(project.clip(right).unwrap().beats, vec![10, 60, 90]);
    // Left window [0,50): only the beat at 10 maps in.
    assert_eq!(project.clip(clip).unwrap().beat_timeline_ticks(), vec![10]);
    // Right window [50,100) at timeline [50,100): beats 60, 90 map in.
    assert_eq!(
        project.clip(right).unwrap().beat_timeline_ticks(),
        vec![60, 90]
    );
}

// --- set_clip_audio (M1) -------------------------------------------------

#[test]
fn set_clip_audio_sets_volume_and_fades() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    project
        .set_clip_audio(clip, Some(0.5), rt(10), rt(20))
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.volume.constant(), Some(0.5));
    assert_eq!((c.fade_in, c.fade_out), (10, 20));
    assert!(c.has_custom_audio());

    // A `None` volume keeps the gain and only moves the fades.
    project.set_clip_audio(clip, None, rt(5), rt(0)).unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.volume.constant(), Some(0.5));
    assert_eq!((c.fade_in, c.fade_out), (5, 0));

    // Back to defaults clears the custom-audio state.
    project
        .set_clip_audio(clip, Some(1.0), rt(0), rt(0))
        .unwrap();
    assert!(!project.clip(clip).unwrap().has_custom_audio());
}

#[test]
fn set_clip_audio_none_volume_preserves_envelope() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    // A keyframed gain envelope (dips to 0.2 mid-clip).
    project
        .set_param_keyframe(
            clip,
            ClipParam::Volume,
            rt(0),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Volume,
            rt(50),
            ParamValue::Scalar(0.2),
            Easing::Linear,
            None,
        )
        .unwrap();
    assert!(project.clip(clip).unwrap().has_volume_envelope());

    // Setting only fades (None volume) must not flatten the envelope.
    project.set_clip_audio(clip, None, rt(10), rt(20)).unwrap();
    let c = project.clip(clip).unwrap();
    assert!(c.has_volume_envelope());
    assert_eq!(c.volume.sample(50), 0.2);
    assert_eq!((c.fade_in, c.fade_out), (10, 20));
}

#[test]
fn set_clip_audio_rejects_invalid_inputs() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    for volume in [-0.1, 11.0, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            project.set_clip_audio(clip, Some(volume), rt(0), rt(0)),
            Err(ModelError::InvalidParam(_))
        ));
    }
    // Negative or longer-than-the-clip fades.
    assert!(matches!(
        project.set_clip_audio(clip, Some(1.0), rt(-1), rt(0)),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        project.set_clip_audio(clip, Some(1.0), rt(0), rt(101)),
        Err(ModelError::InvalidParam(_))
    ));

    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let generated = project
        .add_generated(fx, Generator::Adjustment, tr(0, 100))
        .unwrap();
    assert!(matches!(
        project.set_clip_audio(generated, Some(0.5), rt(0), rt(0)),
        Err(ModelError::InvalidParam(_))
    ));

    let missing = ClipId::next();
    assert_eq!(
        project.set_clip_audio(missing, Some(0.5), rt(0), rt(0)),
        Err(ModelError::UnknownClip(missing))
    );
}

#[test]
fn split_keeps_volume_and_partitions_fades() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    project
        .set_clip_audio(clip, Some(0.5), rt(10), rt(20))
        .unwrap();

    let right = project.split_clip(clip, rt(60)).unwrap();
    let left = project.clip(clip).unwrap();
    let right = project.clip(right).unwrap();
    // Volume rides both halves; the fade-in stays with the head, the
    // fade-out moves to the tail.
    assert_eq!(
        (left.volume.constant(), right.volume.constant()),
        (Some(0.5), Some(0.5))
    );
    assert_eq!((left.fade_in, left.fade_out), (10, 0));
    assert_eq!((right.fade_in, right.fade_out), (0, 20));
}

#[test]
fn pan_param_keyframe_through_project_api() {
    // Video-lane media clip (same target rule as volume — not audio-only).
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    project
        .set_param_keyframe(
            clip,
            ClipParam::Pan,
            rt(0),
            ParamValue::Scalar(-1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Pan,
            rt(50),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert!(c.has_pan_envelope());
    assert_eq!(c.pan.sample(0), -1.0);
    assert_eq!(c.pan.sample(50), 1.0);
    assert_eq!(c.pan.sample(25), 0.0);

    project
        .set_param_constant(clip, ClipParam::Pan, ParamValue::Scalar(0.25))
        .unwrap();
    assert_eq!(project.clip(clip).unwrap().pan.constant(), Some(0.25));

    for bad in [-1.01_f32, 1.01, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            project.set_param_constant(clip, ClipParam::Pan, ParamValue::Scalar(bad)),
            Err(ModelError::InvalidParam(_))
        ));
    }

    // Generated clips reject pan exactly like volume/set_clip_audio.
    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let generated = project
        .add_generated(fx, Generator::Adjustment, tr(0, 100))
        .unwrap();
    assert!(matches!(
        project.set_param_constant(generated, ClipParam::Pan, ParamValue::Scalar(0.5)),
        Err(ModelError::InvalidParam(_))
    ));
}

// --- set_clip_crop (M1) ---------------------------------------------------

#[test]
fn set_clip_crop_sets_framing() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    let crop = CropRect {
        x: 0.25,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    };
    project
        .set_clip_crop(clip, crop, true, false, None)
        .unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.crop, Param::Constant(crop));
    assert!(c.flip_h && !c.flip_v);
    assert!(c.has_custom_crop());

    // Back to the full frame clears the custom-framing state.
    project
        .set_clip_crop(clip, CropRect::FULL, false, false, None)
        .unwrap();
    assert!(!project.clip(clip).unwrap().has_custom_crop());
}

#[test]
fn set_clip_crop_rejects_invalid_targets() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();

    // Invalid rects bounce.
    assert!(matches!(
        project.set_clip_crop(
            clip,
            CropRect {
                x: 0.8,
                y: 0.0,
                w: 0.5,
                h: 1.0
            },
            false,
            false,
            None,
        ),
        Err(ModelError::InvalidParam(_))
    ));

    // Audio lanes have no frame to crop.
    let audio = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project.add_clip(audio, media_id, tr(0, 50), rt(0)).unwrap();
    assert!(matches!(
        project.set_clip_crop(audio_clip, CropRect::FULL, true, false, None),
        Err(ModelError::IncompatibleTrackKind { .. })
    ));

    let missing = ClipId::next();
    assert_eq!(
        project.set_clip_crop(missing, CropRect::FULL, false, false, None),
        Err(ModelError::UnknownClip(missing))
    );
}

#[test]
fn split_keeps_crop_and_flips_on_both_halves() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    let crop = CropRect {
        x: 0.1,
        y: 0.1,
        w: 0.8,
        h: 0.8,
    };
    project.set_clip_crop(clip, crop, true, true, None).unwrap();

    let right = project.split_clip(clip, rt(60)).unwrap();
    for id in [clip, right] {
        let c = project.clip(id).unwrap();
        assert_eq!(c.crop, Param::Constant(crop));
        assert!(c.flip_h && c.flip_v);
    }
}

#[test]
fn crop_param_keyframe_validates_each_value() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(0, 100), rt(0))
        .unwrap();
    let ok = CropRect {
        x: 0.1,
        y: 0.1,
        w: 0.5,
        h: 0.5,
    };
    project
        .set_param_keyframe(
            clip,
            ClipParam::Crop,
            rt(0),
            ParamValue::Rect([ok.x, ok.y, ok.w, ok.h]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Crop,
            rt(50),
            ParamValue::Rect([0.2, 0.2, 0.4, 0.4]),
            Easing::Linear,
            None,
        )
        .unwrap();
    assert!(project.clip(clip).unwrap().crop.is_animated());
    assert!(matches!(
        project.set_param_keyframe(
            clip,
            ClipParam::Crop,
            rt(25),
            ParamValue::Rect([0.0, 0.0, 0.001, 1.0]),
            Easing::Linear,
            None
        ),
        Err(ModelError::InvalidParam(_))
    ));
    // Scalar value rejected for crop.
    assert!(matches!(
        project.set_param_constant(clip, ClipParam::Crop, ParamValue::Scalar(0.5)),
        Err(ModelError::InvalidParam(_))
    ));
}

#[test]
fn trim_scales_source_consumption_by_speed() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(100, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip, Rational::new(2, 1), false)
        .unwrap();
    // Now timeline [0, 50) over source [100, 200).

    // Head trim by 10 timeline ticks eats 20 source ticks.
    project.trim_clip(clip, tr(10, 40)).unwrap();
    let c = project.clip(clip).unwrap();
    assert_eq!(c.source_range(), Some(tr(120, 80)));

    // Tail trim to 20 timeline ticks keeps 40 source ticks.
    project.trim_clip(clip, tr(10, 20)).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(120, 40))
    );
}

#[test]
fn trim_reversed_clip_mirrors_source_window() {
    let (mut project, media_id, track) = project_with_media(500);
    let clip = project
        .add_clip(track, media_id, tr(100, 100), rt(0))
        .unwrap();
    project
        .set_clip_speed(clip, Rational::new(1, 1), true)
        .unwrap();

    // The timeline head shows the source END: a head trim by 10 drops
    // the top 10 source ticks, keeping the bottom.
    project.trim_clip(clip, tr(10, 90)).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(100, 90))
    );

    // A tail trim drops the source BOTTOM.
    project.trim_clip(clip, tr(10, 80)).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().source_range(),
        Some(tr(110, 80))
    );
}
