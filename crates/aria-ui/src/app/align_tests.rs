//! Align and Distribute, measured on the bounds a composite draws.
//!
//! # Why
//!
//! The class: a move computed from a different box than the one on screen, so
//! a clip lands a scale, an anchor, a rotation or a parent away from the edge
//! it was aligned to; a press that writes some clips and not others, or more
//! than one undo step; and a keyed Position that is flattened instead of keyed.
//! Every expected edge below is computed here from the measured bounds with its
//! own arithmetic, never with the edge function under test, so a swapped edge
//! or a dropped rotation term turns a case red.
//!
//! Not covered: the panel's drawing, which `drawn_census_align` reads as source.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, DriverCode, SpatialTangents};

use super::{AlignAxis, AlignEdge, AlignTo, Bounds, DISTRIBUTE_MIN, placements};
use crate::app::AppState;
use crate::app::fixtures::{key_point, parented_pair, spread_three};
use crate::app::sent_census::Watch;

/// How far a measured edge may sit from where it was sent, in canvas pixels.
const TOLERANCE: f32 = 0.05;

/// The bounds of `clips` as the resolver and renderer place them at the
/// playhead, and the canvas size.
fn measured(state: &AppState, clips: &[ClipId]) -> (Vec<Bounds>, [f32; 2]) {
    let mut engine = state.engine().lock();
    let (placed, canvas) = placements(&mut engine, clips, state.playhead);
    assert_eq!(
        placed.len(),
        clips.len(),
        "every clip draws at the playhead"
    );
    (placed.iter().map(|placed| placed.bounds).collect(), canvas)
}

/// Where `edge` of `bounds` lies, computed independently of
/// [`AlignEdge::line`].
fn edge_of(edge: AlignEdge, bounds: Bounds) -> f32 {
    match edge {
        AlignEdge::Left => bounds.min[0],
        AlignEdge::CenterX => (bounds.min[0] + bounds.max[0]) / 2.0,
        AlignEdge::Right => bounds.max[0],
        AlignEdge::Top => bounds.min[1],
        AlignEdge::CenterY => (bounds.min[1] + bounds.max[1]) / 2.0,
        AlignEdge::Bottom => bounds.max[1],
    }
}

/// The axis `edge` does not move.
fn other_axis(edge: AlignEdge) -> usize {
    match edge {
        AlignEdge::Left | AlignEdge::CenterX | AlignEdge::Right => 1,
        AlignEdge::Top | AlignEdge::CenterY | AlignEdge::Bottom => 0,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= TOLERANCE
}

#[test]
fn every_edge_lands_on_its_target_for_every_align_to() {
    for to in AlignTo::ALL {
        for edge in AlignEdge::ALL {
            let (mut state, clips) = spread_three();
            let (before, canvas) = measured(&state, &clips);
            let around = Bounds {
                min: [
                    before.iter().map(|b| b.min[0]).fold(f32::MAX, f32::min),
                    before.iter().map(|b| b.min[1]).fold(f32::MAX, f32::min),
                ],
                max: [
                    before.iter().map(|b| b.max[0]).fold(f32::MIN, f32::max),
                    before.iter().map(|b| b.max[1]).fold(f32::MIN, f32::max),
                ],
            };
            let want = edge_of(
                edge,
                match to {
                    AlignTo::Frame => Bounds {
                        min: [0.0, 0.0],
                        max: canvas,
                    },
                    AlignTo::Selection => around,
                    AlignTo::FirstClip => before[0],
                },
            );
            state.set_align_to(to);
            state.align_selected(edge).expect("align lands");
            let (after, _) = measured(&state, &clips);
            let still = other_axis(edge);
            for (index, (was, now)) in before.iter().zip(&after).enumerate() {
                assert!(
                    close(edge_of(edge, *now), want),
                    "{to:?} {edge:?}: clip {index} lies at {}, not {want}",
                    edge_of(edge, *now)
                );
                assert!(
                    close(now.min[still], was.min[still]),
                    "{to:?} {edge:?}: clip {index} moved across the other axis"
                );
            }
        }
    }
}

#[test]
fn a_rotated_clip_aligns_by_the_box_around_its_turned_corners() {
    let (mut state, clips) = spread_three();
    let mut turned = state.ui_clip(clips[2]).expect("the top clip").transform;
    turned.rotation = 30.0;
    state.selection = vec![clips[2]];
    state.sync_from_engine();
    state.set_selected_transform(turned).expect("turn the clip");
    state.set_align_to(AlignTo::Frame);
    state.align_selected(AlignEdge::Left).expect("align lands");

    // The corners, turned here rather than through `Bounds::of_quad`.
    let mut engine = state.engine().lock();
    let scene = aria_render::resolve(engine.project(), state.playhead).expect("a scene");
    let layer = scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(clips[2]))
        .expect("the turned clip draws");
    let canvas = [scene.width as f32, scene.height as f32];
    let (center, size) = engine.layer_quad(layer, canvas).expect("a quad");
    let (sin, cos) = layer.rotation.sin_cos();
    assert!(sin.abs() > 0.4, "the clip is turned");
    let left = [-0.5_f32, 0.5]
        .iter()
        .flat_map(|sx| [-0.5_f32, 0.5].map(|sy| (sx * size[0], sy * size[1])))
        .map(|(x, y)| center[0] + x * cos - y * sin)
        .fold(f32::MAX, f32::min);
    assert!(
        close(left, 0.0),
        "the turned clip's leftmost corner is at {left}"
    );
}

