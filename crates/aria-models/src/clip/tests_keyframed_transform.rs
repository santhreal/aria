//! `keyframed_transform` cases.

use super::*;

#[test]
fn keyframed_transform_roundtrips() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    ClipParam::Opacity
        .set_keyframe(&mut clip, 0, ParamValue::Scalar(0.0), Easing::Linear)
        .unwrap();
    ClipParam::Opacity
        .set_keyframe(&mut clip, 24, ParamValue::Scalar(1.0), Easing::EaseOut)
        .unwrap();
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.transform, clip.transform);
    assert!(loaded.transform.is_animated());
    // Segment 0→24 leaves the linear keyframe at tick 0: halfway = 0.5.
    assert_eq!(loaded.transform.sample(12).opacity, 0.5);
}

#[test]
fn animated_transform_samples_per_property() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    ClipParam::Scale
        .set_keyframe(&mut clip, 0, ParamValue::Scalar(1.0), Easing::Linear)
        .unwrap();
    ClipParam::Scale
        .set_keyframe(&mut clip, 10, ParamValue::Scalar(2.0), Easing::Linear)
        .unwrap();
    // Scale animates; everything else stays constant.
    let mid = clip.transform.sample(5);
    assert_eq!(mid.scale, 1.5.into());
    assert_eq!(mid.position, [0.0, 0.0]);
    assert_eq!(mid.opacity, 1.0);
}

#[test]
fn compose_at_writes_keyframe_only_on_animated_properties() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    ClipParam::Scale
        .set_keyframe(&mut clip, 0, ParamValue::Scalar(1.0), Easing::Linear)
        .unwrap();
    ClipParam::Scale
        .set_keyframe(&mut clip, 20, ParamValue::Scalar(3.0), Easing::Linear)
        .unwrap();
    let t = &mut clip.transform;

    let edit = ClipTransform {
        position: [0.3, 0.0],
        scale: 2.0.into(),
        rotation: 0.0,
        opacity: 1.0,
        ..ClipTransform::IDENTITY
    };
    t.compose_at(edit, 10);

    // Scale gained a keyframe at tick 10; the curve still animates.
    assert_eq!(t.scale.keyframes().len(), 3);
    assert_eq!(t.sample(10).scale, 2.0.into());
    assert_eq!(t.sample(0).scale, 1.0.into());
    assert_eq!(t.sample(20).scale, 3.0.into());
    // Position was constant and stays constant.
    assert!(!t.position.is_animated());
    assert_eq!(t.sample(0).position, [0.3, 0.0]);
}

#[test]
fn remove_param_keyframe_errors_when_absent() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    assert!(ClipParam::Scale.remove_keyframe(&mut clip, 5).is_err());
    ClipParam::Scale
        .set_keyframe(&mut clip, 5, ParamValue::Scalar(2.0), Easing::Linear)
        .unwrap();
    assert!(ClipParam::Scale.remove_keyframe(&mut clip, 5).is_ok());
    assert!(!clip.transform.scale.is_animated());
    assert_eq!(clip.transform.scale.constant(), Some(2.0.into()));
}

#[test]
fn param_kind_mismatch_rejected() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    // Scale accepts Scalar (uniform) and Vec2 (per-axis); Color still rejects.
    assert!(matches!(
        ClipParam::Scale.set_keyframe(
            &mut clip,
            0,
            ParamValue::Color([255, 0, 0, 255]),
            Easing::Linear
        ),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        ClipParam::Position.set_constant(&mut clip, ParamValue::Scalar(1.0)),
        Err(ModelError::InvalidParam(_))
    ));
}

/// A style parameter is written by the style axis, never by the transform.
///
/// The transform used to own the param mutators, so it had to refuse every
/// parameter that was not one of its own properties and this test asserted that
/// refusal. The registry row now names the axis, so what still has to hold is
/// the other half of the same claim: a style write that its own axis refuses —
/// here because the clip carries no shadow block — leaves the transform
/// untouched instead of landing somewhere plausible.
#[test]
fn a_style_param_write_never_reaches_the_transform() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    let before = clip.transform.clone();
    let style = ClipParam::Style {
        param: StyleParam::ShadowBlur,
    };
    assert!(matches!(
        style.set_keyframe(&mut clip, 0, ParamValue::Scalar(4.0), Easing::Linear),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        style.set_constant(&mut clip, ParamValue::Scalar(4.0)),
        Err(ModelError::InvalidParam(_))
    ));
    assert!(matches!(
        style.remove_keyframe(&mut clip, 0),
        Err(ModelError::InvalidParam(_))
    ));
    assert_eq!(clip.transform, before);
}

