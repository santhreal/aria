//! A lane bar's look is animated in sequence time.
//!
//! A lane bar has no geometry: it reprocesses whatever is composited beneath
//! its track, over the stretch of timeline it covers. Its effect stack and its
//! grade were sampled at a clip-local tick like a picture clip's transform, so
//! the same bar dragged two seconds later played its look two seconds later —
//! a look pinned to the bar rather than to the programme under it.
//!
//! The generator catalog now declares which convention a generator's
//! parameters are addressed in ([`aria_models::ParamTime`]), and one clip
//! method converts in both directions. A non-painting generator addresses its
//! keyframes in sequence ticks: the write stores the absolute tick, the
//! resolver samples at it, and moving the bar does not move its look.
//!
//! **The discriminating operation is a move, and nothing else.** Two bars at
//! different starts carrying keyframes written at the same absolute ticks
//! resolve identically under *either* convention — clip-local storage differs
//! from absolute storage by exactly the clip's start, which is exactly what
//! clip-local sampling then subtracts again. A test built that way passes
//! whatever the column says, so every case here keyframes one clip and then
//! drags it.
//!
//! What these cases close, beyond the reported defect:
//!
//! - The variant space is read from the catalog at run time, so a fourth lane
//!   bar is covered the day it is added rather than the day someone remembers
//!   this file.
//! - Both sides of the pairing are asserted: a painting clip's look still
//!   rides with the clip, so "make everything sequence-timed" fails here.
//! - Effect parameters and the grade are both checked, since the bar carries
//!   both and an earlier defect dropped one of them per variant.
//! - Sample ticks avoid the midpoint of a symmetric ramp. A grade that
//!   interpolates through neutral is elided from the scene, so a case that
//!   reads the zero crossing cannot tell "not animated" from "not there".
//!
//! What they do not catch: nothing here renders pixels. That a grade reaches
//! the GPU is the compositor's conformance suite; this file proves the values
//! the resolver hands it.

use super::tests::*;
use crate::grade::effective_grade;
use aria_models::{ClipParam, LookParam, ParamValue, Rational, TimeRange, TrackKind};

const FPS: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, FPS)
}

fn approx(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-3, "expected ~{b}, got {a}");
}

/// A red fill on a sticker lane with one lane bar over it, placed at `bar`.
///
/// The fill is what a lane bar exists to reprocess, so every case here has
/// something underneath: a bar over nothing is a different (already covered)
/// question.
fn with_bar(kind: TrackKind, generator: Generator, bar: TimeRange) -> (Project, ClipId) {
    let mut project = Project::new("lane keyframes", FPS);
    let base = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            base,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 300),
        )
        .expect("the fill");
    let lane = project.add_track(kind, "L1");
    let id = project
        .add_generated(lane, generator, bar)
        .expect("the bar");
    (project, id)
}

/// Write one scalar keyframe at an absolute timeline tick.
fn key(project: &mut Project, clip: ClipId, param: ClipParam, at: i64, value: f32) {
    project
        .set_param_keyframe(
            clip,
            param,
            rt(at),
            ParamValue::Scalar(value),
            Easing::Linear,
            None,
        )
        .expect("keyframe");
}

/// Drag `clip` to a new start on the lane it already sits on.
fn move_to(project: &mut Project, clip: ClipId, start: i64) {
    let track = project
        .timeline()
        .track_of(clip)
        .expect("the clip is on a track");
    project.move_clip(clip, track, rt(start)).expect("the move");
}

const SATURATION: ClipParam = ClipParam::Look {
    param: LookParam::AdjustSaturation,
};

/// The lane-bar rows, read from the catalog rather than named here: the ones
/// that paint nothing and therefore address their parameters in sequence time.
fn lane_bars() -> Vec<(TrackKind, Generator)> {
    aria_models::generator_specs()
        .iter()
        .filter(|spec| !spec.paints)
        .map(|spec| (spec.lane, (spec.census)()))
        .collect()
}

