//! `shape_param` cases.

use super::*;

#[test]
fn shape_param_routing_rejects_wrong_targets() {
    let mut rect = Generator::shape(Shape::Rectangle, [255, 255, 255, 255]);
    // Star-only param on a rectangle.
    assert!(
        rect.set_shape_param_constant(ShapeParam::InnerRatio, ParamValue::Scalar(0.5))
            .is_err()
    );
    // Stroke params with no stroke set.
    assert!(
        rect.set_shape_param_constant(ShapeParam::StrokeWidth, ParamValue::Scalar(4.0))
            .is_err()
    );
    // Kind mismatches both ways.
    assert!(
        rect.set_shape_param_constant(ShapeParam::Width, ParamValue::Color([0, 0, 0, 255]))
            .is_err()
    );
    assert!(
        rect.set_shape_param_constant(ShapeParam::Fill, ParamValue::Scalar(1.0))
            .is_err()
    );
    // Out-of-range values are rejected at the routing boundary.
    assert!(
        rect.set_shape_param_constant(ShapeParam::Width, ParamValue::Scalar(-5.0))
            .is_err()
    );
    // Non-shape generators reject everything.
    let mut text = Generator::text("hi");
    assert!(
        text.set_shape_param_constant(ShapeParam::Width, ParamValue::Scalar(10.0))
            .is_err()
    );
}

#[test]
fn text_style_roundtrips_through_serde() {
    let style = TextStyle {
        font: "Helvetica".into(),
        size: 120.0.into(),
        bold: true,
        italic: true,
        underline: true,
        case: TextCase::Upper,
        fill: [10, 20, 30, 255].into(),
        letter_spacing: 3.0.into(),
        kerning: false,
        line_spacing: 1.5.into(),
        align_h: TextAlignH::Justify,
        align_v: TextAlignV::Bottom,
        wrap: false,
        stroke: Some(TextStroke {
            rgba: [0, 0, 0, 255].into(),
            width: 8.0.into(),
        }),
        background: Some(TextBackground {
            rgba: [255, 255, 0, 200].into(),
            radius: 0.5.into(),
        }),
        shadow: Some(TextShadow {
            rgba: [0, 0, 0, 230].into(),
            blur: 0.25.into(),
            distance: 12.0.into(),
        }),
        effect_preset: None,
        runs: Vec::new(),
    };
    let clip = Clip::generated(
        Generator::Text {
            content: "Styled".into(),
            style: style.clone(),
        },
        tr(0, 24, R24),
    );
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    match loaded.content {
        ClipSource::Generated(Generator::Text {
            content,
            style: got,
        }) => {
            assert_eq!(content, "Styled");
            assert_eq!(got, style);
        }
        other => panic!("expected text generator, got {other:?}"),
    }
}

/// A style saved before the kerning switch existed has no `kerning` key and
/// loads with kerning on; a style with kerning on saves no key, so a file
/// written now still opens in a build that predates the switch.
#[test]
fn kerning_is_on_when_absent_and_only_written_when_off() {
    let loaded: TextStyle = serde_json::from_str("{}").expect("empty style");
    assert!(loaded.kerning);
    let on = serde_json::to_value(TextStyle::default()).expect("serialize");
    assert!(on.get("kerning").is_none(), "kerning on was written: {on}");
    let off = serde_json::to_value(TextStyle {
        kerning: false,
        ..TextStyle::default()
    })
    .expect("serialize");
    assert_eq!(off.get("kerning"), Some(&serde_json::Value::Bool(false)));
}

#[test]
fn legacy_bare_text_background_json_loads_as_constants() {
    // Pre-Param saves wrote bare rgba/radius; Constant serializes identically.
    let json = r#"{"rgba":[0,0,0,255],"radius":0.5}"#;
    let background: TextBackground = serde_json::from_str(json).expect("legacy background");
    assert_eq!(
        background,
        TextBackground {
            rgba: Param::Constant([0, 0, 0, 255]),
            radius: Param::Constant(0.5),
        }
    );
}

#[test]
fn keyframed_text_background_rgba_roundtrips() {
    let background = TextBackground {
        rgba: Param::Keyframed {
            keyframes: vec![
                Keyframe {
                    tick: 0,
                    value: [0, 0, 0, 255],
                    easing: Easing::Linear,
                    tangents: None,
                },
                Keyframe {
                    tick: 10,
                    value: [255, 0, 0, 255],
                    easing: Easing::Linear,
                    tangents: None,
                },
            ],
        },
        radius: Param::Constant(0.25),
    };
    let json = serde_json::to_string(&background).expect("serialize");
    let loaded: TextBackground = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded, background);
    assert_eq!(loaded.rgba.sample(5), [128, 0, 0, 255]);
}

