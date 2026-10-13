//! Every keyframe a clip carries moves when the clip does.
//!
//! WHY: a rebase that misses a carrier is invisible. The curve still renders,
//! at the wrong time, on the half of a split that was supposed to keep it — the
//! grade, the mask and the LUT were all in that state, and no test could see it
//! because every test named the carriers it knew about, exactly like the code
//! it was checking.
//!
//! So these cases do not name carriers. They animate a clip, serialize it, walk
//! the whole document for `"kf"` arrays, rebase the clip, and compare the two
//! documents key for key at every path a curve was found at. A carrier added to
//! `Clip` or to any look it holds appears in that document by existing, so it is
//! covered without an edit here, and a carrier the walk forgets fails with the
//! path that did not move.
//!
//! What this does not catch: a curve that is not serialized (there are none —
//! the project file is the model), and a wrong *value* at a right tick. It also
//! cannot tell a deliberately-still curve from a forgotten one, which is why
//! [`HELD`] names the exceptions and why each one carries its reason.

use crate::{
    ChromaKey, Clip, ColorAdjustments, CropRect, Easing, EffectInstance, Filter, Generator,
    Keyframe, LayerBackground, LayerGlow, LayerOutline, LayerShadow, LayerStyles, Lut, Mask,
    MaskKind, MatteSource, MediaId, ModelError, Param, Project, Rational, RationalTime,
    SPEED_CURVE_SCALE, Shape, ShapeStroke, TextBackground, TextShadow, TextStroke, TextStyle,
    TimeRange, TrackKind, TrackMatte,
};
use serde_json::Value;

const R24: Rational = Rational::FPS_24;

/// How far the fixture is rebased. Odd and non-round so an off-by-a-frame or a
/// doubled application is visible in the failure.
const DELTA: i64 = 37;

/// Serialized field names whose keys are addressed in something other than
/// clip-relative timeline ticks, and must therefore hold still.
///
/// `speed_curve` keys live on the normalized speed domain: `0` is the clip's
/// start and `SPEED_CURVE_SCALE` its end, so a rebase would stretch the ramp
/// rather than move it.
const HELD: &[&str] = &["speed_curve"];

fn animated_f32(value: f32) -> Param<f32> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, value, Easing::Linear),
            Keyframe::new(12, value * 0.5, Easing::Linear),
        ],
    }
}

fn animated_vec2(value: [f32; 2]) -> Param<[f32; 2]> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe::new(1, value, Easing::Linear),
            Keyframe::new(13, [value[1], value[0]], Easing::Linear),
        ],
    }
}

fn animated_rgba() -> Param<[u8; 4]> {
    Param::Keyframed {
        keyframes: vec![
            Keyframe::new(2, [10, 20, 30, 255], Easing::Linear),
            Keyframe::new(14, [40, 50, 60, 255], Easing::Linear),
        ],
    }
}

