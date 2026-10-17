//! Motion blur inside a nested sequence: a clip of the sequence samples its
//! shutter on the sequence's frame within the nest's canvas.
//!
//! The class this closes: samples placed on the nest canvas as if the
//! sequence filled it, which put the blur of a sequence of another shape
//! beside the clip it blurs. What this does not catch: the blur's pixels.

use super::*;
use aria_models::{
    CanvasSettings, ClipParam, Easing, Generator, MotionBlur, ParamValue, Project, Rational,
    RationalTime, TimeRange, TrackKind,
};

const FPS: Rational = Rational::FPS_30;

fn sized(size: [u32; 2]) -> CanvasSettings {
    CanvasSettings {
        resolution: Some(size),
        ..CanvasSettings::default()
    }
}

/// A square sequence in a wide frame: the sequence's frame starts 420 px in.
#[test]
fn a_clip_inside_a_nest_samples_on_the_sequences_frame() {
    let mut project = Project::new("mb_nest", FPS);
    project.timeline_mut().set_canvas(sized([1920, 1080]));
    let sequence = project.create_sequence("Nest", FPS);
    project.set_active_sequence(Some(sequence)).unwrap();
    project.timeline_mut().set_canvas(sized([1080, 1080]));
    let lane = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            lane,
            Generator::SolidColor {
                rgba: [255, 255, 255, 255],
            },
            TimeRange::at_rate(0, 30, FPS),
        )
        .unwrap();
    for (tick, x) in [(0, -0.4), (29, 0.4)] {
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                RationalTime::new(tick, FPS),
                ParamValue::Vec2([x, 0.0]),
                Easing::Linear,
                None,
            )
            .unwrap();
    }
    project
        .set_motion_blur(
            clip,
            MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 4,
            },
        )
        .unwrap();
    project.set_active_sequence(None).unwrap();
    let root = project.add_track(TrackKind::Video, "V1");
    project
        .add_sequence_clip(
            root,
            sequence,
            TimeRange::at_rate(0, 30, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap();

    let mut scene = crate::resolve::resolve(&project, RationalTime::new(15, FPS)).unwrap();
    attach_motion_blur_passes(&project, &mut scene);
    let LayerSource::Nest { scene: nest, .. } = &scene.layers[0].source else {
        panic!("the sequence clip places a nest");
    };
    let layer = nest.layers.iter().find(|l| l.clip == Some(clip)).unwrap();
    assert_eq!(layer.blur_passes.len(), 4);
    let n = layer.blur_passes.len() as f32;
    let mean = layer.blur_passes.iter().fold([0.0, 0.0], |sum, pass| {
        [sum[0] + pass.center[0] / n, sum[1] + pass.center[1] / n]
    });
    // One frame of this sweep is 30 px, so the shutter's samples sit within
    // a frame of the layer they blur.
    assert!(
        (mean[0] - layer.center[0]).abs() < 30.0 && (mean[1] - layer.center[1]).abs() < 1.0,
        "the samples center on {mean:?}, the layer on {:?}",
        layer.center
    );
}
