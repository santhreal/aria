//! `clip_without` cases.

use super::*;

#[test]
fn audio_gain_follows_a_keyframed_envelope() {
    // A 0→1 ramp envelope over the span: the gain tracks the curve.
    let env = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 0.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 100,
                value: 1.0,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    assert_eq!(audio_gain_at(0, 100, &env, 0, 0), 0.0);
    assert_eq!(audio_gain_at(50, 100, &env, 0, 0), 0.5);
    assert_eq!(audio_gain_at(100, 100, &env, 0, 0), 1.0);
    // Fades still multiply on top of the sampled envelope value.
    assert_eq!(audio_gain_at(50, 100, &env, 0, 20), 0.5);
    assert_eq!(audio_gain_at(90, 100, &env, 0, 20), 0.9 * 0.5);
}

// --- crop & flip (M1) ----------------------------------------------------

#[test]
fn default_crop_serializes_without_fields() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    assert!(!clip.has_custom_crop());
    let value = serde_json::to_value(&clip).expect("serialize");
    let map = value.as_object().expect("clip serializes to a map");
    assert!(!map.contains_key("crop"), "full crop must stay absent");
    assert!(!map.contains_key("flip_h"), "no flip must stay absent");
    assert!(!map.contains_key("flip_v"), "no flip must stay absent");

    // And a pre-crop save loads with the defaults.
    let loaded: Clip = serde_json::from_value(value).expect("deserialize");
    assert_eq!(loaded.crop, Param::Constant(CropRect::FULL));
    assert!(!loaded.flip_h && !loaded.flip_v);
}

#[test]
fn custom_crop_roundtrips_through_serde() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(0, 10, R24));
    clip.crop = CropRect {
        x: 0.1,
        y: 0.2,
        w: 0.5,
        h: 0.25,
    }
    .into();
    clip.flip_h = true;
    assert!(clip.has_custom_crop());
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.crop, clip.crop);
    assert!(loaded.flip_h && !loaded.flip_v);
}

#[test]
fn legacy_bare_crop_rect_json_loads_as_constant() {
    // Pre-Param saves wrote the bare rect; Constant serializes identically.
    let json = r#"{"x":0.1,"y":0.2,"w":0.5,"h":0.5}"#;
    let crop: Param<CropRect> = serde_json::from_str(json).expect("legacy crop");
    assert_eq!(
        crop,
        Param::Constant(CropRect {
            x: 0.1,
            y: 0.2,
            w: 0.5,
            h: 0.5,
        })
    );
}

#[test]
fn keyframed_crop_roundtrips_and_lerps() {
    let a = CropRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    let b = CropRect {
        x: 0.2,
        y: 0.2,
        w: 0.6,
        h: 0.6,
    };
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    clip.crop = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: a,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 10,
                value: b,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.crop, clip.crop);
    let mid = loaded.crop.sample(5);
    assert!((mid.x - 0.1).abs() < 1e-5);
    assert!((mid.y - 0.1).abs() < 1e-5);
    assert!((mid.w - 0.8).abs() < 1e-5);
    assert!((mid.h - 0.8).abs() < 1e-5);
}

#[test]
fn crop_rect_validation() {
    assert!(CropRect::FULL.validate().is_ok());
    assert!(
        CropRect {
            x: 0.25,
            y: 0.0,
            w: 0.5,
            h: 1.0,
        }
        .validate()
        .is_ok()
    );

    // Degenerate extents.
    for (w, h) in [(0.0, 1.0), (1.0, 0.0), (0.001, 1.0)] {
        assert!(
            CropRect {
                x: 0.0,
                y: 0.0,
                w,
                h
            }
            .validate()
            .is_err(),
            "w={w} h={h} must be rejected"
        );
    }
    // Out of frame.
    assert!(
        CropRect {
            x: -0.1,
            y: 0.0,
            w: 0.5,
            h: 0.5
        }
        .validate()
        .is_err()
    );
    assert!(
        CropRect {
            x: 0.6,
            y: 0.0,
            w: 0.5,
            h: 0.5
        }
        .validate()
        .is_err()
    );
    assert!(
        CropRect {
            x: 0.0,
            y: 0.9,
            w: 0.5,
            h: 0.2
        }
        .validate()
        .is_err()
    );
    // Non-finite.
    assert!(
        CropRect {
            x: f32::NAN,
            y: 0.0,
            w: 1.0,
            h: 1.0
        }
        .validate()
        .is_err()
    );
}