/// A media clip with every curve it can hold keyframed.
///
/// Deliberately absurd as a look: the point is that nothing is left constant,
/// because a constant is invisible to the walk.
fn everything_animated() -> Clip {
    let mut clip = Clip::from_media(
        MediaId::from_raw(1),
        TimeRange::at_rate(0, 96, R24),
        TimeRange::at_rate(240, 96, R24),
    );
    clip.transform.position = animated_vec2([0.1, 0.2]);
    clip.transform.anchor_point = animated_vec2([0.3, 0.4]);
    clip.transform.rotation = animated_f32(15.0);
    clip.transform.opacity = animated_f32(0.8);
    clip.volume = animated_f32(0.9);
    clip.pan = animated_f32(0.25);
    clip.crop = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(3, CropRect::FULL, Easing::Linear),
            Keyframe::new(15, CropRect::FULL, Easing::Linear),
        ],
    };
    // A ramp, to prove the one carrier that must not move does not move.
    clip.speed_curve = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, 1.0, Easing::Linear),
            Keyframe::new(SPEED_CURVE_SCALE, 2.0, Easing::Linear),
        ],
    };
    let mut effect = EffectInstance::new("gaussian_blur");
    effect
        .params
        .insert("radius".to_string(), animated_f32(4.0));
    clip.effects = vec![effect];
    // Unregistered on purpose: an insert keeps its curves whether or not its
    // processor is loaded, so the walk has to move them either way.
    let mut insert = crate::AudioEffectInstance::new("example.compressor");
    insert
        .params
        .insert("threshold".to_string(), animated_f32(-12.0));
    clip.audio_effects = vec![insert];
    clip.styles = LayerStyles {
        shadow: Some(LayerShadow {
            rgba: animated_rgba(),
            offset: animated_vec2([4.0, 4.0]),
            blur: animated_f32(8.0),
        }),
        glow: Some(LayerGlow {
            rgba: animated_rgba(),
            radius: animated_f32(12.0),
            intensity: animated_f32(1.0),
        }),
        outline: Some(LayerOutline {
            rgba: animated_rgba(),
            width: animated_f32(4.0),
        }),
        background: Some(LayerBackground {
            rgba: animated_rgba(),
            padding: animated_f32(12.0),
            radius: animated_f32(2.0),
        }),
    };
    let mut adjust = ColorAdjustments::default();
    for slider in ColorAdjustments::SLIDERS {
        *(slider.get_mut)(&mut adjust) = animated_f32(slider.neutral + 0.1);
    }
    clip.adjust = adjust;
    clip.mask = Some(Mask {
        kind: MaskKind::Rectangle,
        feather: animated_f32(0.2),
        center: animated_vec2([0.0, 0.1]),
        size: animated_vec2([0.9, 0.8]),
        rotation: animated_f32(10.0),
        roundness: animated_f32(0.3),
        invert: false,
    });
    clip.chroma_key = Some(ChromaKey {
        rgb: [0, 255, 0],
        strength: animated_f32(0.4),
        shadow: animated_f32(0.2),
    });
    clip.matte = Some(TrackMatte {
        source: MatteSource::Alpha,
        invert: false,
        softness: animated_f32(0.5),
    });
    clip.filter = Some(Filter {
        id: "vivid".to_string(),
        intensity: animated_f32(0.7),
    });
    clip.lut = Some(Lut {
        path: "/tmp/aria-census.cube".to_string(),
        intensity: animated_f32(0.6),
    });
    // The clip a render-and-replace put aside, animated too.
    let mut baked = Clip::from_media(
        MediaId::from_raw(2),
        TimeRange::at_rate(0, 96, R24),
        TimeRange::at_rate(240, 96, R24),
    );
    baked.transform.opacity = animated_f32(0.5);
    clip.bake = Some(Box::new(baked));
    clip
}

/// Every `"kf"` array in `value`, as `(path, ticks)`.
fn curves(value: &Value, path: &mut Vec<String>, into: &mut Vec<(String, Vec<i64>)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "kf" {
                    let ticks = child
                        .as_array()
                        .expect("a keyframe list is an array")
                        .iter()
                        .map(|key| key["t"].as_i64().expect("a keyframe carries a tick"))
                        .collect();
                    into.push((path.join("."), ticks));
                    continue;
                }
                path.push(key.to_string());
                curves(child, path, into);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                path.push(index.to_string());
                curves(child, path, into);
                path.pop();
            }
        }
        _ => {}
    }
}

fn curves_of(clip: &Clip) -> Vec<(String, Vec<i64>)> {
    let value = serde_json::to_value(clip).expect("a clip serializes");
    let mut found = Vec::new();
    curves(&value, &mut Vec::new(), &mut found);
    found
}

fn holds_still(path: &str) -> bool {
    HELD.iter()
        .any(|held| path.split('.').any(|seg| seg == *held))
}

#[test]
fn every_curve_a_clip_carries_is_rebased_with_it() {
    let mut clip = everything_animated();
    let before = curves_of(&clip);
    // The fixture is the coverage: if it stops animating carriers, the walk has
    // nothing to prove.
    assert!(
        before.len() > 40,
        "the fixture only animates {} curves; it is supposed to animate every \
         carrier a clip has",
        before.len()
    );

    clip.shift_timeline_params(DELTA)
        .expect("a rebase inside the range of time");
    let after = curves_of(&clip);
    assert_eq!(
        before.len(),
        after.len(),
        "a rebase changed which curves exist"
    );

    let mut moved = 0;
    for ((path, was), (same_path, now)) in before.iter().zip(&after) {
        assert_eq!(
            path, same_path,
            "the walk visited two documents differently"
        );
        if holds_still(path) {
            assert_eq!(was, now, "{path} is declared held and moved anyway");
            continue;
        }
        let expected: Vec<i64> = was.iter().map(|tick| tick + DELTA).collect();
        assert_eq!(
            now, &expected,
            "{path} did not move with the clip: add it to its type's walk_params"
        );
        moved += 1;
    }
    assert_eq!(
        moved,
        before.len() - HELD.len(),
        "every curve but the declared exceptions should have moved"
    );
}

