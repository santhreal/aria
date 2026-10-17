//! A path mask's keyframed vertices.
//!
//! WHY: the path is the one mask property whose value is a list. Its curve has
//! to move every vertex and both handles together, hold when two keyframes
//! disagree on the vertex count, and keep loading the files written before the
//! path could be keyframed. Each case below breaks one of those.
//!
//! What this does not catch: how the viewer or the renderer draws a path (the
//! UI and render suites), and any matching of vertices by position, which the
//! model does not attempt: vertex `i` of one keyframe moves to vertex `i` of the
//! next.

use std::borrow::Cow;

use super::super::{MAX_MASK_PATH_POINTS, Mask, MaskKind};
use crate::clip::ShapePathPoint;
use crate::param::{Easing, Keyframe, Param};

fn point(anchor: [f32; 2], handle_in: [f32; 2], handle_out: [f32; 2]) -> ShapePathPoint {
    ShapePathPoint {
        anchor,
        handle_in,
        handle_out,
    }
}

fn triangle(x: f32) -> Vec<ShapePathPoint> {
    vec![
        point([x, 0.1], [x - 0.05, 0.1], [x + 0.05, 0.1]),
        ShapePathPoint::corner([0.9, 0.5]),
        point([x, 0.9], [x, 0.85], [x, 0.95]),
    ]
}

fn square() -> Vec<ShapePathPoint> {
    [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]]
        .into_iter()
        .map(ShapePathPoint::corner)
        .collect()
}

/// A path mask keyed with `keys`, in tick order.
fn keyed(keys: Vec<(i64, Vec<ShapePathPoint>, Easing)>) -> Mask {
    let mut mask = Mask::from_path(square(), true);
    mask.path = Param::Keyframed {
        keyframes: keys
            .into_iter()
            .map(|(tick, points, easing)| Keyframe::new(tick, points, easing))
            .collect(),
    };
    mask
}

fn shown(mask: &Mask, tick: f64) -> Vec<ShapePathPoint> {
    mask.path_at(tick).into_owned()
}

fn ticks(mask: &Mask) -> Vec<i64> {
    mask.path.keyframes().iter().map(|key| key.tick).collect()
}

fn close(a: &[ShapePathPoint], b: &[ShapePathPoint]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            [
                (p.anchor, q.anchor),
                (p.handle_in, q.handle_in),
                (p.handle_out, q.handle_out),
            ]
            .iter()
            .all(|(u, v)| (u[0] - v[0]).abs() < 1e-5 && (u[1] - v[1]).abs() < 1e-5)
        })
}

#[test]
fn an_unkeyed_path_reads_the_same_at_every_time() {
    let mask = Mask::from_path(triangle(0.1), true);
    for tick in [-5.0, 0.0, 7.5, 1e6] {
        assert!(
            matches!(mask.path_at(tick), Cow::Borrowed(points) if points == triangle(0.1)),
            "tick {tick}"
        );
    }
}

#[test]
fn between_two_keyframes_every_vertex_and_handle_moves_along_the_easing() {
    for (easing, at_quarter) in [
        (Easing::Linear, 0.25),
        (Easing::EaseIn, 0.0625),
        (Easing::EaseOut, 0.4375),
    ] {
        let mask = keyed(vec![
            (10, triangle(0.1), easing),
            (30, triangle(0.5), Easing::Linear),
        ]);
        let x = 0.1 + 0.4 * at_quarter;
        assert!(
            close(&shown(&mask, 15.0), &triangle(x)),
            "{easing:?} at a quarter: {:?}",
            shown(&mask, 15.0)
        );
        assert!(
            close(&shown(&mask, 10.0), &triangle(0.1)),
            "{easing:?} at the first key"
        );
        assert!(
            close(&shown(&mask, 30.0), &triangle(0.5)),
            "{easing:?} at the last key"
        );
    }
}

#[test]
fn a_fractional_time_lands_between_frames() {
    let mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (2, triangle(0.3), Easing::Linear),
    ]);
    assert!(close(&shown(&mask, 0.5), &triangle(0.15)));
}