#[test]
fn param_values_validated_per_property() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    assert!(
        ClipParam::Scale
            .set_keyframe(&mut clip, 0, ParamValue::Scalar(-1.0), Easing::Linear)
            .is_err()
    );
    assert!(
        ClipParam::Opacity
            .set_keyframe(&mut clip, 0, ParamValue::Scalar(1.5), Easing::Linear)
            .is_err()
    );
    assert!(
        ClipParam::Position
            .set_constant(&mut clip, ParamValue::Vec2([f32::NAN, 0.0]))
            .is_err()
    );
}

#[test]
fn animation_tick_clamps_into_clip() {
    let clip = Clip::generated(Generator::Adjustment, tr(100, 50, R24));
    assert_eq!(clip.animation_tick(100), 0);
    assert_eq!(clip.animation_tick(125), 25);
    assert_eq!(clip.animation_tick(149), 49);
    assert_eq!(clip.animation_tick(90), 0);
    assert_eq!(clip.animation_tick(500), 49);
    // The fractional variant keeps sub-frame offsets and clamps the
    // same way.
    assert!((clip.animation_tick_f(125.4) - 25.4).abs() < 1e-9);
    assert_eq!(clip.animation_tick_f(99.5), 0.0);
    assert_eq!(clip.animation_tick_f(149.6), 49.0);
}

// --- text style ---------------------------------------------------------

#[test]
fn legacy_text_clip_without_style_loads_default() {
    // A title serialized before styling existed: the Text variant only had
    // a `content` field.
    let json = r#"{
        "id": 1,
        "content": { "Generated": { "Text": { "content": "old title" } } },
        "timeline": { "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
                      "duration": { "value": 24, "rate": { "num": 24, "den": 1 } } }
    }"#;
    let clip: Clip = serde_json::from_str(json).expect("deserialize legacy text clip");
    match clip.content {
        ClipSource::Generated(Generator::Text { content, style }) => {
            assert_eq!(content, "old title");
            assert_eq!(style, TextStyle::default());
        }
        other => panic!("expected text generator, got {other:?}"),
    }
}

#[test]
fn legacy_shape_without_dimensions_loads_legacy_defaults() {
    let json = r#"{
        "id": 1,
        "content": { "Generated": { "Shape": {
            "shape": "Rectangle",
            "rgba": [255, 0, 0, 255]
        } } },
        "timeline": { "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
                      "duration": { "value": 24, "rate": { "num": 24, "den": 1 } } }
    }"#;
    let clip: Clip = serde_json::from_str(json).expect("deserialize legacy shape clip");
    match clip.content {
        ClipSource::Generated(Generator::Shape {
            rgba,
            width,
            height,
            corner_radius,
            stroke,
            ..
        }) => {
            // Bare legacy values load as constants (the M2 Param trick).
            assert_eq!(rgba.constant(), Some([255, 0, 0, 255]));
            assert_eq!(width.constant(), Some(960.0));
            assert_eq!(height.constant(), Some(540.0));
            // Fields that postdate the file default to "not set".
            assert_eq!(corner_radius.constant(), Some(0.0));
            assert!(stroke.is_none());
        }
        other => panic!("expected shape generator, got {other:?}"),
    }
}

#[test]
fn never_touched_shape_serializes_byte_identical_to_legacy() {
    // A constant shape must not leak the new fields into saves: `Param`
    // constants serialize bare, corner_radius/stroke are elided.
    let generator = Generator::shape(Shape::Rectangle, [10, 20, 30, 255]);
    let json = serde_json::to_value(&generator).unwrap();
    let obj = &json["Shape"];
    assert_eq!(obj["rgba"], serde_json::json!([10, 20, 30, 255]));
    assert_eq!(obj["width"], serde_json::json!(200.0));
    assert!(
        obj.get("corner_radius").is_none(),
        "zero corner radius must be elided: {obj}"
    );
    assert!(obj.get("stroke").is_none(), "absent stroke must be elided");
}