#[test]
fn the_box_of_a_turned_quad_holds_its_corners() {
    let quarter = Bounds::of_quad([100.0, 50.0], [40.0, 20.0], std::f32::consts::FRAC_PI_2);
    assert!(close(quarter.max[0] - quarter.min[0], 20.0));
    assert!(close(quarter.max[1] - quarter.min[1], 40.0));
    let square = Bounds::of_quad([0.0, 0.0], [10.0, 10.0], std::f32::consts::FRAC_PI_4);
    assert!(close(square.max[0], 50.0_f32.sqrt()));
    assert!(close(square.min[1], -(50.0_f32.sqrt())));
}

#[test]
fn distribute_spaces_centres_evenly_and_keeps_the_ends() {
    for axis in AlignAxis::ALL {
        let (mut state, clips) = spread_three();
        let index = match axis {
            AlignAxis::Horizontal => 0,
            AlignAxis::Vertical => 1,
        };
        let centre = |b: &Bounds| (b.min[index] + b.max[index]) / 2.0;
        let (before, _) = measured(&state, &clips);
        state.distribute_selected(axis).expect("distribute lands");
        let (after, _) = measured(&state, &clips);
        let mut was: Vec<f32> = before.iter().map(centre).collect();
        let mut now: Vec<f32> = after.iter().map(centre).collect();
        was.sort_by(f32::total_cmp);
        now.sort_by(f32::total_cmp);
        assert!(
            close(now[0], was[0]) && close(now[2], was[2]),
            "{axis:?} moved an end"
        );
        assert!(
            close(now[1] - now[0], now[2] - now[1]),
            "{axis:?} left the centres at {now:?}"
        );
        assert!(
            !close(now[1], was[1]),
            "{axis:?} moved nothing: the fixture is evenly spread"
        );
        for (b, a) in before.iter().zip(&after) {
            assert!(
                close(a.min[1 - index], b.min[1 - index]),
                "{axis:?} moved the other axis"
            );
        }
    }
}

#[test]
fn a_press_is_one_undo_step() {
    let (mut state, clips) = spread_three();
    let (before, _) = measured(&state, &clips);
    state.set_align_to(AlignTo::Frame);
    state.align_selected(AlignEdge::Right).expect("align lands");
    let (moved, _) = measured(&state, &clips);
    assert!(
        before
            .iter()
            .zip(&moved)
            .all(|(b, m)| !close(b.max[0], m.max[0])),
        "every clip moved"
    );
    assert!(state.undo(), "the press is on the undo stack");
    let (undone, _) = measured(&state, &clips);
    for (index, (b, u)) in before.iter().zip(&undone).enumerate() {
        assert!(
            close(b.max[0], u.max[0]),
            "one undo left clip {index} moved"
        );
    }
}

#[test]
fn a_keyed_position_takes_a_keyframe_and_keeps_its_curve() {
    let (mut state, clips) = spread_three();
    let smooth = SpatialTangents {
        in_t: [-0.05, 0.0],
        out_t: [0.05, 0.0],
    };
    state.selection = vec![clips[1]];
    state.sync_from_engine();
    key_point(&mut state, ClipParam::Position, 0, [0.05, 0.1], None);
    key_point(
        &mut state,
        ClipParam::Position,
        12,
        [0.1, 0.1],
        Some(smooth),
    );
    key_point(&mut state, ClipParam::Position, 24, [0.15, 0.1], None);
    state.selection = clips.to_vec();
    state.sync_from_engine();
    state.set_align_to(AlignTo::Frame);

    let keys = |state: &AppState| {
        let engine = state.engine().lock();
        let clip = engine.project().clip(clips[1]).expect("the keyed clip");
        clip.transform
            .position
            .keyframes()
            .iter()
            .map(|key| (key.tick, key.value, key.tangents))
            .collect::<Vec<_>>()
    };
    state.set_playhead(6);
    state
        .align_selected(AlignEdge::Top)
        .expect("align between keys");
    let between = keys(&state);
    assert_eq!(
        between.iter().map(|key| key.0).collect::<Vec<_>>(),
        [0, 6, 12, 24],
        "a move between keyframes adds one under the playhead"
    );
    assert_eq!(
        between[0].1,
        [0.05, 0.1],
        "the key before the playhead kept its value"
    );
    assert_eq!(
        between[3].1,
        [0.15, 0.1],
        "the key after the playhead kept its value"
    );

    state.set_playhead(12);
    state
        .align_selected(AlignEdge::Bottom)
        .expect("align on a key");
    let on = keys(&state);
    assert_eq!(on.len(), 4, "a move on a keyframe rewrites it");
    assert_eq!(
        on[2].2,
        Some(smooth),
        "the rewritten keyframe kept its path handles"
    );
    assert_ne!(on[2].1, [0.1, 0.1], "the keyframe on the playhead moved");
}