// --- transform ----------------------------------------------------------

#[test]
fn new_clips_have_identity_transform() {
    let clip = Clip::generated(Generator::Adjustment, tr(0, 10, R24));
    assert!(clip.transform.is_identity());
    assert_eq!(clip.transform, AnimatedTransform::default());
}

#[test]
fn clip_without_transform_field_deserializes_to_identity() {
    // A clip serialized before transforms existed: no `transform` key.
    let clip = Clip::generated(Generator::text("old"), tr(0, 10, R24));
    let mut value = serde_json::to_value(&clip).expect("serialize");
    value
        .as_object_mut()
        .expect("clip serializes to a map")
        .remove("transform")
        .expect("transform field present");

    let loaded: Clip = serde_json::from_value(value).expect("deserialize legacy clip");
    assert!(loaded.transform.is_identity());
    assert_eq!(loaded.content, clip.content);
}

#[test]
fn normal_blend_mode_is_elided_from_serde() {
    let clip = Clip::generated(Generator::text("blend"), tr(0, 10, R24));
    assert_eq!(clip.blend_mode, crate::look::BlendMode::Normal);
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(
        !json.contains("blend_mode"),
        "Normal blend must be absent from saves: {json}"
    );
}

#[test]
fn multiply_blend_mode_roundtrips_through_serde() {
    let mut clip = Clip::generated(Generator::text("blend"), tr(0, 10, R24));
    clip.blend_mode = crate::look::BlendMode::Multiply;
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(json.contains("\"blend_mode\":\"multiply\""));
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.blend_mode, crate::look::BlendMode::Multiply);
}

#[test]
fn clip_without_blend_mode_field_deserializes_to_normal() {
    let clip = Clip::generated(Generator::text("old"), tr(0, 10, R24));
    let mut value = serde_json::to_value(&clip).expect("serialize");
    // Normal is already skipped; ensure a legacy map without the key loads.
    value
        .as_object_mut()
        .expect("clip serializes to a map")
        .remove("blend_mode");

    let loaded: Clip = serde_json::from_value(value).expect("deserialize legacy clip");
    assert_eq!(loaded.blend_mode, crate::look::BlendMode::Normal);
    assert_eq!(loaded.content, clip.content);
}

#[test]
fn default_motion_blur_is_elided_from_serde() {
    let clip = Clip::generated(Generator::text("blur"), tr(0, 10, R24));
    assert!(clip.motion_blur.is_default());
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(
        !json.contains("motion_blur"),
        "default motion blur must be absent from saves: {json}"
    );
}

#[test]
fn enabled_motion_blur_roundtrips_through_serde() {
    let mut clip = Clip::generated(Generator::text("blur"), tr(0, 10, R24));
    clip.motion_blur = crate::look::MotionBlur {
        enabled: true,
        shutter_deg: 270.0,
        samples: 12,
    };
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(json.contains("\"motion_blur\""));
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert!(loaded.motion_blur.enabled);
    assert_eq!(loaded.motion_blur.shutter_deg, 270.0);
    assert_eq!(loaded.motion_blur.samples, 12);
}

#[test]
fn clip_without_motion_blur_field_deserializes_to_default() {
    let clip = Clip::generated(Generator::text("old"), tr(0, 10, R24));
    let mut value = serde_json::to_value(&clip).expect("serialize");
    value
        .as_object_mut()
        .expect("clip serializes to a map")
        .remove("motion_blur");

    let loaded: Clip = serde_json::from_value(value).expect("deserialize legacy clip");
    assert!(loaded.motion_blur.is_default());
    assert_eq!(loaded.content, clip.content);
}

