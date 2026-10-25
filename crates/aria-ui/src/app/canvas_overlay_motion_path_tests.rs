//! Motion paths in the Viewer: the path a keyframed Position or Anchor point
//! draws, its keyframes and tangent handles, and the edits made on it.
//!
//! Closes the class where the drawn path disagrees with the curve the renderer
//! samples (a dot off the curve, a frame left with no dot near it, a child's
//! path drawn on the canvas rather than through its parent), where a press on a
//! keyframe or a tangent edits something other than that keyframe or edits
//! before the release, and where an edit on the path moves the path off the
//! points it ran through.
//!
//! Not covered: the painted dots and the pointer routing in the panel, which
//! the headed drive shows.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipParam, ClipTransform, Easing, Param, SpatialTangents};

use super::{
    DOT_GAP, DrawnKey, OwnKey, PATH_HIT, PathKey, PathParam, TangentSide, auto_tangents, bend,
    paths_of,
};
use crate::app::AppState;
use crate::app::fixtures::{key_point, keyed_path, parented_pair};
use crate::app::sent_census::Watch;
use crate::app::transform_handle_tests::{framed, selected};
use crate::surfaces::overlay_gesture::{self, Gesture, Step};

const CORNER: PathKey = PathKey {
    param: PathParam::Position,
    index: 0,
};
const SMOOTH: PathKey = PathKey {
    param: PathParam::Position,
    index: 1,
};

/// The still selected and framed, its Position keyed by [`keyed_path`]: a
/// corner at frame 0, a smooth keyframe at 12, a corner at 24.
fn pathed() -> AppState {
    let mut state = selected();
    framed(&mut state);
    keyed_path(&mut state);
    state
}

/// `param`'s curve on the selected clip, as the model holds it.
fn curve(state: &AppState, param: ClipParam) -> Param<[f32; 2]> {
    param
        .motion_path(state.selected_model().expect("a selected clip"))
        .expect("the point is keyed")
        .clone()
}

/// Where a Position value lands on the canvas `framed` declares: 400 by 200
/// at the window's origin, a position measured from its centre.
fn on_canvas([x, y]: [f32; 2]) -> [f32; 2] {
    [400.0 * (0.5 + x), 200.0 * (0.5 + y)]
}