#[test]
fn before_the_first_and_after_the_last_keyframe_the_end_path_holds() {
    let mask = keyed(vec![
        (10, triangle(0.1), Easing::Linear),
        (30, triangle(0.5), Easing::Linear),
    ]);
    assert_eq!(shown(&mask, -100.0), triangle(0.1));
    assert_eq!(shown(&mask, 9.0), triangle(0.1));
    assert_eq!(shown(&mask, 31.0), triangle(0.5));
    assert_eq!(shown(&mask, 1e9), triangle(0.5));
}

#[test]
fn a_hold_keyframe_jumps_at_the_next_keyframe() {
    let mask = keyed(vec![
        (10, triangle(0.1), Easing::Hold),
        (30, triangle(0.5), Easing::Linear),
    ]);
    assert_eq!(shown(&mask, 29.0), triangle(0.1));
    assert_eq!(shown(&mask, 29.999), triangle(0.1));
    assert_eq!(shown(&mask, 30.0), triangle(0.5));
}

#[test]
fn keyframes_with_different_vertex_counts_hold_the_earlier_path_until_the_later_one() {
    let mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (10, square(), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    for tick in [0.0, 1.0, 5.0, 9.99] {
        assert_eq!(shown(&mask, tick), triangle(0.1), "tick {tick}");
    }
    for tick in [10.0, 15.0, 19.9] {
        assert_eq!(shown(&mask, tick), square(), "tick {tick}");
    }
    assert_eq!(shown(&mask, 20.0), triangle(0.5));
    assert_eq!(mask.path_at(15.0).len(), 4);
    assert!(
        matches!(mask.path_at(15.0), Cow::Borrowed(_)),
        "a held path is the stored one"
    );
}

#[test]
fn setting_the_path_writes_the_keyframe_at_that_time_or_the_path_itself() {
    let mut still = Mask::from_path(triangle(0.1), true);
    still.set_path_at(40, square());
    assert_eq!(
        still.path,
        Param::Constant(square()),
        "an unkeyed path is replaced"
    );

    let mut mask = keyed(vec![
        (10, triangle(0.1), Easing::EaseIn),
        (30, triangle(0.5), Easing::Linear),
    ]);
    mask.set_path_at(10, triangle(0.2));
    assert_eq!(ticks(&mask), [10, 30]);
    let first = &mask.path.keyframes()[0];
    assert_eq!(first.value, triangle(0.2));
    assert_eq!(
        first.easing,
        Easing::EaseIn,
        "the keyframe keeps its easing"
    );

    mask.set_path_at(20, triangle(0.9));
    mask.set_path_at(-4, triangle(0.0));
    mask.set_path_at(50, triangle(0.7));
    assert_eq!(
        ticks(&mask),
        [-4, 10, 20, 30, 50],
        "new keyframes land in time order"
    );
    assert_eq!(
        mask.path.keyframe_at(20).map(|key| key.value.clone()),
        Some(triangle(0.9))
    );
    assert!(mask.validate().is_ok());
}

#[test]
fn keying_the_path_records_the_path_shown_at_that_time() {
    let mut mask = Mask::from_path(triangle(0.1), true);
    mask.key_path(12);
    assert_eq!(
        mask.path,
        Param::Keyframed {
            keyframes: vec![Keyframe::new(12, triangle(0.1), Easing::Linear)]
        },
        "the first key holds the path as it was"
    );

    let mut mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    mask.key_path(5);
    assert_eq!(ticks(&mask), [0, 5, 20]);
    assert!(close(&mask.path.keyframes()[1].value, &triangle(0.2)));
    for tick in [0.0, 2.5, 5.0, 12.0, 20.0] {
        let before = keyed(vec![
            (0, triangle(0.1), Easing::Linear),
            (20, triangle(0.5), Easing::Linear),
        ]);
        assert!(
            close(&shown(&mask, tick), &shown(&before, tick)),
            "keying at 5 changed the path at {tick}"
        );
    }

    mask.set_path_at(5, square());
    mask.key_path(5);
    assert_eq!(
        mask.path.keyframe_at(5).map(|key| key.value.clone()),
        Some(square()),
        "keying over a keyframe keeps it"
    );
}

#[test]
fn removing_path_keyframes_ends_at_an_unkeyed_path() {
    let mut mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    assert!(!mask.unkey_path(7), "no keyframe at 7");
    assert_eq!(ticks(&mask), [0, 20]);
    assert!(mask.unkey_path(0));
    assert_eq!(ticks(&mask), [20]);
    assert!(mask.unkey_path(20));
    assert_eq!(
        mask.path,
        Param::Constant(triangle(0.5)),
        "the last keyframe's path stays"
    );
    assert!(
        !mask.unkey_path(20),
        "an unkeyed path has no keyframe to remove"
    );
    assert!(mask.validate().is_ok());
}

#[test]
fn holding_the_path_keeps_the_shape_shown_at_that_time() {
    let mut mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    mask.hold_path(10);
    let Param::Constant(points) = &mask.path else {
        panic!("still keyed: {:?}", mask.path);
    };
    assert!(close(points, &triangle(0.3)));

    let mut still = Mask::from_path(square(), true);
    still.hold_path(10);
    assert_eq!(still.path, Param::Constant(square()));
}

#[test]
fn the_easing_belongs_to_the_keyframe_at_that_time() {
    let mut mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    assert!(mask.set_path_easing(0, Easing::Hold));
    assert!(
        !mask.set_path_easing(0, Easing::Hold),
        "the keyframe already holds"
    );
    assert!(!mask.set_path_easing(5, Easing::Hold), "no keyframe at 5");
    assert_eq!(mask.path.easing_at(0), Some(Easing::Hold));
    assert_eq!(mask.path.easing_at(20), Some(Easing::Linear));
    assert_eq!(shown(&mask, 19.0), triangle(0.1));

    let mut still = Mask::from_path(square(), true);
    assert!(!still.set_path_easing(0, Easing::Hold));
}

#[test]
fn moving_a_path_keyframe_takes_its_path_and_easing_and_replaces_one_at_the_target() {
    let start = || {
        keyed(vec![
            (0, triangle(0.1), Easing::Hold),
            (10, square(), Easing::EaseIn),
            (20, triangle(0.5), Easing::Linear),
        ])
    };

    let mut mask = start();
    assert!(mask.move_path_key(0, 15), "past a later keyframe");
    assert_eq!(ticks(&mask), [10, 15, 20], "the curve stays in time order");
    assert_eq!(
        mask.path.keyframe_at(15).map(|key| key.value.clone()),
        Some(triangle(0.1))
    );
    assert_eq!(mask.path.easing_at(15), Some(Easing::Hold));
    assert!(mask.validate().is_ok());

    let mut mask = start();
    assert!(mask.move_path_key(20, 10), "onto another keyframe");
    assert_eq!(ticks(&mask), [0, 10]);
    assert_eq!(
        mask.path.keyframe_at(10).map(|key| key.value.clone()),
        Some(triangle(0.5)),
        "the keyframe at the target gave way"
    );
    assert_eq!(mask.path.easing_at(10), Some(Easing::Linear));

    let mut mask = start();
    assert!(!mask.move_path_key(10, 10), "in place is no move");
    assert_eq!(mask, start());
    assert!(!mask.move_path_key(5, 12), "no keyframe at 5");
    assert_eq!(mask, start());

    let mut still = Mask::from_path(square(), true);
    assert!(
        !still.move_path_key(0, 4),
        "an unkeyed path has no keyframe"
    );
    assert_eq!(still.path, Param::Constant(square()));
}

#[test]
fn a_vertex_edit_reaches_every_keyframe() {
    let mut mask = keyed(vec![
        (0, triangle(0.1), Easing::Linear),
        (10, triangle(0.3), Easing::Linear),
        (20, triangle(0.5), Easing::Linear),
    ]);
    mask.edit_every_path(|points| {
        points.insert(1, ShapePathPoint::corner([0.5, 0.0]));
    });
    for key in mask.path.keyframes() {
        assert_eq!(key.value.len(), 4, "keyframe at {}", key.tick);
        assert_eq!(key.value[1], ShapePathPoint::corner([0.5, 0.0]));
    }
    assert!(
        close(&shown(&mask, 5.0)[..1], &triangle(0.2)[..1]),
        "the path still moves"
    );

    let mut still = Mask::from_path(triangle(0.1), true);
    still.edit_every_path(|points| {
        points.remove(0);
    });
    assert_eq!(still.path_at(0.0).len(), 2);
}

#[test]
fn a_mask_has_a_path_when_any_keyframe_has_vertices() {
    assert!(!Mask::new(MaskKind::Circle).has_path());
    assert!(Mask::new(MaskKind::Path).has_path());
    assert!(
        keyed(vec![
            (0, Vec::new(), Easing::Linear),
            (5, triangle(0.1), Easing::Linear)
        ])
        .has_path()
    );
    assert!(!keyed(vec![(0, Vec::new(), Easing::Linear)]).has_path());
}

#[test]
fn a_path_saved_before_keyframes_still_loads_and_saves_the_same() {
    let saved = r#"{"kind":"path","path":[{"anchor":[0.1,0.1],"handle_in":[0.1,0.1],"handle_out":[0.1,0.1]},{"anchor":[0.9,0.1],"handle_in":[0.9,0.1],"handle_out":[0.9,0.1]},{"anchor":[0.5,0.9],"handle_in":[0.5,0.9],"handle_out":[0.5,0.9]}],"path_closed":true}"#;
    let mask: Mask = serde_json::from_str(saved).unwrap();
    assert_eq!(
        mask.path,
        Param::Constant(vec![
            ShapePathPoint::corner([0.1, 0.1]),
            ShapePathPoint::corner([0.9, 0.1]),
            ShapePathPoint::corner([0.5, 0.9]),
        ])
    );
    // Through text, as the file is: an `f32` widened straight into a JSON value
    // is not the decimal the file holds.
    let again: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&mask).unwrap()).unwrap();
    let original: serde_json::Value = serde_json::from_str(saved).unwrap();
    assert_eq!(
        again["path"], original["path"],
        "an unkeyed path is a bare array"
    );
}