#[test]
fn text_generator_validation_rejects_unsafe_metrics() {
    let invalid = |style| {
        Generator::Text {
            content: "Unsafe".into(),
            style,
        }
        .validate()
        .is_err()
    };

    assert!(invalid(TextStyle {
        size: f32::NAN.into(),
        ..Default::default()
    }));
    assert!(invalid(TextStyle {
        line_spacing: 0.0.into(),
        ..Default::default()
    }));
    assert!(invalid(TextStyle {
        stroke: Some(TextStroke {
            rgba: [0, 0, 0, 255].into(),
            width: 513.0.into(),
        }),
        ..Default::default()
    }));
    assert!(invalid(TextStyle {
        background: Some(TextBackground {
            rgba: [0, 0, 0, 255].into(),
            radius: 1.01.into(),
        }),
        ..Default::default()
    }));
    assert!(invalid(TextStyle {
        shadow: Some(TextShadow {
            rgba: [0, 0, 0, 255].into(),
            blur: 0.2.into(),
            distance: (-1.0).into(),
        }),
        ..Default::default()
    }));

    Generator::Text {
        content: "Safe".into(),
        style: TextStyle {
            stroke: Some(TextStroke {
                rgba: [0, 0, 0, 255].into(),
                width: 40.0.into(),
            }),
            background: Some(TextBackground {
                rgba: [0, 0, 0, 200].into(),
                radius: 1.0.into(),
            }),
            shadow: Some(TextShadow {
                rgba: [0, 0, 0, 230].into(),
                blur: 1.0.into(),
                distance: 50.0.into(),
            }),
            ..Default::default()
        },
    }
    .validate()
    .expect("inspector effect limits are valid");
}

#[test]
fn text_case_apply() {
    assert_eq!(TextCase::Normal.apply("Hello World"), "Hello World");
    assert_eq!(TextCase::Upper.apply("Hello World"), "HELLO WORLD");
    assert_eq!(TextCase::Lower.apply("Hello World"), "hello world");
    assert_eq!(TextCase::Title.apply("hello world"), "Hello World");
    assert_eq!(TextCase::Title.apply("hELLO  wORLD"), "Hello  World");
}

#[test]
fn transform_validation() {
    assert!(ClipTransform::IDENTITY.validate().is_ok());
    assert!(
        ClipTransform {
            position: [0.4, -0.4],
            scale: 3.0.into(),
            rotation: -720.0,
            opacity: 0.0,
            ..ClipTransform::IDENTITY
        }
        .validate()
        .is_ok()
    );

    let bad_scale = ClipTransform {
        scale: Scale2::uniform(-0.5),
        ..ClipTransform::IDENTITY
    };
    assert!(matches!(
        bad_scale.validate(),
        Err(ModelError::InvalidTransform(_))
    ));

    let bad_axis = ClipTransform {
        scale: Scale2 { x: 1.0, y: 0.0 },
        ..ClipTransform::IDENTITY
    };
    assert!(matches!(
        bad_axis.validate(),
        Err(ModelError::InvalidTransform(_))
    ));

    let bad_opacity = ClipTransform {
        opacity: -0.1,
        ..ClipTransform::IDENTITY
    };
    assert!(matches!(
        bad_opacity.validate(),
        Err(ModelError::InvalidTransform(_))
    ));

    let bad_position = ClipTransform {
        position: [0.0, f32::NAN],
        ..ClipTransform::IDENTITY
    };
    assert!(matches!(
        bad_position.validate(),
        Err(ModelError::InvalidTransform(_))
    ));
}

#[test]
fn scale2_uniform_serializes_as_bare_float() {
    let s = Scale2::uniform(1.5);
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, "1.5");
    let loaded: Scale2 = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, s);
}

#[test]
fn scale2_split_serializes_as_array_and_roundtrips() {
    let s = Scale2 { x: 2.0, y: 1.0 };
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, "[2.0,1.0]");
    let loaded: Scale2 = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, s);
}

#[test]
fn legacy_keyframed_f32_scale_loads_as_uniform() {
    // Pre-Scale2 keyframed saves wrote bare f32 values in each keyframe.
    let json = r#"{"kf":[{"t":0,"v":1.0},{"t":10,"v":2.0}]}"#;
    let p: Param<Scale2> = serde_json::from_str(json).unwrap();
    assert_eq!(p.sample(0), Scale2::ONE);
    assert_eq!(p.sample(5), Scale2::uniform(1.5));
    assert_eq!(p.sample(10), Scale2::uniform(2.0));
}

#[test]
fn scale_param_accepts_scalar_and_vec2_keyframes() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 48, R24));
    ClipParam::Scale
        .set_keyframe(&mut clip, 0, ParamValue::Scalar(1.0), Easing::Linear)
        .unwrap();
    ClipParam::Scale
        .set_keyframe(&mut clip, 10, ParamValue::Vec2([2.0, 1.0]), Easing::Linear)
        .unwrap();
    let t = &clip.transform;
    assert_eq!(t.sample(0).scale, Scale2::ONE);
    assert_eq!(t.sample(5).scale, Scale2 { x: 1.5, y: 1.0 });
    assert_eq!(t.sample(10).scale, Scale2 { x: 2.0, y: 1.0 });
}

#[test]
fn scale2_lerp_and_isotropic() {
    use crate::param::Lerp;
    let a = Scale2 { x: 1.0, y: 1.0 };
    let b = Scale2 { x: 3.0, y: 5.0 };
    assert_eq!(Scale2::lerp(a, b, 0.5), Scale2 { x: 2.0, y: 3.0 });
    // Geometric mean: sqrt(4*1) = 2.
    assert!((Scale2 { x: 4.0, y: 1.0 }.isotropic() - 2.0).abs() < 1e-6);
    assert!((Scale2::uniform(2.0).isotropic() - 2.0).abs() < 1e-6);
}

// --- source_time_at: errors -------------------------------------------

#[test]
fn source_time_at_rate_mismatch_errors() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    let err = clip.source_time_at(rt(5, R30)).unwrap_err();
    assert_eq!(
        err,
        ModelError::RateMismatch {
            expected: R30,
            got: R24,
        }
    );
}
