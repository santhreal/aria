//! Transform-only motion blur via temporal supersampling.
//!
//! When a clip has [`MotionBlur`](aria_models::MotionBlur) enabled and an
//! animated transform, export re-samples the clip's placement across the
//! shutter interval and the compositor averages those placements into one
//! premultiplied layer before the normal stack composite.
//!
//! ## Approximation vs true shutter integration
//!
//! Averaging happens **pre-composite** in the layer's own offscreen: edges that
//! reveal the background blend against the layer's accumulated alpha, not
//! against whatever sits below in the stack. Acceptable for CapCut-style
//! transform motion blur; whole-frame shutter integration is out of scope.
//!
//! ## Transform-only (not source-motion)
//!
//! The layer's texture stays the frame-at-T content — we do **not** re-decode
//! video at sub-ticks. This matches After Effects' transform motion blur;
//! source-motion / optical-flow blur is out of scope.
//!
//! ## Cost
//!
//! Per blurred layer: **N** extra placement draws + N weighted additive blits
//! (N clamped to `2..=16`), then one normal composite of the average. Layers
//! without motion blur pay nothing.

use aria_compositor::LayerPlacement;
use aria_core::{Rational, RationalTime};
use aria_models::{Clip, ClipParam, ClipTransform, MAX_PARENT_DEPTH, Project};

use crate::parent::{own_pose, world_pose};
use crate::scene::{LayerSource, Scene, SceneBlurPass, SceneLayer, SizeSpec};

/// Populate [`SceneLayer::blur_passes`](crate::scene::SceneLayer::blur_passes)
/// for every clip that opts into motion blur with an animated transform,
/// including each side of a transition and of a track matte, and every layer
/// of a nest.
///
/// Call **before** [`Scene::fit_within`] / [`Scene::fit_into`] so scale and
/// letterbox adjust the sample placements with the rest of the scene for both
/// [`SizeSpec::Fixed`] and [`SizeSpec::BitmapScaled`] layers.
pub fn attach_motion_blur_passes(project: &Project, scene: &mut Scene) {
    let canvas = [scene.width as f32, scene.height as f32];
    attach_scene(project, scene, Frame::whole(canvas));
}

/// Where a timeline's own canvas sits on the canvas a scene draws onto: its
/// top-left corner and its size, in that canvas's pixels.
#[derive(Clone, Copy)]
struct Frame {
    origin: [f32; 2],
    size: [f32; 2],
}

impl Frame {
    /// A timeline that fills the canvas.
    fn whole(size: [f32; 2]) -> Self {
        Self {
            origin: [0.0, 0.0],
            size,
        }
    }
}

/// Sample every layer of `scene`, whose timeline draws at `frame`.
fn attach_scene(project: &Project, scene: &mut Scene, frame: Frame) {
    let tick = scene.tick;
    for layer in scene.references.iter_mut().chain(&mut scene.layers) {
        attach_layer(project, tick, frame, layer);
    }
}

/// Sample the shutter of `layer` at `tick`, or of each layer inside it.
///
/// A wrapper takes no samples of its own: its sides are what move. A
/// transition side samples at the tick the resolver drew it at, the scene
/// tick clamped into its clip, so a side held past its clip's end blurs the
/// frame it shows. A nest samples its own layers at its scene's tick, on the
/// sequence's frame within the nest's canvas, and then moves as one layer.
fn attach_layer(project: &Project, tick: RationalTime, frame: Frame, layer: &mut SceneLayer) {
    match &mut layer.source {
        LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            for side in [outgoing, incoming] {
                let tick = side_tick(project, side, tick);
                attach_layer(project, tick, frame, side);
            }
            return;
        }
        LayerSource::Matte { layers, .. } => {
            for side in layers.iter_mut() {
                attach_layer(project, tick, frame, side);
            }
            return;
        }
        LayerSource::Nest {
            scene,
            frame: [x0, y0, x1, y1],
        } => {
            let [w, h] = [scene.width as f32, scene.height as f32];
            let inner = Frame {
                origin: [*x0 * w, *y0 * h],
                size: [(*x1 - *x0) * w, (*y1 - *y0) * h],
            };
            attach_scene(project, scene, inner);
        }
        _ => {}
    }
    if !layer.source.takes_motion_blur() {
        return;
    }
    let Some(clip) = layer.clip.and_then(|id| project.clip(id)) else {
        return;
    };
    let (SizeSpec::Fixed(base_size) | SizeSpec::BitmapScaled(base_size)) = layer.size;
    layer.blur_passes = sample_blur_passes(project, clip, tick, frame, base_size);
}