/// The lane pass at `tick`, or `None` when the scene carries none.
fn pass_at(project: &Project, tick: i64) -> Option<SceneLayer> {
    let scene = resolve(project, rt(tick)).expect("resolve");
    scene
        .layers
        .iter()
        .find(|layer| layer.source == LayerSource::CanvasPass)
        .cloned()
}

/// The grade the pass carries at `tick`.
fn grade_at(project: &Project, tick: i64) -> Option<aria_compositor::ColorGrade> {
    pass_at(project, tick).expect("the lane pass").color_grade
}

/// The grade one clip's own layer carries at `tick`.
fn clip_grade_at(
    project: &Project,
    clip: ClipId,
    tick: i64,
) -> Option<aria_compositor::ColorGrade> {
    let scene = resolve(project, rt(tick)).expect("resolve");
    scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(clip))
        .expect("the clip's layer")
        .color_grade
}

#[test]
fn a_lane_keyframe_changes_the_frame() {
    let (mut project, bar) = with_bar(TrackKind::Adjustment, Generator::Adjustment, tr(0, 100));
    key(&mut project, bar, SATURATION, 0, -1.0);
    key(&mut project, bar, SATURATION, 60, 1.0);

    // 15 and 45, not 30: a ramp from -1 to 1 is exactly neutral at its
    // midpoint, and a neutral grade is elided from the scene.
    let start = grade_at(&project, 0).expect("a grade at the first keyframe");
    let quarter = grade_at(&project, 15).expect("a grade a quarter in");
    let end = grade_at(&project, 60).expect("a grade at the last keyframe");

    assert_ne!(
        start, end,
        "the lane keyframes did not change the resolved grade"
    );
    assert_ne!(
        start, quarter,
        "the segment between them is not interpolated"
    );
    assert_ne!(quarter, end, "the segment between them is not interpolated");
    assert_eq!(
        end,
        effective_grade(
            None,
            &ColorAdjustments {
                saturation: 1.0.into(),
                ..ColorAdjustments::default()
            },
        ),
        "the last keyframe's value is not what the pass carries"
    );
}

#[test]
fn a_lane_bars_look_stays_where_the_programme_is_when_the_bar_moves() {
    let (mut project, bar) = with_bar(TrackKind::Adjustment, Generator::Adjustment, tr(0, 150));
    key(&mut project, bar, SATURATION, 40, -1.0);
    key(&mut project, bar, SATURATION, 100, 1.0);
    let before: Vec<_> = [40, 55, 100]
        .into_iter()
        .map(|tick| grade_at(&project, tick))
        .collect();

    // The same bar, forty ticks later, still covering every sampled tick.
    move_to(&mut project, bar, 40);

    for (index, tick) in [40, 55, 100].into_iter().enumerate() {
        assert_eq!(
            grade_at(&project, tick),
            before[index],
            "the bar's look moved with the bar at tick {tick}"
        );
    }
}

#[test]
fn every_lane_bar_variant_keeps_both_its_effect_stack_and_its_grade_in_place() {
    let variants = lane_bars();
    assert!(
        !variants.is_empty(),
        "the catalog declares no non-painting generator"
    );
    for (kind, generator) in variants {
        let (mut project, bar) = with_bar(kind, generator, tr(0, 150));
        project.add_effect(bar, "gaussian_blur").expect("effect");
        let blur = ClipParam::Effect {
            effect: 0,
            param: 0,
        };
        key(&mut project, bar, blur, 40, 2.0);
        key(&mut project, bar, blur, 100, 8.0);
        key(&mut project, bar, SATURATION, 40, -1.0);
        key(&mut project, bar, SATURATION, 100, 1.0);

        let ticks = [40, 55, 100];
        let before: Vec<_> = ticks
            .into_iter()
            .map(|tick| {
                let pass = pass_at(&project, tick).expect("the lane pass");
                (pass.color_grade, pass.effects[0].params[0])
            })
            .collect();
        // The parameter is animated, so the comparison below is not vacuous.
        for (index, tick) in ticks.into_iter().enumerate() {
            approx(before[index].1, 2.0 + 6.0 * ((tick - 40) as f32 / 60.0));
        }

        move_to(&mut project, bar, 40);

        for (index, tick) in ticks.into_iter().enumerate() {
            let pass = pass_at(&project, tick).expect("the lane pass");
            assert_eq!(
                pass.color_grade, before[index].0,
                "{kind:?} moved its grade with the bar at tick {tick}"
            );
            approx(pass.effects[0].params[0], before[index].1);
        }
    }
}