#[test]
fn a_rebase_past_the_end_of_time_moves_nothing() {
    // The overflow is planted on the LAST carrier the walk reaches while every
    // earlier one has room, which is the only shape that can tell an atomic
    // rebase from one that gives up halfway: with the whole clip out of range
    // the first curve already refuses and there is nothing to half-apply.
    let mut clip = everything_animated();
    let late = clip.bake.as_mut().expect("the fixture bakes");
    late.transform.opacity = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(i64::MAX - 8, 1.0, Easing::Linear),
            Keyframe::new(i64::MAX - 4, 0.0, Easing::Linear),
        ],
    };
    let before = curves_of(&clip);
    assert_eq!(
        clip.shift_timeline_params(1_000),
        Err(ModelError::TimeOverflow)
    );
    assert_eq!(
        curves_of(&clip),
        before,
        "a refused rebase left the carriers it had already reached moved"
    );
}

#[test]
fn every_curve_a_clip_carries_is_resampled_with_it() {
    let mut clip = everything_animated();
    let before = curves_of(&clip);
    clip.resample_timeline_params(R24, Rational::new(48, 1));
    let after = curves_of(&clip);
    for ((path, was), (_, now)) in before.iter().zip(&after) {
        if holds_still(path) {
            assert_eq!(was, now, "{path} is declared held and was resampled anyway");
            continue;
        }
        // 24 → 48 fps doubles every tick; strictly increasing ticks stay so.
        let expected: Vec<i64> = was.iter().map(|tick| tick * 2).collect();
        assert_eq!(
            now, &expected,
            "{path} was not resampled with the clip: add it to its type's walk_params"
        );
    }
}

#[test]
fn splitting_a_graded_clip_keeps_the_grade_on_the_picture() {
    // The behaviour the walk exists for, through the operator's own path: a
    // split, then the same grade sampled at the same sequence tick.
    let mut project = Project::new("grade", R24);
    let media = project.add_media(crate::MediaSource::new(
        "/tmp/aria-grade.mp4",
        1920,
        1080,
        R24,
        480,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 96, R24),
            RationalTime::new(0, R24),
        )
        .expect("place the clip");
    project
        .set_clip_adjustments(clip, {
            let mut adjust = ColorAdjustments::default();
            adjust.brightness = Param::Keyframed {
                keyframes: vec![
                    Keyframe::new(0, 0.0, Easing::Linear),
                    Keyframe::new(96, 1.0, Easing::Linear),
                ],
            };
            adjust
        })
        .expect("grade the clip");

    // Sample the grade two thirds of the way in, before and after a split at
    // half. The tail is a new clip whose curve is rebased to its own start, so
    // the value at the same sequence tick has to be the same value.
    let at = 64;
    let before = project
        .clip(clip)
        .expect("the clip is there")
        .adjust
        .brightness
        .sample(at);
    project
        .split_clip(clip, RationalTime::new(48, R24))
        .expect("split at half");
    let tail = project
        .timeline()
        .track(track)
        .expect("the lane")
        .clips()
        .find(|c| c.timeline.start.value == 48)
        .expect("a tail clip");
    let after = tail.adjust.brightness.sample(at - 48);
    assert!(
        (before - after).abs() < 1e-6,
        "the grade slid against the picture: {before} became {after}"
    );
}

/// A title with every curve its style can hold keyframed.
///
/// The background card is the point: the per-variant rebase this replaced
/// listed the stroke and the shadow and not the card, so a title whose card
/// animated kept that animation where the split found it.
fn animated_title() -> Clip {
    let style = TextStyle {
        size: animated_f32(90.0),
        fill: animated_rgba(),
        letter_spacing: animated_f32(1.0),
        line_spacing: animated_f32(1.2),
        stroke: Some(TextStroke {
            rgba: animated_rgba(),
            width: animated_f32(3.0),
        }),
        background: Some(TextBackground {
            rgba: animated_rgba(),
            radius: animated_f32(0.4),
        }),
        shadow: Some(TextShadow {
            rgba: animated_rgba(),
            blur: animated_f32(0.2),
            distance: animated_f32(6.0),
        }),
        ..TextStyle::default()
    };
    Clip::generated(
        Generator::Text {
            content: "census".to_string(),
            style,
        },
        TimeRange::at_rate(0, 96, R24),
    )
}

/// A star with every curve its geometry and its outline can hold keyframed.
fn animated_shape() -> Clip {
    Clip::generated(
        Generator::Shape {
            shape: Shape::Star {
                points: 5,
                inner_ratio: animated_f32(0.5),
            },
            rgba: animated_rgba(),
            width: animated_f32(320.0),
            height: animated_f32(320.0),
            corner_radius: animated_f32(8.0),
            stroke: Some(ShapeStroke {
                rgba: animated_rgba(),
                width: animated_f32(4.0),
            }),
        },
        TimeRange::at_rate(0, 96, R24),
    )
}

