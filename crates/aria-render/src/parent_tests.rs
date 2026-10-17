//! A parented clip drawn in its parent's frame.
//!
//! Closes the class where a placement path reads the child's own transform
//! and skips the parent: the frame, a gesture in progress on the parent, and
//! the motion blur samples. Also pins the edges: before and after the parent's
//! span the child is drawn by its own transform.
//!
//! Not covered: the math of the frame, which `aria-models` tests.

use super::*;
use crate::motion_blur::attach_motion_blur_passes;
use crate::resolve::{ResolveOverrides, resolve, resolve_gesture_partitions, resolve_with};
use crate::scene::{Scene, SceneLayer, SizeSpec};
use aria_core::RationalTime;
use aria_models::{
    ClipParam, Easing, Generator, MotionBlur, Param, ParamValue, Scale2, TimeRange, TrackKind,
};

const FPS: Rational = Rational::FPS_24;

fn solid(project: &mut Project, start: i64, duration: i64) -> ClipId {
    let track = project.add_track(TrackKind::Sticker, "S");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 255, 255, 255],
            },
            TimeRange::at_rate(start, duration, FPS),
        )
        .unwrap()
}

fn posed(position: [f32; 2], rotation: f32, scale: f32) -> ClipTransform {
    ClipTransform {
        position,
        rotation,
        scale: Scale2 { x: scale, y: scale },
        ..ClipTransform::IDENTITY
    }
}

/// A parent over ticks 24..72, a quarter turn clockwise at twice the size a
/// quarter canvas right of center; a child over 0..96 a tenth of the canvas
/// right of its anchor, turned 10 degrees.
fn parented() -> (Project, ClipId, ClipId) {
    let mut project = Project::new("parent", FPS);
    let parent = solid(&mut project, 24, 48);
    let child = solid(&mut project, 0, 96);
    project.clip_mut(parent).unwrap().transform = posed([0.25, 0.0], 90.0, 2.0).into();
    let own = posed([0.1, 0.0], 10.0, 1.0).into();
    project.set_clip_parent(child, Some(parent), own).unwrap();
    (project, parent, child)
}

fn layer(scene: &Scene, clip: ClipId) -> &SceneLayer {
    scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(clip))
        .expect("the clip draws")
}

fn close2(a: [f32; 2], b: [f32; 2]) {
    assert!(
        (a[0] - b[0]).abs() < 1e-2 && (a[1] - b[1]).abs() < 1e-2,
        "expected {b:?}, got {a:?}"
    );
}

#[test]
fn a_child_is_drawn_in_its_parent_s_frame_while_the_parent_covers_it() {
    let (project, _, child) = parented();
    let scene = resolve(&project, RationalTime::new(30, FPS)).unwrap();
    assert_eq!((scene.width, scene.height), (1920, 1080));
    let drawn = layer(&scene, child);
    // 192 px right of the parent's anchor, turned a quarter and doubled.
    close2(drawn.center, [1440.0, 540.0 + 384.0]);
    assert!((drawn.rotation - 100f32.to_radians()).abs() < 1e-4);
    let SizeSpec::Fixed(size) = drawn.size else {
        panic!("a solid is drawn at a fixed size");
    };

    // Outside the parent's span the child is placed by its own transform
    // alone, at half the size.
    for tick in [0, 23, 72, 95] {
        let scene = resolve(&project, RationalTime::new(tick, FPS)).unwrap();
        let alone = layer(&scene, child);
        close2(alone.center, [960.0 + 192.0, 540.0]);
        assert!(
            (alone.rotation - 10f32.to_radians()).abs() < 1e-4,
            "at {tick}"
        );
        let SizeSpec::Fixed(alone_size) = alone.size else {
            panic!("a solid is drawn at a fixed size");
        };
        close2([alone_size[0] * 2.0, alone_size[1] * 2.0], size);
    }
}

#[test]
fn a_gesture_on_the_parent_carries_the_child() {
    let (project, parent, child) = parented();
    let overrides = ResolveOverrides {
        transform: Some((parent, posed([-0.25, 0.0], 0.0, 1.0))),
        ..ResolveOverrides::default()
    };
    let scene = resolve_with(&project, RationalTime::new(30, FPS), overrides).unwrap();
    close2(layer(&scene, parent).center, [480.0, 540.0]);
    close2(layer(&scene, child).center, [480.0 + 192.0, 540.0]);
}

