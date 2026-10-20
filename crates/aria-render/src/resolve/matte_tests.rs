//! Contracts for folding a track matte out of two resolved layers.
//!
//! The class closed here is "the stencil is the layer above": a matte is not a
//! property the compositor can read off one layer, so resolve has to consume a
//! neighbour. Every way that pairing can go wrong — nothing above, a
//! canvas-wide pass on either side, a stencil that carries its own matte, a
//! softness that is animated — is a way to either blank a frame or ask the
//! compositor for a pass it cannot nest.

use super::*;
use crate::scene::{LayerSource, SizeSpec};
use aria_models::{
    ClipParam, ColorAdjustments, Easing, Generator, LookParam, MatteSource, Param, ParamValue,
    Project, Rational, RationalTime, TimeRange, TrackKind, TrackMatte,
};

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS_24)
}

fn solid(rgba: [u8; 4]) -> Generator {
    Generator::SolidColor { rgba }
}

/// A project with `count` sticker lanes, each carrying one full-length bar, and
/// the clip ids bottom-first.
fn stack(count: usize) -> (Project, Vec<aria_models::ClipId>) {
    let mut project = Project::new("p", FPS_24);
    let mut clips = Vec::new();
    for i in 0..count {
        let track = project.add_track(TrackKind::Sticker, format!("S{}", i + 1));
        let shade = (i as u8 + 1) * 40;
        clips.push(
            project
                .add_generated(track, solid([shade, shade, shade, 255]), tr(0, 100))
                .expect("bar"),
        );
    }
    (project, clips)
}

fn luma_matte() -> TrackMatte {
    TrackMatte {
        source: MatteSource::Luma,
        invert: false,
        softness: Param::Constant(0.0),
    }
}

#[test]
fn a_matted_layer_consumes_the_layer_above_as_its_stencil() {
    let (mut project, clips) = stack(2);
    project
        .set_clip_matte(clips[0], Some(luma_matte()))
        .expect("matte");

    let scene = resolve(&project, rt(5)).unwrap();
    // Two layers in, one wrapper out: the stencil is read, never drawn.
    assert_eq!(scene.layers.len(), 1);
    let wrapper = &scene.layers[0];
    assert_eq!(wrapper.clip, Some(clips[0]));
    // The cut happens in canvas space, so the wrapper covers the whole frame.
    match wrapper.size {
        SizeSpec::Fixed([w, h]) => {
            assert_eq!((w, h), (scene.width as f32, scene.height as f32));
        }
        other => panic!("expected a canvas-sized wrapper, got {other:?}"),
    }
    match &wrapper.source {
        LayerSource::Matte {
            layers,
            read,
            invert,
            softness,
        } => {
            assert_eq!(layers[0].clip, Some(clips[0]));
            assert_eq!(layers[1].clip, Some(clips[1]));
            assert_eq!(
                (&layers[1] as *const SceneLayer as usize)
                    - (&layers[0] as *const SceneLayer as usize),
                std::mem::size_of::<SceneLayer>(),
                "the recursive matte pair must use one contiguous allocation"
            );
            assert_eq!(*read, MatteSource::Luma);
            assert!(!*invert);
            assert_eq!(*softness, 0.0);
        }
        other => panic!("expected a matte wrapper, got {other:?}"),
    }
}

#[test]
fn a_matted_layer_with_nothing_above_draws_unmatted() {
    let (mut project, clips) = stack(1);
    project
        .set_clip_matte(clips[0], Some(luma_matte()))
        .expect("matte");

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(
        matches!(scene.layers[0].source, LayerSource::Solid(_)),
        "a stack still being built must keep drawing, not blank the frame"
    );
}

#[test]
fn a_stencil_does_not_chain_its_own_matte() {
    let (mut project, clips) = stack(3);
    // Bottom and middle both ask to be matted; the middle one is consumed as a
    // stencil, so its own request is dropped rather than eating the top bar.
    for clip in &clips[..2] {
        project
            .set_clip_matte(*clip, Some(luma_matte()))
            .expect("matte");
    }

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    match &scene.layers[0].source {
        LayerSource::Matte { layers, .. } => {
            assert_eq!(layers[0].clip, Some(clips[0]));
            assert_eq!(layers[1].clip, Some(clips[1]));
        }
        other => panic!("expected a matte wrapper, got {other:?}"),
    }
    assert_eq!(scene.layers[1].clip, Some(clips[2]));
    assert!(matches!(scene.layers[1].source, LayerSource::Solid(_)));
}