#[test]
fn empty_layer_styles_are_elided_from_serde() {
    let clip = Clip::generated(Generator::text("plain"), tr(0, 10, R24));
    assert!(clip.styles.is_empty());
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(
        !json.contains("\"styles\""),
        "empty styles must be absent from saves: {json}"
    );
}

#[test]
fn clip_without_styles_field_deserializes_empty() {
    let clip = Clip::generated(Generator::text("old"), tr(0, 10, R24));
    let mut value = serde_json::to_value(&clip).expect("serialize");
    value
        .as_object_mut()
        .expect("clip serializes to a map")
        .remove("styles");

    let loaded: Clip = serde_json::from_value(value).expect("deserialize legacy clip");
    assert!(loaded.styles.is_empty());
    assert_eq!(loaded.content, clip.content);
}

#[test]
fn layer_shadow_roundtrips_through_serde() {
    let mut clip = Clip::generated(Generator::text("styles"), tr(0, 10, R24));
    clip.styles.shadow = Some(crate::look::LayerShadow::default());
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(json.contains("\"styles\""));
    assert!(json.contains("\"shadow\""));
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        loaded.styles.shadow.as_ref().unwrap().blur,
        Param::Constant(8.0)
    );
}

#[test]
fn keyframed_layer_shadow_blur_roundtrips_through_serde() {
    let mut clip = Clip::generated(Generator::text("styles"), tr(0, 10, R24));
    let shadow = crate::look::LayerShadow {
        blur: Param::Keyframed {
            keyframes: vec![
                Keyframe {
                    tick: 0,
                    value: 4.0,
                    easing: Easing::Linear,
                    tangents: None,
                },
                Keyframe {
                    tick: 10,
                    value: 16.0,
                    easing: Easing::EaseIn,
                    tangents: None,
                },
            ],
        },
        ..Default::default()
    };
    clip.styles.shadow = Some(shadow);
    let json = serde_json::to_string(&clip).expect("serialize");
    assert!(json.contains("\"kf\""));
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        loaded.styles.shadow.as_ref().unwrap().blur,
        clip.styles.shadow.as_ref().unwrap().blur
    );
}

#[test]
fn transform_roundtrips_through_serde() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 10, R24));
    clip.transform = ClipTransform {
        position: [-0.25, 0.5],
        scale: 1.5.into(),
        rotation: 90.0,
        opacity: 0.25,
        ..ClipTransform::IDENTITY
    }
    .into();
    let json = serde_json::to_string(&clip).expect("serialize");
    let loaded: Clip = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loaded.transform, clip.transform);
}

#[test]
fn legacy_plain_transform_json_deserializes_as_constants() {
    // The exact shape every pre-M2 save wrote: bare values per property.
    let json = r#"{
        "id": 1,
        "content": { "Generated": { "Text": { "content": "t" } } },
        "timeline": { "start": { "value": 0, "rate": { "num": 24, "den": 1 } },
                      "duration": { "value": 24, "rate": { "num": 24, "den": 1 } } },
        "transform": { "position": [0.25, -0.1], "scale": 2.0,
                       "rotation": 45.0, "opacity": 0.5 }
    }"#;
    let clip: Clip = serde_json::from_str(json).expect("deserialize pre-M2 transform");
    assert!(!clip.transform.is_animated());
    assert_eq!(
        clip.transform.sample(0),
        ClipTransform {
            position: [0.25, -0.1],
            scale: 2.0.into(),
            rotation: 45.0,
            opacity: 0.5,
            ..ClipTransform::IDENTITY
        }
    );
}

#[test]
fn constant_transform_serializes_in_pre_m2_shape() {
    let mut clip = Clip::generated(Generator::Adjustment, tr(0, 10, R24));
    clip.transform = ClipTransform {
        position: [0.25, 0.5],
        scale: 1.5.into(),
        rotation: 0.0,
        opacity: 1.0,
        ..ClipTransform::IDENTITY
    }
    .into();
    let value = serde_json::to_value(&clip).expect("serialize");
    // Bare values, not {"kf": ...} wrappers — byte-compatible with old readers.
    assert_eq!(value["transform"]["scale"], 1.5);
    assert_eq!(value["transform"]["position"][0], 0.25);
}