/// The child does not move on its own; its parent sweeps right a hundredth of
/// the canvas per tick, so the child's blur spans the parent's travel over
/// the shutter.
#[test]
fn a_still_child_of_a_moving_parent_is_blurred_by_the_parent_s_motion() {
    let mut project = Project::new("blur", FPS);
    let parent = solid(&mut project, 0, 96);
    let child = solid(&mut project, 0, 96);
    for (at, x) in [(0, 0.0), (48, 0.48)] {
        project
            .set_param_keyframe(
                parent,
                ClipParam::Position,
                RationalTime::new(at, FPS),
                ParamValue::Vec2([x, 0.0]),
                Easing::Linear,
                None,
            )
            .unwrap();
    }
    let own = posed([0.1, 0.0], 0.0, 1.0).into();
    project.set_clip_parent(child, Some(parent), own).unwrap();
    project
        .set_motion_blur(
            child,
            MotionBlur {
                enabled: true,
                shutter_deg: 180.0,
                samples: 4,
            },
        )
        .unwrap();
    assert!(matches!(
        project.clip(child).unwrap().transform.position,
        Param::Constant(_)
    ));
    let mut scene = resolve(&project, RationalTime::new(24, FPS)).unwrap();
    attach_motion_blur_passes(&project, &mut scene);
    let passes = &layer(&scene, child).blur_passes;
    assert_eq!(passes.len(), 4);
    // At tick 24 the parent's anchor is 0.24 of the canvas right of center;
    // the shutter spans a quarter tick each way, 4.8 px.
    let center = 1920.0 * (0.5 + 0.24) + 192.0;
    close2(passes[0].center, [center - 4.8, 540.0]);
    close2(passes[3].center, [center + 4.8, 540.0]);
}

/// A gesture sprite is drawn at identity and moved alone, so neither a parent
/// (whose children move with it) nor a child (whose sprite sits in another
/// frame) may be split into one; an unrelated clip still is.
#[test]
fn a_gesture_on_a_parent_or_a_child_is_drawn_whole() {
    let (mut project, parent, child) = parented();
    let other = solid(&mut project, 0, 96);
    let at = RationalTime::new(30, FPS);
    for clip in [parent, child] {
        assert!(
            resolve_gesture_partitions(&project, at, clip)
                .unwrap()
                .is_none(),
            "{clip} was split into a sprite"
        );
    }
    assert!(
        resolve_gesture_partitions(&project, at, other)
            .unwrap()
            .is_some()
    );
}

fn drawn(project: &Project, clip: ClipId, tick: i64) -> (f32, [f32; 2], f32, [f32; 2]) {
    let scene = resolve(project, RationalTime::new(tick, FPS)).unwrap();
    let layer = layer(&scene, clip);
    let SizeSpec::Fixed(size) = layer.size else {
        panic!("a solid is drawn at a fixed size");
    };
    (tick as f32, layer.center, layer.rotation, size)
}

fn same_place(a: (f32, [f32; 2], f32, [f32; 2]), b: (f32, [f32; 2], f32, [f32; 2])) {
    close2(a.1, b.1);
    assert!(
        (a.2 - b.2).abs() < 1e-4,
        "at {}: {} became {}",
        a.0,
        a.2,
        b.2
    );
    close2(a.3, b.3);
}

/// Parenting and releasing keep the clip where it is drawn at the kept
/// instant: `at` itself when the frame that changes covers it, else the
/// nearest instant it covers.
#[test]
fn parenting_and_releasing_keep_the_clip_in_place() {
    let mut project = Project::new("keep", FPS);
    let parent = solid(&mut project, 24, 48);
    let child = solid(&mut project, 0, 96);
    project.clip_mut(parent).unwrap().transform = posed([0.25, -0.1], 90.0, 2.0).into();
    project.clip_mut(child).unwrap().transform = posed([-0.2, 0.15], 30.0, 0.5).into();
    for (at, kept) in [(30, 30), (0, 24), (90, 71)] {
        let mut project = project.clone();
        let before = drawn(&project, child, kept);
        let to = reparented(
            &project,
            child,
            Some(parent),
            RationalTime::new(at, FPS),
            ParentKeep::Place,
        )
        .unwrap();
        project.set_clip_parent(child, Some(parent), to).unwrap();
        same_place(before, drawn(&project, child, kept));

        let parented = drawn(&project, child, kept);
        let back = reparented(
            &project,
            child,
            None,
            RationalTime::new(at, FPS),
            ParentKeep::Place,
        )
        .unwrap();
        project.set_clip_parent(child, None, back).unwrap();
        same_place(parented, drawn(&project, child, kept));
    }
}

#[test]
fn snapping_puts_the_clip_on_the_parent_s_anchor() {
    let (project, parent, child) = parented();
    let snapped = reparented(
        &project,
        child,
        Some(parent),
        RationalTime::new(30, FPS),
        ParentKeep::Snap,
    )
    .unwrap();
    let mut project = project;
    project
        .set_clip_parent(child, Some(parent), snapped)
        .unwrap();
    let (_, center, rotation, size) = drawn(&project, child, 30);
    let (_, parent_center, parent_rotation, parent_size) = drawn(&project, parent, 30);
    close2(center, parent_center);
    assert!((rotation - parent_rotation).abs() < 1e-4);
    close2(size, parent_size);
}

#[test]
fn a_parent_that_may_not_parent_the_clip_is_refused() {
    let (project, parent, child) = parented();
    let at = RationalTime::new(30, FPS);
    assert_eq!(
        reparented(&project, parent, Some(child), at, ParentKeep::Place),
        Err(ModelError::ParentCycle(parent))
    );
    let mut flat = project;
    flat.clip_mut(parent).unwrap().transform = posed([0.0, 0.0], 0.0, 1e-9).into();
    flat.set_clip_parent(child, None, AnimatedTransform::identity())
        .unwrap();
    assert_eq!(
        reparented(&flat, child, Some(parent), at, ParentKeep::Place),
        Err(ModelError::ParentFlattened(parent))
    );
}