#[test]
fn a_canvas_pass_is_never_folded_into_a_matte() {
    // An adjustment bar is a canvas-wide pass: it has no picture of its own to
    // read or to cut, and pairing one would ask the compositor to nest a pass.
    for (matted_is_pass, lanes) in [
        (false, [TrackKind::Sticker, TrackKind::Adjustment]),
        (true, [TrackKind::Adjustment, TrackKind::Sticker]),
    ] {
        let mut project = Project::new("p", FPS_24);
        let mut clips = Vec::new();
        for (i, kind) in lanes.iter().enumerate() {
            let track = project.add_track(*kind, format!("L{}", i + 1));
            let generator = match kind {
                TrackKind::Adjustment => Generator::Adjustment,
                _ => solid([200, 200, 200, 255]),
            };
            let clip = project
                .add_generated(track, generator, tr(0, 100))
                .expect("bar");
            // A bare adjustment bar carries nothing, so resolve emits no pass
            // for it at all. Give it a grade so the canvas pass is real.
            if *kind == TrackKind::Adjustment {
                project
                    .set_clip_adjustments(
                        clip,
                        ColorAdjustments {
                            saturation: (-1.0).into(),
                            ..ColorAdjustments::default()
                        },
                    )
                    .expect("grade");
            }
            clips.push(clip);
        }
        project
            .set_clip_matte(clips[0], Some(luma_matte()))
            .expect("matte");

        let scene = resolve(&project, rt(5)).unwrap();
        assert_eq!(
            scene.layers.len(),
            2,
            "canvas pass on the {} side must stay unfolded",
            if matted_is_pass { "matted" } else { "stencil" }
        );
        for layer in &scene.layers {
            assert!(
                !matches!(layer.source, LayerSource::Matte { .. }),
                "a canvas pass must never be matted"
            );
        }
    }
}

#[test]
fn matte_softness_is_sampled_at_the_clip_local_tick() {
    let mut project = Project::new("p", FPS_24);
    let bottom = project.add_track(TrackKind::Sticker, "S1");
    let top = project.add_track(TrackKind::Sticker, "S2");
    // The matted bar starts at 24, so a timeline tick of 36 is a clip-local 12.
    let content = project
        .add_generated(bottom, solid([10, 10, 10, 255]), tr(24, 100))
        .expect("bar");
    project
        .add_generated(top, solid([20, 20, 20, 255]), tr(24, 100))
        .expect("bar");
    project
        .set_clip_matte(content, Some(luma_matte()))
        .expect("matte");
    let softness = ClipParam::Look {
        param: LookParam::MatteSoftness,
    };
    project
        .set_param_keyframe(
            content,
            softness,
            rt(24),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .expect("keyframe");
    project
        .set_param_keyframe(
            content,
            softness,
            rt(48),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .expect("keyframe");

    let read_softness = |at: i64| -> f32 {
        let scene = resolve(&project, rt(at)).unwrap();
        match &scene.layers[0].source {
            LayerSource::Matte { softness, .. } => *softness,
            other => panic!("expected a matte wrapper at {at}, got {other:?}"),
        }
    };
    assert!(read_softness(24).abs() < 1e-3);
    assert!((read_softness(36) - 0.5).abs() < 1e-3);
    assert!((read_softness(48) - 1.0).abs() < 1e-3);
}

#[test]
fn the_read_channel_and_invert_survive_the_fold() {
    for (source, invert) in [
        (MatteSource::Alpha, false),
        (MatteSource::Alpha, true),
        (MatteSource::Luma, true),
    ] {
        let (mut project, clips) = stack(2);
        project
            .set_clip_matte(
                clips[0],
                Some(TrackMatte {
                    source,
                    invert,
                    softness: Param::Constant(0.25),
                }),
            )
            .expect("matte");

        let scene = resolve(&project, rt(5)).unwrap();
        match &scene.layers[0].source {
            LayerSource::Matte {
                read,
                invert: folded,
                softness,
                ..
            } => {
                assert_eq!(*read, source);
                assert_eq!(*folded, invert);
                assert!((softness - 0.25).abs() < 1e-3);
            }
            other => panic!("expected a matte wrapper, got {other:?}"),
        }
    }
}

#[test]
fn a_matte_wrapper_keeps_the_content_blend_mode() {
    let (mut project, clips) = stack(2);
    project
        .set_blend_mode(clips[0], aria_models::BlendMode::Screen)
        .expect("blend");
    project
        .set_clip_matte(clips[0], Some(luma_matte()))
        .expect("matte");

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers[0].blend_mode, aria_models::BlendMode::Screen);
}
