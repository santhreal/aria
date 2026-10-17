//! Parenting math: placing a child in a parent's frame and taking it back out.
//!
//! The class closed here: a child that moves when it is parented, released, or
//! moved to another parent, and a frame whose turn and scale are applied in the
//! wrong order or about the wrong point. Each case is checked on a canvas that
//! is not square, because a turn measured in canvas fractions instead of pixels
//! only shows on one.
//!
//! Not covered: where the renderer reads the frame from, which
//! `aria-render`'s parenting tests drive.

use super::*;
use crate::param::{Easing, Keyframe, SpatialTangents};

const CANVAS: [f32; 2] = [1920.0, 1080.0];

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn close2(a: [f32; 2], b: [f32; 2]) -> bool {
    close(a[0], b[0]) && close(a[1], b[1])
}

fn pose(position: [f32; 2], rotation: f32, scale: [f32; 2]) -> ClipTransform {
    ClipTransform {
        position,
        anchor_point: [0.25, 0.75],
        scale: Scale2 {
            x: scale[0],
            y: scale[1],
        },
        rotation,
        opacity: 0.6,
    }
}

fn frames() -> [ParentFrame; 4] {
    [
        ParentFrame::of(&ClipTransform::IDENTITY, CANVAS),
        ParentFrame::of(&pose([0.25, 0.0], 90.0, [2.0, 2.0]), CANVAS),
        ParentFrame::of(&pose([-0.1, 0.3], -37.0, [0.5, 1.5]), CANVAS),
        ParentFrame::of(&pose([0.4, -0.45], 200.0, [1.25, 0.8]), CANVAS),
    ]
}

#[test]
fn an_identity_parent_places_the_child_where_it_was() {
    let identity = ParentFrame::of(&ClipTransform::IDENTITY, CANVAS);
    let child = pose([0.3, -0.2], 15.0, [1.2, 0.7]);
    let world = identity.compose(&child);
    assert!(close2(world.position, child.position), "{world:?}");
    assert!(close(world.rotation, child.rotation));
    assert_eq!(world.scale, child.scale);
}

/// A parent 1/4 canvas right of center, turned a quarter clockwise and doubled:
/// a child 1/10 canvas right of the anchor lands 384 px below it.
#[test]
fn a_turned_scaled_parent_carries_its_child_about_its_anchor() {
    let frame = ParentFrame::of(&pose([0.25, 0.0], 90.0, [2.0, 2.0]), CANVAS);
    let world = frame.compose(&pose([0.1, 0.0], 10.0, [0.5, 3.0]));
    assert!(
        close2(world.position, [0.25, 384.0 / 1080.0]),
        "{:?}",
        world.position
    );
    assert!(close(world.rotation, 100.0));
    assert_eq!(world.scale, Scale2 { x: 1.0, y: 6.0 });
    assert_eq!(
        world.anchor_point,
        [0.25, 0.75],
        "the anchor is the child's own"
    );
    assert!(close(world.opacity, 0.6), "the opacity is the child's own");
}

#[test]
fn taking_a_child_out_of_its_frame_undoes_placing_it() {
    let child = pose([0.3, -0.2], 15.0, [1.2, 0.7]);
    for frame in frames() {
        let back = frame
            .decompose(&frame.compose(&child))
            .expect("an invertible frame");
        assert!(close2(back.position, child.position), "{frame:?}: {back:?}");
        assert!(close(back.rotation, child.rotation), "{frame:?}: {back:?}");
        assert!(close(back.scale.x, child.scale.x) && close(back.scale.y, child.scale.y));
    }
}

#[test]
fn a_flattened_parent_cannot_take_a_child_back_out() {
    for scale in [[0.0, 1.0], [1.0, 0.0], [1e-9, 1.0]] {
        let frame = ParentFrame::of(&pose([0.0, 0.0], 0.0, scale), CANVAS);
        assert!(!frame.invertible(), "{scale:?}");
        assert_eq!(frame.decompose(&ClipTransform::IDENTITY), None, "{scale:?}");
        let mut transform = AnimatedTransform::identity();
        let before = transform.clone();
        assert_eq!(
            transform.reparent(None, Some(&frame), ClipId::from_raw(7)),
            Err(ModelError::ParentFlattened(ClipId::from_raw(7)))
        );
        assert_eq!(transform, before, "a refused reparent wrote");
    }
}