/// Rebase `clip` and assert every curve in its serialized form moved by
/// [`DELTA`], the same way the media-clip cases assert it.
fn every_curve_moves(mut clip: Clip, least: usize) {
    let before = curves_of(&clip);
    assert!(
        before.len() >= least,
        "the fixture animates {} curves, fewer than the {least} it is supposed to",
        before.len()
    );
    clip.shift_timeline_params(DELTA)
        .expect("a rebase inside the range of time");
    let after = curves_of(&clip);
    assert_eq!(
        before.len(),
        after.len(),
        "a rebase changed which curves exist"
    );
    for ((path, was), (same_path, now)) in before.iter().zip(&after) {
        assert_eq!(
            path, same_path,
            "the walk visited two documents differently"
        );
        let expected: Vec<i64> = was.iter().map(|tick| tick + DELTA).collect();
        assert_eq!(
            now, &expected,
            "{path} did not move with the clip: add it to its type's walk_params"
        );
    }
}

#[test]
fn every_curve_generated_content_carries_is_rebased_with_it() {
    // 11 for the style, 7 for the star: named so a carrier that stops being
    // walked reads as a count rather than as a silently shorter document.
    every_curve_moves(animated_title(), 11);
    every_curve_moves(animated_shape(), 7);
}

#[test]
fn every_curve_generated_content_carries_is_resampled_with_it() {
    for mut clip in [animated_title(), animated_shape()] {
        let before = curves_of(&clip);
        clip.resample_timeline_params(R24, Rational::new(48, 1));
        for ((path, was), (_, now)) in before.iter().zip(&curves_of(&clip)) {
            let expected: Vec<i64> = was.iter().map(|tick| tick * 2).collect();
            assert_eq!(
                now, &expected,
                "{path} was not resampled with the clip: add it to its type's walk_params"
            );
        }
    }
}

/// The fixture is the coverage, so the fixture is derived from the source:
/// every `Param` field the text style module declares has to be animated here,
/// which makes a new curve visible as a failure rather than as an omission.
///
/// What this does not catch: a curve on a type the fixture does not build at
/// all. Both optional treatments are `Some` above for exactly that reason.
#[test]
fn the_title_fixture_animates_every_curve_the_text_style_declares() {
    let declared = include_str!("clip/text.rs")
        .lines()
        .filter(|line| {
            let code = line.trim_start();
            code.starts_with("pub ") && code.contains(": Param<")
        })
        .count();
    let animated = curves_of(&animated_title()).len();
    assert_eq!(
        animated, declared,
        "clip/text.rs declares {declared} curves and the fixture animates {animated}: \
         animate the new one here, and add it to TextStyle's walk_params"
    );
}

#[test]
fn splitting_a_title_keeps_its_card_on_the_words() {
    // The operator's path for the carrier the old rebase forgot: a card whose
    // corner radius animates, split in half, sampled at the same sequence tick.
    let mut project = Project::new("card", R24);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "hello".to_string(),
                style: TextStyle {
                    background: Some(TextBackground {
                        rgba: Param::Constant([0, 0, 0, 255]),
                        radius: Param::Keyframed {
                            keyframes: vec![
                                Keyframe::new(0, 0.0, Easing::Linear),
                                Keyframe::new(96, 1.0, Easing::Linear),
                            ],
                        },
                    }),
                    ..TextStyle::default()
                },
            },
            TimeRange::at_rate(0, 96, R24),
        )
        .expect("place the title");

    let at = 64;
    let before = card_radius(&project, clip).sample(at);
    project
        .split_clip(clip, RationalTime::new(48, R24))
        .expect("split at half");
    let tail = project
        .timeline()
        .track(track)
        .expect("the lane")
        .clips()
        .find(|c| c.timeline.start.value == 48)
        .expect("a tail clip")
        .id;
    let after = card_radius(&project, tail).sample(at - 48);
    assert!(
        (before - after).abs() < 1e-6,
        "the card slid against the words: {before} became {after}"
    );
}

/// The animated corner radius of a title's background card.
fn card_radius(project: &Project, clip: crate::ClipId) -> Param<f32> {
    let crate::ClipSource::Generated(Generator::Text { style, .. }) =
        &project.clip(clip).expect("the clip is there").content
    else {
        panic!("the fixture is a title");
    };
    style
        .background
        .as_ref()
        .expect("the fixture carries a card")
        .radius
        .clone()
}