/// `tick` clamped into the clip `side` draws: the tick the resolver resolves a
/// transition side at.
fn side_tick(project: &Project, side: &SceneLayer, tick: RationalTime) -> RationalTime {
    let Some(clip) = side.clip.and_then(|id| project.clip(id)) else {
        return tick;
    };
    let first = clip.timeline.start.value;
    let last = clip.timeline.end_tick() - 1;
    RationalTime::new(tick.value.min(last).max(first), tick.rate)
}

/// Where a quad `size` pixels across, placed at the pose of `layer`, draws at
/// each of the layer's motion-blur samples: one placement per entry in
/// [`SceneLayer::blur_passes`](crate::scene::SceneLayer::blur_passes), none
/// when it has none.
///
/// A sample's size is in the layer's own size units, pixels for a
/// [`SizeSpec::Fixed`] layer and a scale for a [`SizeSpec::BitmapScaled`] one.
/// Each placement scales `size` by the sample's ratio to the layer's size and
/// keeps the layer's anchor at the sample's position, so the realized bitmap
/// of a text or path layer, and a mask stencil sized apart from its layer,
/// both follow the layer.
pub fn blur_placements(layer: &SceneLayer, size: [f32; 2]) -> Vec<LayerPlacement> {
    let (SizeSpec::Fixed(base) | SizeSpec::BitmapScaled(base)) = layer.size;
    if base[0] <= 0.0 || base[1] <= 0.0 {
        return Vec::new();
    }
    layer
        .blur_passes
        .iter()
        .map(|pass| {
            let sample = SceneBlurPass {
                size: [
                    size[0] * pass.size[0] / base[0],
                    size[1] * pass.size[1] / base[1],
                ],
                ..*pass
            };
            placement_from_blur_pass(&sample, layer.anchor_point)
        })
        .collect()
}

fn sample_blur_passes(
    project: &Project,
    clip: &Clip,
    t: RationalTime,
    frame: Frame,
    base_size: [f32; 2],
) -> Vec<SceneBlurPass> {
    let Frame {
        origin: [ox, oy],
        size: [cw, ch],
    } = frame;
    let mb = clip.motion_blur;
    if !mb.enabled || mb.shutter_deg <= 0.0 || !transform_moves(project, clip) {
        return Vec::new();
    }
    let n = mb.samples.clamp(2, 16);
    // Export steps one timeline tick per output frame at the sampling rate.
    let shutter_span = f64::from(mb.shutter_deg) / 360.0;

    let xf0 = transform_at(project, clip, t.value as f64, t.rate, [cw, ch]);
    let sx = xf0.scale.x.abs();
    let sy = xf0.scale.y.abs();
    if sx < 1e-4 || sy < 1e-4 || base_size[0] <= 0.0 || base_size[1] <= 0.0 {
        return Vec::new();
    }
    let natural = [base_size[0] / sx, base_size[1] / sy];
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let u = i as f64 / f64::from(n - 1);
        let sample_t = t.value as f64 + shutter_span * (u - 0.5);
        let xf = transform_at(project, clip, sample_t, t.rate, [cw, ch]);
        let size = [natural[0] * xf.scale.x, natural[1] * xf.scale.y];
        out.push(SceneBlurPass {
            center: [
                ox + cw * (0.5 + xf.position[0]),
                oy + ch * (0.5 + xf.position[1]),
            ],
            size,
            rotation: xf.rotation.to_radians(),
            opacity: xf.opacity.clamp(0.0, 1.0),
        });
    }
    out
}