/// A keyframed child with a turn and a scale, moving along a curve when
/// `curved`, else along a line.
fn animated(curved: bool) -> AnimatedTransform {
    let mut transform = AnimatedTransform::identity();
    let curve = curved.then_some(SpatialTangents {
        out_t: [0.1, 0.05],
        in_t: [-0.05, 0.1],
    });
    transform.position = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tangents: curve,
                ..Keyframe::new(0, [-0.2, 0.1], Easing::Linear)
            },
            Keyframe {
                tangents: curve,
                ..Keyframe::new(48, [0.3, -0.2], Easing::Linear)
            },
        ],
    };
    transform.rotation = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, 0.0, Easing::Linear),
            Keyframe::new(48, 45.0, Easing::Linear),
        ],
    };
    transform.scale = Param::Constant(Scale2 { x: 1.5, y: 0.5 });
    transform
}

fn on_canvas(frame: Option<&ParentFrame>, local: [f32; 2]) -> [f32; 2] {
    frame.map_or(local, |frame| frame.place(local))
}

fn every_move() -> Vec<(Option<ParentFrame>, Option<ParentFrame>)> {
    let options: Vec<Option<ParentFrame>> = std::iter::once(None)
        .chain(frames().into_iter().map(Some))
        .collect();
    options
        .iter()
        .flat_map(|&from| options.iter().map(move |&to| (from, to)))
        .collect()
}

/// Every frame of a straight motion, turn, and scale is where it was on the
/// canvas after the child moves between frames, for every pair of frames.
#[test]
fn reparenting_keeps_every_frame_of_a_straight_motion_in_place() {
    for (from, to) in every_move() {
        let (from, to) = (from.as_ref(), to.as_ref());
        let original = animated(false);
        let mut moved = animated(false);
        moved
            .reparent(from, to, ClipId::from_raw(1))
            .expect("invertible frames");
        for tick in [0, 7, 24, 31, 48] {
            let (local_before, local_after) = (original.sample(tick), moved.sample(tick));
            let before = from.map_or(local_before, |frame| frame.compose(&local_before));
            let after = to.map_or(local_after, |frame| frame.compose(&local_after));
            assert!(
                close2(before.position, after.position),
                "{from:?} -> {to:?} at {tick}: {:?} became {:?}",
                before.position,
                after.position
            );
            assert!(close(before.rotation, after.rotation), "at {tick}");
            assert!(close(before.scale.x, after.scale.x), "at {tick}");
            assert!(close(before.scale.y, after.scale.y), "at {tick}");
        }
    }
}

/// A curved motion keeps its shape: each keyframe and both of its handles
/// land where they were on the canvas.
#[test]
fn reparenting_keeps_a_motion_path_s_keyframes_and_handles_in_place() {
    for (from, to) in every_move() {
        let (from, to) = (from.as_ref(), to.as_ref());
        let original = animated(true);
        let mut moved = animated(true);
        moved
            .reparent(from, to, ClipId::from_raw(1))
            .expect("invertible frames");
        for (old, new) in original
            .position
            .keyframes()
            .iter()
            .zip(moved.position.keyframes())
        {
            let (old_t, new_t) = (
                old.tangents.expect("curved"),
                new.tangents.expect("the handles were dropped"),
            );
            let points = |value: [f32; 2], t: SpatialTangents| {
                [
                    value,
                    [value[0] + t.out_t[0], value[1] + t.out_t[1]],
                    [value[0] + t.in_t[0], value[1] + t.in_t[1]],
                ]
            };
            for (a, b) in points(old.value, old_t)
                .into_iter()
                .zip(points(new.value, new_t))
            {
                assert!(
                    close2(on_canvas(from, a), on_canvas(to, b)),
                    "{from:?} -> {to:?} at {}: {a:?} became {b:?}",
                    old.tick
                );
            }
        }
    }
}

#[test]
fn snapping_drops_the_pose_and_keeps_the_rest() {
    let mut transform = animated(true);
    transform.opacity = Param::Constant(0.4);
    transform.anchor_point = Param::Constant([0.1, 0.2]);
    transform.snap_to_parent();
    assert_eq!(transform.position, Param::Constant([0.0, 0.0]));
    assert_eq!(transform.rotation, Param::Constant(0.0));
    assert_eq!(transform.scale, Param::Constant(Scale2::ONE));
    assert_eq!(transform.opacity, Param::Constant(0.4));
    assert_eq!(transform.anchor_point, Param::Constant([0.1, 0.2]));
}