fn near(a: [f32; 2], b: [f32; 2], within: f32) -> bool {
    (a[0] - b[0]).abs() <= within && (a[1] - b[1]).abs() <= within
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn step(state: &mut AppState, step: Step) {
    overlay_gesture::apply(state, step)
        .unwrap_or_else(|err| panic!("a press on the path was refused: {err}"));
}

/// `key` as the Viewer draws it.
fn drawn(state: &AppState, key: PathKey) -> DrawnKey {
    let paths = state.motion_paths();
    let path = paths
        .iter()
        .find(|path| path.param == key.param)
        .expect("the path is drawn");
    *path.keys.get(key.index).expect("the keyframe is drawn")
}

fn live_overrides(state: &AppState) -> bool {
    state.engine.lock().has_live_overrides()
}

#[test]
fn every_dot_is_the_curve_at_one_frame_and_no_frame_is_far_from_a_dot() {
    let state = pathed();
    let curve = curve(&state, ClipParam::Position);
    let keys = curve.keyframes();
    let frames: Vec<[f32; 2]> = (keys[0].tick..=keys[keys.len() - 1].tick)
        .map(|tick| on_canvas(curve.sample(tick)))
        .collect();
    let paths = state.motion_paths();
    let params: Vec<PathParam> = paths.iter().map(|path| path.param).collect();
    assert_eq!(params, [PathParam::Position], "only Position is keyed");
    let dots = &paths[0].dots;
    assert!(
        near(dots[0], frames[0], 1e-3),
        "the path does not start on its first keyframe"
    );
    // Walk the frames in order: each is either the next drawn dot, or within
    // the dot gap of the dot drawn before it.
    let mut next = 0;
    for (tick, frame) in frames.iter().enumerate() {
        if dots.get(next).is_some_and(|dot| near(*dot, *frame, 1e-3)) {
            if next > 0 {
                assert!(
                    distance(dots[next], dots[next - 1]) >= DOT_GAP,
                    "dot {next} is painted under the one before it"
                );
            }
            next += 1;
        } else {
            let last = next
                .checked_sub(1)
                .and_then(|index| dots.get(index))
                .expect("a dot is drawn before any frame is skipped");
            assert!(
                distance(*frame, *last) < DOT_GAP,
                "frame {tick} at {frame:?} has no dot near it"
            );
        }
    }
    assert_eq!(next, dots.len(), "a dot lies on no frame of the curve");
}

#[test]
fn keyframes_sit_on_their_values_and_only_a_bending_side_draws_a_handle() {
    let mut state = pathed();
    // Every keyframe smooth, so each end has a handle that shapes nothing.
    let both = SpatialTangents {
        in_t: [-0.05, 0.05],
        out_t: [0.05, -0.05],
    };
    for (tick, value) in [(0, [-0.25, 0.0]), (12, [0.0, 0.2]), (24, [0.25, 0.0])] {
        key_point(&mut state, ClipParam::Position, tick, value, Some(both));
    }
    assert_eq!(
        state.playhead.value, 0,
        "the playhead parks on the first key"
    );
    let curve = curve(&state, ClipParam::Position);
    let keys = curve.keyframes();
    let last = keys.len() - 1;
    for (index, key) in keys.iter().enumerate() {
        let shown = drawn(
            &state,
            PathKey {
                param: PathParam::Position,
                index,
            },
        );
        assert!(
            near(shown.at, on_canvas(key.value), 1e-2),
            "keyframe {index}"
        );
        assert_eq!(
            shown.current,
            index == 0,
            "keyframe {index} under the playhead"
        );
        let handle =
            |offset: [f32; 2]| on_canvas([key.value[0] + offset[0], key.value[1] + offset[1]]);
        assert_eq!(
            shown.handle(TangentSide::In).is_some(),
            index > 0,
            "keyframe {index}: an incoming handle with no segment arriving"
        );
        assert_eq!(
            shown.handle(TangentSide::Out).is_some(),
            index < last,
            "keyframe {index}: an outgoing handle with no segment leaving"
        );
        if let Some(at) = shown.handle(TangentSide::In) {
            assert!(near(at, handle(both.in_t), 1e-2), "keyframe {index} in");
        }
        if let Some(at) = shown.handle(TangentSide::Out) {
            assert!(near(at, handle(both.out_t), 1e-2), "keyframe {index} out");
        }
    }

    // A corner draws no handle on either side.
    let state = pathed();
    let corner = drawn(&state, CORNER);
    assert_eq!((corner.tangent_in, corner.tangent_out), (None, None));
}

#[test]
fn nothing_is_drawn_without_keys_before_a_paint_or_on_an_audio_lane() {
    let mut state = selected();
    framed(&mut state);
    assert!(
        state.motion_paths().is_empty(),
        "an unkeyed clip drew a path"
    );

    let mut state = pathed();
    let model = state.selected_model().expect("a selected clip");
    assert!(
        paths_of(model, true).is_empty(),
        "a clip on an audio lane drew a path"
    );
    state.overlay_frame.canvas = None;
    assert!(
        state.motion_paths().is_empty(),
        "a path was placed before the picture was painted"
    );
}

#[test]
fn a_childs_path_is_drawn_through_its_parent_and_its_keyframe_follows_the_pointer() {
    let (mut state, _) = parented_pair();
    let parent = state
        .selected_parent_choices()
        .and_then(|choices| choices.current)
        .expect("the child has a parent");
    state
        .apply_command(Command::Edit(EditCommand::SetClipTransform {
            clip: parent,
            transform: ClipTransform {
                position: [0.25, 0.0],
                rotation: 90.0,
                ..ClipTransform::IDENTITY
            },
            at: None,
        }))
        .expect("turn the parent");
    framed(&mut state);
    keyed_path(&mut state);
    let frame = state
        .selected_parent_frame()
        .expect("the parent covers the playhead");
    let curve = curve(&state, ClipParam::Position);
    for (index, key) in curve.keyframes().iter().enumerate() {
        let placed = frame.place(key.value);
        assert!(
            !near(placed, key.value, 1e-3),
            "keyframe {index}: the parent's frame does not move it"
        );
        let shown = drawn(
            &state,
            PathKey {
                param: PathParam::Position,
                index,
            },
        );
        assert!(
            near(shown.at, on_canvas(placed), 1e-2),
            "keyframe {index} is drawn at {:?}, not through its parent",
            shown.at
        );
    }

    let from = drawn(&state, SMOOTH).at;
    let to = [from[0] + 30.0, from[1] + 10.0];
    step(&mut state, Step::Grab(Gesture::PathKey(SMOOTH), from));
    step(&mut state, Step::Carry(to));
    let shown = drawn(&state, SMOOTH).at;
    assert!(
        near(shown, to, 1e-2),
        "the keyframe is drawn at {shown:?}, not under the pointer at {to:?}"
    );
}

#[test]
fn a_keyframe_dragged_is_drafted_then_committed_as_one_undoable_keyframe() {
    let mut state = pathed();
    let watch = Watch::on(&state);
    // Pressed off the square's centre: the keyframe moves with the pointer
    // rather than jumping under it.
    let from = drawn(&state, SMOOTH).at;
    let grab = [from[0] + 3.0, from[1] - 2.0];
    step(&mut state, Step::Grab(Gesture::PathKey(SMOOTH), grab));
    step(&mut state, Step::Carry([grab[0] + 40.0, grab[1] + 20.0]));
    assert!(
        near(
            drawn(&state, SMOOTH).at,
            [from[0] + 40.0, from[1] + 20.0],
            1e-2
        ),
        "the keyframe does not follow the pointer mid-drag"
    );
    assert!(
        live_overrides(&state),
        "the picture does not follow the drag"
    );
    assert!(
        watch.take().is_empty(),
        "the drag edited before its release"
    );

    step(&mut state, Step::Release);
    assert_eq!(watch.take(), ["Set keyframe"]);
    assert!(!live_overrides(&state), "the release left its draft");
    let held = curve(&state, ClipParam::Position).keyframes()[1];
    assert!(
        near(held.value, [0.1, 0.3], 1e-4),
        "committed {:?}",
        held.value
    );
    assert_eq!(held.easing, Easing::Linear, "the drag dropped the easing");
    assert_eq!(
        held.tangents,
        Some(SpatialTangents {
            in_t: [-0.08, 0.0],
            out_t: [0.08, 0.0],
        }),
        "the drag dropped the tangents"
    );

    assert!(state.undo(), "the drag left nothing to undo");
    let held = curve(&state, ClipParam::Position).keyframes()[1];
    assert!(
        near(held.value, [0.0, 0.2], 1e-6),
        "one undo did not restore it"
    );
}

#[test]
fn the_picture_follows_a_drag_on_the_keyframe_under_the_playhead() {
    // The handles and the picture read the drafted curve at the playhead, so
    // the layer under a dragged keyframe moves with it before the release.
    let mut state = pathed();
    let from = drawn(&state, CORNER).at;
    step(&mut state, Step::Grab(Gesture::PathKey(CORNER), from));
    step(&mut state, Step::Carry([from[0] + 40.0, from[1] + 20.0]));
    let shown = state.overlay_transform().expect("a selected clip");
    assert!(
        near([shown.position_x, shown.position_y], [-0.15, 0.1], 1e-4),
        "mid-drag the picture sits at {:?}",
        [shown.position_x, shown.position_y]
    );
    step(&mut state, Step::Release);
    let shown = state.overlay_transform().expect("a selected clip");
    assert!(
        near([shown.position_x, shown.position_y], [-0.15, 0.1], 1e-4),
        "the release committed a position other than the one drawn"
    );
}

#[test]
fn a_tangent_bent_moves_its_handle_and_turns_the_other_against_it() {
    let mut state = pathed();
    let from = drawn(&state, SMOOTH)
        .handle(TangentSide::Out)
        .expect("a smooth keyframe draws an outgoing handle");
    step(
        &mut state,
        Step::Grab(Gesture::PathTangent(SMOOTH, TangentSide::Out), from),
    );
    step(&mut state, Step::Carry([from[0] + 20.0, from[1] + 40.0]));
    step(&mut state, Step::Release);
    let held = curve(&state, ClipParam::Position).keyframes()[1];
    assert!(
        near(held.value, [0.0, 0.2], 1e-6),
        "bending moved the keyframe"
    );
    let bent = held.tangents.expect("the keyframe stays smooth");
    assert!(near(bent.out_t, [0.13, 0.2], 1e-4), "out {:?}", bent.out_t);
    let (out, inward) = (bent.out_t, bent.in_t);
    assert!(
        (inward[0].hypot(inward[1]) - 0.08).abs() < 1e-4,
        "the other handle changed length: {inward:?}"
    );
    assert!(
        (inward[0] * out[1] - inward[1] * out[0]).abs() < 1e-5
            && inward[0] * out[0] + inward[1] * out[1] < 0.0,
        "the other handle does not point against the dragged one: {inward:?}"
    );
}

#[test]
fn a_press_on_the_path_that_never_moved_edits_nothing() {
    for gesture in [
        Gesture::PathKey(SMOOTH),
        Gesture::PathTangent(SMOOTH, TangentSide::In),
    ] {
        let mut state = pathed();
        let before = curve(&state, ClipParam::Position);
        let watch = Watch::on(&state);
        let at = match gesture {
            Gesture::PathTangent(key, side) => drawn(&state, key).handle(side).expect("a handle"),
            _ => drawn(&state, SMOOTH).at,
        };
        step(&mut state, Step::Grab(gesture, at));
        step(&mut state, Step::Release);
        assert!(
            watch.take().is_empty(),
            "{} with no movement edited",
            gesture.what()
        );
        assert!(!live_overrides(&state), "{} left a draft", gesture.what());
        assert_eq!(curve(&state, ClipParam::Position), before);
    }
}

#[test]
fn a_cancelled_path_drag_drops_its_draft() {
    for gesture in [
        Gesture::PathKey(SMOOTH),
        Gesture::PathTangent(SMOOTH, TangentSide::Out),
    ] {
        let mut state = pathed();
        let before = state.motion_paths();
        let watch = Watch::on(&state);
        let at = match gesture {
            Gesture::PathTangent(key, side) => drawn(&state, key).handle(side).expect("a handle"),
            _ => drawn(&state, SMOOTH).at,
        };
        step(&mut state, Step::Grab(gesture, at));
        step(&mut state, Step::Carry([at[0] + 25.0, at[1] - 15.0]));
        assert_ne!(
            state.motion_paths(),
            before,
            "{} drafted nothing",
            gesture.what()
        );
        state.cancel_overlay_drag();
        assert!(
            watch.take().is_empty(),
            "cancelling {} committed it",
            gesture.what()
        );
        assert!(!live_overrides(&state), "{} left a draft", gesture.what());
        assert_eq!(
            state.motion_paths(),
            before,
            "{} left the path moved",
            gesture.what()
        );
    }
}

#[test]
fn alt_click_turns_a_keyframe_between_corner_and_smooth() {
    let mut state = pathed();
    let watch = Watch::on(&state);
    let at = drawn(&state, SMOOTH).at;
    step(&mut state, Step::Grab(Gesture::PathCorner(SMOOTH), at));
    assert_eq!(watch.take(), ["Set keyframe"]);
    let held = curve(&state, ClipParam::Position).keyframes()[1];
    assert!(
        held.tangents.is_none_or(|t| t == SpatialTangents::ZERO),
        "the smooth keyframe kept its handles: {:?}",
        held.tangents
    );
    assert!(
        near(held.value, [0.0, 0.2], 1e-6),
        "the press moved the keyframe"
    );

    // The first keyframe points its one handle a third of the way to the next.
    let at = drawn(&state, CORNER).at;
    step(&mut state, Step::Grab(Gesture::PathCorner(CORNER), at));
    assert_eq!(watch.take(), ["Set keyframe"]);
    let held = curve(&state, ClipParam::Position).keyframes()[0];
    let made = held.tangents.expect("the corner became smooth");
    assert!(
        near(made.out_t, [0.25 / 3.0, 0.2 / 3.0], 1e-5),
        "out {:?}",
        made.out_t
    );
    assert!(near(made.in_t, [0.0, 0.0], 1e-6), "in {:?}", made.in_t);
    assert!(
        near(held.value, [-0.25, 0.0], 1e-6),
        "the press moved the keyframe"
    );
}

#[test]
fn an_anchor_point_path_is_drawn_on_the_layer_and_never_bends() {
    let mut state = selected();
    framed(&mut state);
    key_point(&mut state, ClipParam::AnchorPoint, 0, [0.25, 0.5], None);
    key_point(&mut state, ClipParam::AnchorPoint, 12, [0.75, 0.5], None);
    let paths = state.motion_paths();
    let params: Vec<PathParam> = paths.iter().map(|path| path.param).collect();
    assert_eq!(params, [PathParam::AnchorPoint]);
    // `framed` puts the layer at (100, 50), 200 by 100.
    let at: Vec<[f32; 2]> = paths[0].keys.iter().map(|key| key.at).collect();
    assert!(
        near(at[0], [150.0, 100.0], 1e-3) && near(at[1], [250.0, 100.0], 1e-3),
        "the anchor's keyframes are drawn at {at:?}, not across the layer"
    );

    let watch = Watch::on(&state);
    let first = PathKey {
        param: PathParam::AnchorPoint,
        index: 0,
    };
    step(&mut state, Step::Grab(Gesture::PathCorner(first), at[0]));
    assert!(
        watch.take().is_empty(),
        "an anchor keyframe was made smooth"
    );
    let middle = state
        .motion_paths()
        .first()
        .and_then(|path| path.dots.get(path.dots.len() / 2).copied())
        .expect("a dot between the keyframes");
    assert!(state.add_key_on_path(middle).expect("the add is accepted"));
    let anchor = curve(&state, ClipParam::AnchorPoint);
    assert_eq!(anchor.keyframes().len(), 3, "Ctrl+click added no keyframe");
    assert!(
        anchor.keyframes().iter().all(|key| key.tangents.is_none()),
        "an anchor keyframe carries tangents"
    );
}

#[test]
fn ctrl_click_adds_a_keyframe_where_the_path_already_runs() {
    // Between two corners on a linear path, the path is the same line after.
    let mut state = selected();
    framed(&mut state);
    key_point(&mut state, ClipParam::Position, 0, [-0.25, -0.1], None);
    key_point(&mut state, ClipParam::Position, 24, [0.25, 0.2], None);
    let before = curve(&state, ClipParam::Position);
    let watch = Watch::on(&state);
    step(
        &mut state,
        Step::Grab(Gesture::PathAdd, on_canvas(before.sample(6))),
    );
    assert_eq!(watch.take(), ["Set keyframe"]);
    let after = curve(&state, ClipParam::Position);
    let added = after
        .keyframe_at(6)
        .expect("a keyframe at the frame pressed");
    assert_eq!(added.tangents, None, "a keyframe between corners bends");
    for tick in 0..=24 {
        assert!(
            near(after.sample(tick), before.sample(tick), 1e-5),
            "frame {tick} moved: {:?} from {:?}",
            after.sample(tick),
            before.sample(tick)
        );
    }

    // Beside a smooth keyframe the new one bends, through the same point.
    let mut state = pathed();
    let before = curve(&state, ClipParam::Position);
    assert!(
        state
            .add_key_on_path(on_canvas(before.sample(18)))
            .expect("the add is accepted"),
        "the press on the path was let through"
    );
    let after = curve(&state, ClipParam::Position);
    let added = after
        .keyframe_at(18)
        .expect("a keyframe at the frame pressed");
    assert!(
        near(added.value, before.sample(18), 1e-5),
        "the path left the point pressed"
    );
    assert!(
        added.tangents.is_some(),
        "a keyframe beside a smooth one is a corner"
    );

    // On a keyframe the press is taken and nothing is written; off the path
    // it is let through.
    let mut state = pathed();
    let watch = Watch::on(&state);
    let on_key = drawn(&state, SMOOTH).at;
    assert!(
        state.add_key_on_path(on_key).expect("inert"),
        "the press on a keyframe was let through"
    );
    assert!(
        !state.add_key_on_path([5.0, 5.0]).expect("inert"),
        "a press off the path was taken"
    );
    assert!(
        watch.take().is_empty(),
        "a press on a keyframe or off the path edited"
    );
}

#[test]
fn a_press_finds_the_nearest_frame_within_reach_and_nothing_beyond() {
    let state = pathed();
    let curve = curve(&state, ClipParam::Position);
    let at = on_canvas(curve.sample(6));
    assert_eq!(state.path_hit(at), Some((PathParam::Position, 6)));
    assert_eq!(
        state.path_hit([at[0] + 1.0, at[1] - 1.0]),
        Some((PathParam::Position, 6))
    );
    assert_eq!(state.path_hit([at[0], at[1] + 2.0 * PATH_HIT]), None);
}

#[test]
fn bending_one_handle_keeps_the_other_opposite_at_its_own_length() {
    let start = SpatialTangents {
        in_t: [-0.3, 0.0],
        out_t: [0.1, 0.0],
    };
    let cases: [(TangentSide, [f32; 2], SpatialTangents); 4] = [
        (
            TangentSide::Out,
            [0.0, 0.2],
            SpatialTangents {
                out_t: [0.0, 0.2],
                in_t: [0.0, -0.3],
            },
        ),
        (
            TangentSide::In,
            [0.0, 0.5],
            SpatialTangents {
                in_t: [0.0, 0.5],
                out_t: [0.0, -0.1],
            },
        ),
        // A handle dragged past the model's bound stops at it.
        (
            TangentSide::Out,
            [9.0, 0.0],
            SpatialTangents {
                out_t: [4.0, 0.0],
                in_t: [-0.3, 0.0],
            },
        ),
        // A handle dragged onto its keyframe has no direction to turn the
        // other one to, so the other stays.
        (
            TangentSide::Out,
            [0.0, 0.0],
            SpatialTangents {
                out_t: [0.0, 0.0],
                in_t: [-0.3, 0.0],
            },
        ),
    ];
    for (side, offset, expected) in cases {
        let bent = bend(start, side, offset);
        assert!(
            near(bent.in_t, expected.in_t, 1e-6) && near(bent.out_t, expected.out_t, 1e-6),
            "{side:?} to {offset:?} bent to {bent:?}, not {expected:?}"
        );
    }
}

#[test]
fn auto_tangents_run_along_the_neighbours_a_third_of_the_way() {
    let key = |value: [f32; 2]| OwnKey {
        tick: 0,
        local: 0,
        value,
        easing: Easing::Linear,
        tangents: None,
    };
    let keys = [key([0.0, 0.0]), key([0.3, 0.0]), key([0.3, 0.6])];
    // The middle keyframe's handles lie along first-to-last, [0.3, 0.6].
    let unit = [0.3 / 0.45_f32.sqrt(), 0.6 / 0.45_f32.sqrt()];
    let middle = auto_tangents(&keys, 1).expect("a middle keyframe bends");
    assert!(
        near(middle.in_t, [-unit[0] * 0.1, -unit[1] * 0.1], 1e-5),
        "{middle:?}"
    );
    assert!(
        near(middle.out_t, [unit[0] * 0.2, unit[1] * 0.2], 1e-5),
        "{middle:?}"
    );
    let first = auto_tangents(&keys, 0).expect("an end keyframe bends");
    assert!(near(first.out_t, [0.1, 0.0], 1e-6) && near(first.in_t, [0.0, 0.0], 1e-6));
    let last = auto_tangents(&keys, 2).expect("an end keyframe bends");
    assert!(near(last.in_t, [0.0, -0.2], 1e-6) && near(last.out_t, [0.0, 0.0], 1e-6));

    let folded = [key([0.0, 0.0]), key([0.3, 0.3]), key([0.0, 0.0])];
    assert_eq!(
        auto_tangents(&folded, 1),
        None,
        "neighbors on one point give no direction"
    );
    assert_eq!(
        auto_tangents(&keys[..1], 0),
        None,
        "a lone keyframe has nothing to point at"
    );
}