#[test]
fn a_parented_clip_aligns_where_its_parent_puts_it() {
    let (mut state, child) = parented_pair();
    let parent = state
        .ui_clip_ref(child)
        .and_then(|clip| clip.parent)
        .map(ClipId::from_raw)
        .expect("the child has a parent");
    for (clip, position, scale) in [(parent, [0.2, 0.1], 0.5), (child, [-0.1, 0.05], 0.4)] {
        state
            .apply_command(Command::Edit(EditCommand::SetClipTransform {
                clip,
                transform: aria_models::ClipTransform {
                    position,
                    scale: aria_models::Scale2::uniform(scale),
                    ..aria_models::ClipTransform::IDENTITY
                },
                at: None,
            }))
            .expect("place a clip");
    }
    state.selection = vec![child];
    state.sync_from_engine();
    state.set_align_to(AlignTo::Frame);
    state.align_selected(AlignEdge::Right).expect("align lands");
    let (after, canvas) = measured(&state, &[child]);
    assert!(
        close(after[0].max[0], canvas[0]),
        "the child's right edge is at {}, not the frame's {}",
        after[0].max[0],
        canvas[0]
    );
}

#[test]
fn a_locked_or_driven_clip_stays_where_it_is() {
    for driven in [false, true] {
        let (mut state, clips) = spread_three();
        if driven {
            state
                .ctx
                .mount(&aria_plugin::ExpressionsPlugin)
                .expect("mount Expressions");
            state
                .apply_command(Command::Edit(EditCommand::SetParamDriver {
                    clip: clips[2],
                    param: ClipParam::Position,
                    code: Some(DriverCode::new("aria.expressions", "value")),
                }))
                .expect("drive the top clip's Position");
        } else {
            let lane = state.ui_clip_ref(clips[2]).expect("the top clip").track_id;
            state
                .set_track_locked(aria_models::TrackId::from_raw(lane), true)
                .expect("lock the top lane");
        }
        state.selection = clips.to_vec();
        state.sync_from_engine();
        let placed = {
            let mut engine = state.engine().lock();
            placements(&mut engine, &clips, state.playhead).0.len()
        };
        assert_eq!(placed, 2, "driven {driven}: the held clip is not placed");
        state.set_align_to(AlignTo::Frame);
        let top = state.ui_clip(clips[2]).expect("the top clip").transform;
        state.align_selected(AlignEdge::Left).expect("align lands");
        assert_eq!(
            state.ui_clip(clips[2]).expect("the top clip").transform,
            top,
            "driven {driven}: the held clip was written"
        );
    }
}

#[test]
fn an_inert_press_writes_nothing_and_says_why() {
    let cases: [(&str, fn(&mut AppState, [ClipId; 3])); 3] = [
        ("nothing selected", |state, _| state.selection.clear()),
        ("two to distribute", |state, clips| {
            state.selection = clips[..DISTRIBUTE_MIN - 1].to_vec();
        }),
        ("already aligned", |state, _| {
            state.set_align_to(AlignTo::Frame);
            state
                .align_selected(AlignEdge::Left)
                .expect("first press lands");
        }),
    ];
    for (name, dress) in cases {
        let (mut state, clips) = spread_three();
        dress(&mut state, clips);
        state.sync_from_engine();
        state.status_message = None;
        let watch = Watch::on(&state);
        let pressed = if name == "two to distribute" {
            state.distribute_selected(AlignAxis::Horizontal)
        } else {
            state.align_selected(AlignEdge::Left)
        };
        pressed.expect("an inert press is not an error");
        assert!(
            watch.take().is_empty(),
            "{name}: the press reached the engine"
        );
        assert!(state.status_message.is_some(), "{name}: nothing says why");
    }
}