fn placement_from_blur_pass(pass: &SceneBlurPass, anchor_point: [f32; 2]) -> LayerPlacement {
    let to_center = [
        (0.5 - anchor_point[0]) * pass.size[0],
        (0.5 - anchor_point[1]) * pass.size[1],
    ];
    let center = if to_center == [0.0, 0.0] {
        pass.center
    } else {
        let (sin, cos) = pass.rotation.sin_cos();
        [
            pass.center[0] + to_center[0] * cos - to_center[1] * sin,
            pass.center[1] + to_center[0] * sin + to_center[1] * cos,
        ]
    };
    LayerPlacement {
        center,
        size: pass.size,
        rotation: pass.rotation,
        opacity: pass.opacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_models::{
        ClipParam, Easing, Generator, MotionBlur, ParamValue, Project, Rational, RationalTime,
        TimeRange, TrackKind,
    };

    const FPS: Rational = Rational::FPS_30;

    #[test]
    fn animated_clip_with_blur_gets_subframe_passes() {
        let mut project = Project::new("mb", FPS);
        let track = project.add_track(TrackKind::Sticker, "S1");
        let clip = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                },
                TimeRange::at_rate(0, 30, FPS),
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                RationalTime::new(0, FPS),
                ParamValue::Vec2([-0.4, 0.0]),
                Easing::Linear,
                None,
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                RationalTime::new(29, FPS),
                ParamValue::Vec2([0.4, 0.0]),
                Easing::Linear,
                None,
            )
            .unwrap();
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

        let mut scene = crate::resolve::resolve(&project, RationalTime::new(15, FPS)).unwrap();
        attach_motion_blur_passes(&project, &mut scene);
        let layer = scene.layers.iter().find(|l| l.clip == Some(clip)).unwrap();
        assert_eq!(layer.blur_passes.len(), 4);
        // Horizontal sweep: first sample left of last.
        assert!(layer.blur_passes[0].center[0] < layer.blur_passes[3].center[0]);
    }

    /// Two abutting solids sweeping left to right, each with motion blur, and
    /// a crossfade over the cut at tick 30 whose window is `[20, 40)`.
    fn blurred_crossfade() -> Project {
        let mut project = Project::new("mb_sides", FPS);
        let track = project.add_track(TrackKind::Sticker, "S1");
        let mut clips = Vec::new();
        for (start, rgba) in [(0, [255, 255, 255, 255]), (30, [0, 0, 0, 255])] {
            let clip = project
                .add_generated(
                    track,
                    Generator::SolidColor { rgba },
                    TimeRange::at_rate(start, 30, FPS),
                )
                .unwrap();
            for (tick, x) in [(start, -0.4), (start + 29, 0.4)] {
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
            clips.push(clip);
        }
        project.add_transition(clips[0], "crossfade").unwrap();
        project.set_transition_duration(clips[0], 20).unwrap();
        project
    }

    /// The outgoing and incoming layers of the transition drawn at `tick`.
    fn sides_at(project: &Project, tick: i64) -> [SceneLayer; 2] {
        let mut scene = crate::resolve::resolve(project, RationalTime::new(tick, FPS)).unwrap();
        attach_motion_blur_passes(project, &mut scene);
        let layer = scene.layers.remove(0);
        let LayerSource::Transition {
            outgoing, incoming, ..
        } = layer.source
        else {
            panic!("tick {tick} is inside the crossfade window");
        };
        [*outgoing, *incoming]
    }

    #[test]
    fn each_side_of_a_transition_takes_its_own_clips_samples() {
        let project = blurred_crossfade();
        let [outgoing, incoming] = sides_at(&project, 25);
        assert_eq!(outgoing.blur_passes.len(), 4);
        assert_eq!(incoming.blur_passes.len(), 4);
        assert!(outgoing.blur_passes[0].center[0] < outgoing.blur_passes[3].center[0]);
    }

    /// Past the cut the outgoing clip is held at its last frame, tick 29, and
    /// its samples are that frame's: sampled at the scene tick, they would
    /// straddle a hold and draw a still picture with no smear.
    #[test]
    fn a_held_transition_side_blurs_the_frame_it_shows() {
        let project = blurred_crossfade();
        let [held, _] = sides_at(&project, 33);
        let [last, _] = sides_at(&project, 29);
        assert_eq!(held.center, last.center, "the side holds its last frame");
        assert_eq!(held.blur_passes, last.blur_passes);
        assert!(held.blur_passes[0].center[0] < held.blur_passes[3].center[0]);
    }

    #[test]
    fn each_layer_of_a_track_matte_takes_its_own_clips_samples() {
        let mut project = Project::new("mb_matte", FPS);
        let below = project.add_track(TrackKind::Sticker, "S1");
        let above = project.add_track(TrackKind::Sticker, "S2");
        let mut clips = Vec::new();
        for track in [below, above] {
            let clip = project
                .add_generated(
                    track,
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
                        samples: 3,
                    },
                )
                .unwrap();
            clips.push(clip);
        }
        project
            .set_clip_matte(clips[0], Some(aria_models::TrackMatte::alpha()))
            .unwrap();
        let mut scene = crate::resolve::resolve(&project, RationalTime::new(15, FPS)).unwrap();
        attach_motion_blur_passes(&project, &mut scene);
        let [wrapper] = scene.layers.as_slice() else {
            panic!(
                "the stencil folds into one matte layer: {:?}",
                scene.layers.len()
            );
        };
        let LayerSource::Matte { layers, .. } = &wrapper.source else {
            panic!("the matted clip draws as a matte");
        };
        assert!(wrapper.blur_passes.is_empty(), "the wrapper does not move");
        assert_eq!(layers[0].blur_passes.len(), 3, "content");
        assert_eq!(layers[1].blur_passes.len(), 3, "stencil");
    }

    #[test]
    fn static_transform_yields_no_passes() {
        let mut project = Project::new("mb", FPS);
        let track = project.add_track(TrackKind::Sticker, "S1");
        let clip = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                },
                TimeRange::at_rate(0, 30, FPS),
            )
            .unwrap();
        project
            .set_motion_blur(
                clip,
                MotionBlur {
                    enabled: true,
                    shutter_deg: 180.0,
                    samples: 8,
                },
            )
            .unwrap();
        let mut scene = crate::resolve::resolve(&project, RationalTime::new(10, FPS)).unwrap();
        attach_motion_blur_passes(&project, &mut scene);
        let layer = scene.layers.iter().find(|l| l.clip == Some(clip)).unwrap();
        assert!(layer.blur_passes.is_empty());
    }

    #[test]
    fn letterboxed_fixed_and_bitmap_scaled_resolve_identical_blur_timing_and_placements() {
        let mut project = Project::new("mb_letterbox", FPS);
        let track = project.add_track(TrackKind::Sticker, "S1");
        let clip = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                },
                TimeRange::at_rate(0, 30, FPS),
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                RationalTime::new(0, FPS),
                ParamValue::Vec2([-0.3, -0.2]),
                Easing::Linear,
                None,
            )
            .unwrap();
        project
            .set_param_keyframe(
                clip,
                ClipParam::Position,
                RationalTime::new(29, FPS),
                ParamValue::Vec2([0.3, 0.2]),
                Easing::Linear,
                None,
            )
            .unwrap();
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

        let mut scene = crate::resolve::resolve(&project, RationalTime::new(15, FPS)).unwrap();
        // Add a second layer with BitmapScaled size referencing the same clip
        let fixed_layer = scene
            .layers
            .iter()
            .find(|l| l.clip == Some(clip))
            .unwrap()
            .clone();
        let mut bitmap_layer = fixed_layer.clone();
        bitmap_layer.size = SizeSpec::BitmapScaled([1.0, 1.0]);
        scene.layers.push(bitmap_layer);

        attach_motion_blur_passes(&project, &mut scene);
        assert_eq!(scene.layers[0].blur_passes.len(), 4);
        assert_eq!(scene.layers[1].blur_passes.len(), 4);

        // Letterbox the scene into a square canvas (1920x1080 into 400x400)
        scene.fit_into(400, 400);

        let l_fixed = &scene.layers[0];
        let l_bitmap = &scene.layers[1];

        for i in 0..4 {
            let p_fixed = &l_fixed.blur_passes[i];
            let p_bitmap = &l_bitmap.blur_passes[i];

            // Centers must resolve identically under letterboxing
            assert!(
                (p_fixed.center[0] - p_bitmap.center[0]).abs() < 1e-4,
                "sample {i} center X mismatch: fixed={}, bitmap={}",
                p_fixed.center[0],
                p_bitmap.center[0]
            );
            assert!(
                (p_fixed.center[1] - p_bitmap.center[1]).abs() < 1e-4,
                "sample {i} center Y mismatch: fixed={}, bitmap={}",
                p_fixed.center[1],
                p_bitmap.center[1]
            );
            assert_eq!(p_fixed.rotation, p_bitmap.rotation);
            assert_eq!(p_fixed.opacity, p_bitmap.opacity);
        }

        // A bitmap realized at the fixed layer's size draws where the fixed
        // layer draws.
        let SizeSpec::Fixed(image) = fixed_layer.size else {
            panic!("expected fixed size");
        };
        let SizeSpec::Fixed(quad) = l_fixed.size else {
            panic!("expected fixed size");
        };
        let SizeSpec::BitmapScaled(scale) = l_bitmap.size else {
            panic!("expected a bitmap scale");
        };
        let fixed_placements = blur_placements(l_fixed, quad);
        let bitmap_placements =
            blur_placements(l_bitmap, [image[0] * scale[0], image[1] * scale[1]]);

        assert_eq!(fixed_placements.len(), 4);
        assert_eq!(bitmap_placements.len(), 4);
        for i in 0..4 {
            assert!(
                (fixed_placements[i].center[0] - bitmap_placements[i].center[0]).abs() < 1e-4,
                "placement {i} center X mismatch"
            );
            assert!(
                (fixed_placements[i].center[1] - bitmap_placements[i].center[1]).abs() < 1e-4,
                "placement {i} center Y mismatch"
            );
            assert!(
                (fixed_placements[i].size[0] - bitmap_placements[i].size[0]).abs() < 1e-4,
                "placement {i} size X mismatch"
            );
            assert!(
                (fixed_placements[i].size[1] - bitmap_placements[i].size[1]).abs() < 1e-4,
                "placement {i} size Y mismatch"
            );
        }
    }
}