#[test]
fn a_painting_clips_look_still_rides_with_the_clip() {
    // The other side of the pairing. A solid fill paints, so its keyframes stay
    // clip-relative: dragging the fill drags its animation along, and the value
    // that was at tick 40 is at tick 80 once the clip has moved forty on.
    let mut project = Project::new("clip local", FPS);
    let lane = project.add_track(TrackKind::Sticker, "S1");
    let fill = project
        .add_generated(
            lane,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 150),
        )
        .expect("the fill");
    key(&mut project, fill, SATURATION, 40, -1.0);
    key(&mut project, fill, SATURATION, 100, 1.0);
    let before: Vec<_> = [40, 55, 100]
        .into_iter()
        .map(|tick| clip_grade_at(&project, fill, tick))
        .collect();

    move_to(&mut project, fill, 40);

    for (index, tick) in [40, 55, 100].into_iter().enumerate() {
        assert_eq!(
            clip_grade_at(&project, fill, tick + 40),
            before[index],
            "a painting clip's look stopped riding with the clip"
        );
    }
    // The first keyframe's value is held before it, so tick 40 reads the same
    // either way and cannot tell a shift from a hold. The ticks inside the ramp
    // can.
    for (index, tick) in [40, 55, 100]
        .into_iter()
        .enumerate()
        .filter(|(_, tick)| *tick > 40)
    {
        assert_ne!(
            clip_grade_at(&project, fill, tick),
            before[index],
            "a painting clip's look stayed behind when the clip moved"
        );
    }
}

#[test]
fn a_lane_bar_stores_its_keyframes_at_the_sequence_tick() {
    // The stored tick is the convention the resolver depends on, so it is
    // asserted directly: a bar at 40 keyframed at 50 stores 50, not 10.
    let (mut project, bar) = with_bar(TrackKind::Effect, Generator::Effect, tr(40, 150));
    key(&mut project, bar, SATURATION, 50, -1.0);
    key(&mut project, bar, SATURATION, 80, 1.0);

    let clip = project.clip(bar).expect("the bar");
    let spec = aria_models::clip_params_for(clip)
        .into_iter()
        .find(|spec| spec.param == SATURATION)
        .expect("the saturation row");
    let ticks: Vec<i64> = spec.keyframes(clip).into_iter().map(|(t, _)| t).collect();
    assert_eq!(ticks, vec![50, 80]);
}

/// A painting clip stores clip-relative ticks, which is the other half of the
/// storage claim above: the same absolute keyframe on a clip that starts at 40
/// is stored at 10.
#[test]
fn a_painting_clip_stores_its_keyframes_clip_relative() {
    let mut project = Project::new("clip local", FPS);
    let lane = project.add_track(TrackKind::Sticker, "S1");
    let fill = project
        .add_generated(
            lane,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(40, 150),
        )
        .expect("the fill");
    key(&mut project, fill, SATURATION, 50, -1.0);
    key(&mut project, fill, SATURATION, 80, 1.0);

    let clip = project.clip(fill).expect("the fill");
    let spec = aria_models::clip_params_for(clip)
        .into_iter()
        .find(|spec| spec.param == SATURATION)
        .expect("the saturation row");
    let ticks: Vec<i64> = spec.keyframes(clip).into_iter().map(|(t, _)| t).collect();
    assert_eq!(ticks, vec![10, 40]);
}