#[test]
fn new_shape_kinds_roundtrip_with_keyframed_params() {
    let mut inner = Param::Constant(0.5);
    inner.set_keyframe(0, 0.2, Easing::Linear);
    inner.set_keyframe(24, 0.9, Easing::EaseInOut);
    let generator = Generator::Shape {
        shape: Shape::Star {
            points: 5,
            inner_ratio: inner,
        },
        rgba: Param::Constant([255, 0, 0, 255]),
        width: Param::Constant(300.0),
        height: Param::Constant(300.0),
        corner_radius: Param::Constant(4.0),
        stroke: Some(ShapeStroke::new([0, 0, 0, 255], 8.0)),
    };
    generator.validate().expect("valid star");
    let json = serde_json::to_string(&generator).unwrap();
    let back: Generator = serde_json::from_str(&json).unwrap();
    assert_eq!(back, generator);

    let path = Generator::Shape {
        shape: Shape::Path(ShapePath {
            points: vec![
                ShapePathPoint::corner([-50.0, 0.0]),
                ShapePathPoint {
                    anchor: [50.0, 0.0],
                    handle_in: [0.0, -60.0],
                    handle_out: [50.0, 0.0],
                },
            ],
            closed: false,
        }),
        rgba: Param::Constant([255, 255, 255, 255]),
        width: Param::Constant(100.0),
        height: Param::Constant(100.0),
        corner_radius: Param::Constant(0.0),
        stroke: Some(ShapeStroke::new([255, 255, 255, 255], 4.0)),
    };
    path.validate().expect("valid path");
    let json = serde_json::to_string(&path).unwrap();
    let back: Generator = serde_json::from_str(&json).unwrap();
    assert_eq!(back, path);
}

#[test]
fn generator_validate_rejects_bad_shapes() {
    let bad = |shape: Shape| Generator::shape(shape, [255, 255, 255, 255]).validate();
    assert!(bad(Shape::Polygon { sides: 2 }).is_err());
    assert!(
        bad(Shape::Star {
            points: 99,
            inner_ratio: Param::Constant(0.5)
        })
        .is_err()
    );
    assert!(
        bad(Shape::Star {
            points: 5,
            inner_ratio: Param::Constant(1.5)
        })
        .is_err(),
        "inner_ratio above 1 must be rejected"
    );
    assert!(
        bad(Shape::Path(ShapePath {
            points: vec![ShapePathPoint::corner([0.0, 0.0])],
            closed: true
        }))
        .is_err(),
        "single-point path is not drawable"
    );
    assert!(
        bad(Shape::Path(ShapePath {
            points: vec![
                ShapePathPoint::corner([0.0, f32::NAN]),
                ShapePathPoint::corner([10.0, 0.0]),
            ],
            closed: true,
        }))
        .is_err(),
        "non-finite path coordinate must be rejected"
    );

    let mut wide = Generator::shape(Shape::Rectangle, [255, 255, 255, 255]);
    if let Generator::Shape { width, .. } = &mut wide {
        *width = Param::Constant(MAX_SHAPE_DIM * 2.0);
    }
    assert!(wide.validate().is_err(), "oversized width must be rejected");

    let mut hot_stroke = Generator::shape(Shape::Ellipse, [255, 255, 255, 255]);
    if let Generator::Shape { stroke, .. } = &mut hot_stroke {
        *stroke = Some(ShapeStroke::new([0, 0, 0, 255], MAX_STROKE_WIDTH + 1.0));
    }
    assert!(hot_stroke.validate().is_err());
}

#[test]
fn shape_param_routing_sets_and_samples() {
    let mut g = Generator::shape(Shape::Rectangle, [255, 255, 255, 255]);
    g.set_shape_param_keyframe(
        ShapeParam::Width,
        0,
        ParamValue::Scalar(100.0),
        Easing::Linear,
    )
    .unwrap();
    g.set_shape_param_keyframe(
        ShapeParam::Width,
        10,
        ParamValue::Scalar(300.0),
        Easing::Linear,
    )
    .unwrap();
    g.set_shape_param_keyframe(
        ShapeParam::Fill,
        0,
        ParamValue::Color([0, 0, 0, 255]),
        Easing::Linear,
    )
    .unwrap();
    g.set_shape_param_keyframe(
        ShapeParam::Fill,
        10,
        ParamValue::Color([255, 0, 0, 255]),
        Easing::Linear,
    )
    .unwrap();
    let Generator::Shape { width, rgba, .. } = &g else {
        panic!()
    };
    assert_eq!(width.sample(5), 200.0);
    assert_eq!(rgba.sample(5), [128, 0, 0, 255], "colors lerp per channel");

    // Removing down to zero keyframes collapses to a constant.
    g.remove_shape_param_keyframe(ShapeParam::Width, 0).unwrap();
    g.remove_shape_param_keyframe(ShapeParam::Width, 10)
        .unwrap();
    let Generator::Shape { width, .. } = &g else {
        panic!()
    };
    assert!(!width.is_animated());
}

#[test]
fn text_size_param_routing_sets_and_samples() {
    let mut text = Generator::text("Animated");
    text.set_text_param_keyframe(TextParam::Size, 0, ParamValue::Scalar(80.0), Easing::Linear)
        .unwrap();
    text.set_text_param_keyframe(
        TextParam::Size,
        10,
        ParamValue::Scalar(120.0),
        Easing::Linear,
    )
    .unwrap();
    let Generator::Text { style, .. } = &text else {
        panic!()
    };
    assert_eq!(style.size.sample(5), 100.0);
}