/// Whether `clip`'s placement changes over time: its transform, or a parent's
/// up its chain, keyframed or driven.
fn transform_moves(project: &Project, clip: &Clip) -> bool {
    std::iter::successors(Some(clip), |clip| {
        clip.parent.and_then(|id| project.clip(id))
    })
    .take(MAX_PARENT_DEPTH + 1)
    .any(own_transform_moves)
}

/// Whether `clip`'s own transform changes over time: keyframed, or driven.
fn own_transform_moves(clip: &Clip) -> bool {
    clip.transform.is_animated()
        || clip.drivers.iter().any(|driver| {
            matches!(
                driver.param,
                ClipParam::Position
                    | ClipParam::AnchorPoint
                    | ClipParam::Scale
                    | ClipParam::Rotation
                    | ClipParam::Opacity
            )
        })
}

/// `clip`'s placement at fractional timeline tick `tick` on a `canvas`-pixel
/// canvas: its own pose, after its drivers and look animations, in its
/// parents' frame.
fn transform_at(
    project: &Project,
    clip: &Clip,
    tick: f64,
    rate: Rational,
    canvas: [f32; 2],
) -> ClipTransform {
    let own = own_pose(project, clip, tick, rate);
    world_pose(project, clip, own, tick, rate, canvas, None)
}

#[cfg(test)]
#[path = "motion_blur_nest_tests.rs"]
mod nest_tests;