#[test]
fn a_keyed_path_round_trips_through_its_file_form() {
    let mask = keyed(vec![
        (0, triangle(0.1), Easing::EaseOut),
        (20, square(), Easing::Linear),
    ]);
    let value = serde_json::to_value(&mask).unwrap();
    let keys = value["path"]["kf"].as_array().expect("keyframes under kf");
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["t"], 0);
    assert_eq!(keys[1]["v"].as_array().map(Vec::len), Some(4));
    let back: Mask = serde_json::from_value(value).unwrap();
    assert_eq!(back, mask);
}

#[test]
fn a_shape_mask_writes_no_path() {
    let value = serde_json::to_value(Mask::new(MaskKind::Circle)).unwrap();
    assert!(value.get("path").is_none(), "{value}");
    let back: Mask = serde_json::from_value(value).unwrap();
    assert!(!back.has_path());
}

#[test]
fn every_keyframe_of_a_path_is_held_to_the_vertex_rules() {
    let far = vec![
        ShapePathPoint::corner([0.1, 0.1]),
        ShapePathPoint::corner([11.0, 0.5]),
    ];
    let nan = vec![ShapePathPoint::corner([f32::NAN, 0.1])];
    let crowd = vec![ShapePathPoint::corner([0.5, 0.5]); MAX_MASK_PATH_POINTS + 1];
    for (label, bad) in [
        ("far", far),
        ("nan", nan),
        ("crowd", crowd),
        ("empty", Vec::new()),
    ] {
        let first = keyed(vec![
            (0, bad.clone(), Easing::Linear),
            (9, triangle(0.1), Easing::Linear),
        ]);
        assert!(first.validate().is_err(), "{label} at the first keyframe");
        let last = keyed(vec![
            (0, triangle(0.1), Easing::Linear),
            (9, bad.clone(), Easing::Linear),
        ]);
        assert!(last.validate().is_err(), "{label} at the last keyframe");
    }
    assert!(
        keyed(vec![
            (0, triangle(0.1), Easing::Linear),
            (9, square(), Easing::Linear)
        ])
        .validate()
        .is_ok()
    );
}

#[test]
fn a_keyed_path_out_of_time_order_or_without_keyframes_is_rejected() {
    let unsorted = keyed(vec![
        (9, triangle(0.1), Easing::Linear),
        (0, triangle(0.2), Easing::Linear),
    ]);
    assert!(unsorted.validate().is_err());
    let twice = keyed(vec![
        (4, triangle(0.1), Easing::Linear),
        (4, triangle(0.2), Easing::Linear),
    ]);
    assert!(twice.validate().is_err());
    let none = keyed(Vec::new());
    assert!(none.validate().is_err());
}
