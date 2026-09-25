//! `clip_crop` cases.

use super::*;
use crate::wire;

#[test]
fn clip_crop_merges_edges_and_rejects_empty_frames() {
    let (mut project, _, _, _, clip, _) = fixture();

    // Fresh clip: edges lower straight into the kept-region rect.
    let edit = lower(
        &project,
        WireCommand::SetClipCrop(wire::SetClipCrop {
            clip,
            left: Some(0.25),
            top: None,
            right: Some(0.25),
            bottom: None,
            flip_h: Some(true),
            flip_v: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipCrop {
            clip: ClipId::from_raw(clip),
            crop: CropRect {
                x: 0.25,
                y: 0.0,
                w: 0.5,
                h: 1.0
            },
            flip_h: true,
            flip_v: false,
            at: None,
        }
    );

    // Omitted fields keep the stored framing: crop the top, keep the
    // earlier horizontal window and flip.
    project
        .set_clip_crop(
            ClipId::from_raw(clip),
            CropRect {
                x: 0.25,
                y: 0.0,
                w: 0.5,
                h: 1.0,
            },
            true,
            false,
            None,
        )
        .unwrap();
    let edit = lower(
        &project,
        WireCommand::SetClipCrop(wire::SetClipCrop {
            clip,
            left: None,
            top: Some(0.1),
            right: None,
            bottom: None,
            flip_h: None,
            flip_v: None,
        }),
    );
    match edit {
        EditCommand::SetClipCrop { crop, flip_h, .. } => {
            assert_eq!(crop.x, 0.25);
            assert_eq!(crop.w, 0.5);
            assert_eq!(crop.y, 0.1);
            assert!((crop.h - 0.9).abs() < 1e-6);
            assert!(flip_h, "stored flip must be kept");
        }
        other => panic!("unexpected lowering: {other:?}"),
    }

    // Edges that eat the whole frame are rejected with a hint.
    let msg = reject(
        &project,
        WireCommand::SetClipCrop(wire::SetClipCrop {
            clip,
            left: Some(0.6),
            top: None,
            right: Some(0.6),
            bottom: None,
            flip_h: None,
            flip_v: None,
        }),
    );
    assert!(msg.contains("leaves no visible frame"), "{msg}");

    // Out-of-range fractions are rejected by name.
    let msg = reject(
        &project,
        WireCommand::SetClipCrop(wire::SetClipCrop {
            clip,
            left: None,
            top: None,
            right: None,
            bottom: Some(1.5),
            flip_h: None,
            flip_v: None,
        }),
    );
    assert!(msg.contains("bottom must be a fraction"), "{msg}");
}

#[test]
fn pan_keyframe_lowers_and_rejects_out_of_range() {
    let (project, _, _, _, clip, title) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Pan,
            at: 1.0,
            value: Some(-0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: Some(wire::WireEasing::Linear),

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Pan,
            at: RationalTime::new(24, R24),
            value: ParamValue::Scalar(-0.5),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    let edit = lower(
        &project,
        WireCommand::SetParamConstant(wire::SetParamConstant {
            clip,
            param: wire::WireClipParam::Pan,
            value: Some(0.25),
            position: None,
            rgba: None,
            rect: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamConstant {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Pan,
            value: ParamValue::Scalar(0.25),
        }
    );

    // Range + media-backed gating are enforced by the model when the edit
    // lands (same as volume keyframes — wire lower accepts the scalar shape).
    let mut project = project;
    assert!(
        project
            .set_param_constant(
                ClipId::from_raw(clip),
                ClipParam::Pan,
                ParamValue::Scalar(-1.01),
            )
            .is_err()
    );
    assert!(
        project
            .set_param_constant(
                ClipId::from_raw(title),
                ClipParam::Pan,
                ParamValue::Scalar(0.5),
            )
            .is_err()
    );
}

#[test]
fn crop_keyframe_uses_rect_on_wire() {
    let (project, _, _, _, clip, _) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Crop,
            at: 1.0,
            value: None,
            position: None,
            rgba: None,
            rect: Some([0.1, 0.2, 0.5, 0.5]),
            easing: Some(wire::WireEasing::Linear),

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Crop,
            at: RationalTime::new(24, R24),
            value: ParamValue::Rect([0.1, 0.2, 0.5, 0.5]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    // Degenerate width rejected by model validation after lowering path.
    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Crop,
            at: 1.0,
            value: None,
            position: None,
            rgba: None,
            rect: Some([0.0, 0.0, 0.001, 1.0]),
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(
        msg.contains("crop") || msg.contains("0.01") || msg.contains("at least"),
        "{msg}"
    );

    // Scalar value rejected for crop.
    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Crop,
            at: 1.0,
            value: Some(0.5),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(msg.contains("rect"), "{msg}");
}

#[test]
fn clip_crop_rejects_audio_lane_clips() {
    let (mut project, media, _, _, _, _) = fixture();
    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetClipCrop(wire::SetClipCrop {
            clip: audio_clip.raw(),
            left: Some(0.2),
            top: None,
            right: None,
            bottom: None,
            flip_h: None,
            flip_v: None,
        }),
    );
    assert!(msg.contains("no frame to crop"), "{msg}");
}

#[test]
fn move_effect_lowers_and_reports_invalid_requests() {
    let (mut project, _, _, _, clip, _) = fixture();
    let clip_id = ClipId::from_raw(clip);
    for effect in ["gaussian_blur", "glitch", "vignette"] {
        project.add_effect(clip_id, effect).unwrap();
    }

    assert_eq!(
        lower(
            &project,
            WireCommand::MoveEffect(wire::MoveEffect {
                clip,
                from_index: 0,
                to_index: 2,
            }),
        ),
        EditCommand::MoveEffect {
            clip: clip_id,
            from_index: 0,
            to_index: 2,
        }
    );

    for command in [
        wire::MoveEffect {
            clip,
            from_index: 3,
            to_index: 0,
        },
        wire::MoveEffect {
            clip,
            from_index: 0,
            to_index: 3,
        },
    ] {
        let msg = reject(&project, WireCommand::MoveEffect(command));
        assert!(msg.contains("chain length 3"), "{msg}");
    }

    let msg = reject(
        &project,
        WireCommand::MoveEffect(wire::MoveEffect {
            clip,
            from_index: 1,
            to_index: 1,
        }),
    );
    assert!(msg.contains("would make no change"), "{msg}");
}

#[test]
fn clip_speed_lowers_to_exact_rationals() {
    let (project, _, _, _, clip, title) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipSpeed(wire::SetClipSpeed {
            clip,
            speed: Some(2.0),
            reversed: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipSpeed {
            clip: ClipId::from_raw(clip),
            speed: Rational::new(2, 1),
            reversed: false,
        }
    );

    // Hundredth snapping: 0.5 → 1/2, 0.75 → 3/4, 0.333 → 33/100.
    assert_eq!(rational_speed(0.5).unwrap(), Rational::new(1, 2));
    assert_eq!(rational_speed(0.75).unwrap(), Rational::new(3, 4));
    assert_eq!(rational_speed(0.333).unwrap(), Rational::new(33, 100));

    // Omitted fields keep the clip's current retiming (reverse-only).
    let edit = lower(
        &project,
        WireCommand::SetClipSpeed(wire::SetClipSpeed {
            clip,
            speed: None,
            reversed: Some(true),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipSpeed {
            clip: ClipId::from_raw(clip),
            speed: Rational::new(1, 1),
            reversed: true,
        }
    );

    // Out-of-range speeds and generated clips are rejected with names.
    let msg = reject(
        &project,
        WireCommand::SetClipSpeed(wire::SetClipSpeed {
            clip,
            speed: Some(0.0),
            reversed: None,
        }),
    );
    assert!(msg.contains("between 0.05 and 100"), "{msg}");
    let msg = reject(
        &project,
        WireCommand::SetClipSpeed(wire::SetClipSpeed {
            clip: title,
            speed: Some(2.0),
            reversed: None,
        }),
    );
    assert!(msg.contains("generated clip"), "{msg}");
}

#[test]
fn clip_pitch_lowers_and_rejects_generated() {
    let (project, _, _, _, clip, title) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetClipPitch(wire::SetClipPitch {
            clip,
            preserve_pitch: false,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipPitch {
            clip: ClipId::from_raw(clip),
            preserve_pitch: false,
        }
    );

    // Generated clips have no footage to stretch, so pitch is meaningless.
    let msg = reject(
        &project,
        WireCommand::SetClipPitch(wire::SetClipPitch {
            clip: title,
            preserve_pitch: true,
        }),
    );
    assert!(msg.contains("generated clip"), "{msg}");
}
