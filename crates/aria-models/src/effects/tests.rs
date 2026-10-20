use super::*;

/// Every shipped effect names the group it is listed under, and an entry
/// that names none is listed under Other. A shipped entry added without a
/// category fails here. A generator is listed under Generators, which has
/// no folders, so it names none.
#[test]
fn every_shipped_effect_names_its_group_and_a_nameless_one_is_other() {
    for spec in shipped_effect_catalog()
        .iter()
        .filter(|spec| !spec.generator)
    {
        assert!(!spec.category.is_empty(), "{} names no category", spec.id);
        assert_eq!(spec.category(), spec.category);
    }
    let spec: EffectSpec = toml::from_str("id = \"x\"\nlabel = \"X\"").expect("parses");
    assert_eq!(spec.category(), OTHER_VIDEO_CATEGORY);
}

#[test]
fn catalog_ids_are_unique() {
    let catalog = effect_catalog();
    let mut ids: Vec<&str> = catalog.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), catalog.len());
}

#[test]
fn unknown_effect_fails_validation() {
    assert!(EffectInstance::new("nope").validate().is_err());
    assert!(EffectInstance::new("gaussian_blur").validate().is_ok());
}

#[test]
fn sampled_param_falls_back_to_default() {
    let fx = EffectInstance::new("gaussian_blur");
    assert_eq!(fx.sample_param("radius", 0.0), Some(4.0));
    assert_eq!(fx.sample_param("missing", 0.0), None);
}

#[test]
fn out_of_range_constant_rejected() {
    let mut fx = EffectInstance::new("vignette");
    assert!(fx.set_param_constant(0, 2.0).is_err()); // amount max 1.0
    assert!(fx.set_param_constant(0, 0.5).is_ok());
    assert_eq!(fx.sample_param("amount", 0.0), Some(0.5));
}

#[test]
fn keyframe_roundtrip_on_a_param() {
    let mut fx = EffectInstance::new("gaussian_blur");
    fx.set_param_keyframe(0, 0, 0.0, Easing::Linear).unwrap();
    fx.set_param_keyframe(0, 24, 8.0, Easing::Linear).unwrap();
    assert_eq!(fx.sample_param("radius", 12.0), Some(4.0));
    fx.validate().unwrap();
    fx.remove_param_keyframe(0, 24).unwrap();
    // Removing the last-but-one keyframe leaves a constant.
    assert!(fx.remove_param_keyframe(0, 999).is_err());
}

#[test]
fn legacy_scalar_json_still_loads() {
    let fx: EffectInstance =
        serde_json::from_str(r#"{"effect_id":"gaussian_blur","params":[["radius",4.0]]}"#).unwrap();
    assert_eq!(fx.effect_id, "gaussian_blur");
    assert_eq!(fx.sample_param("radius", 0.0), Some(4.0));
    assert!(fx.color_params.is_empty());
    assert!(fx.vec2_params.is_empty());
    fx.validate().unwrap();
}

#[test]
fn color_params_roundtrip() {
    let mut fx = EffectInstance::new("color_overlay");
    fx.set_color_param_constant(0, [10, 20, 30, 40]).unwrap();
    fx.set_vec2_param_constant(1, [0.25, -0.5]).unwrap();
    let json = serde_json::to_string(&fx).unwrap();
    let loaded: EffectInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(
        loaded.sample_color_param("color", 0.0),
        Some([10, 20, 30, 40])
    );
    assert_eq!(loaded.sample_vec2_param("offset", 0.0), Some([0.25, -0.5]));
    loaded.validate().unwrap();
}

#[test]
fn wrong_kind_set_rejected() {
    let mut fx = EffectInstance::new("color_overlay");
    // color slot is Color — scalar setter rejected.
    assert!(fx.set_param_constant(0, 0.5).is_err());
    // offset slot is Vec2 — color setter rejected.
    assert!(fx.set_color_param_constant(1, [1, 2, 3, 4]).is_err());
    // amount slot is Scalar — vec2 setter rejected.
    assert!(fx.set_vec2_param_constant(2, [0.0, 0.0]).is_err());
    // Scalar name stored in color_params fails validate.
    fx.color_params
        .insert("amount".into(), Param::Constant([1, 2, 3, 4]));
    assert!(fx.validate().is_err());
}

#[test]
fn vec2_component_out_of_range_rejected() {
    let mut fx = EffectInstance::new("color_overlay");
    assert!(fx.set_vec2_param_constant(1, [0.0, 1.5]).is_err());
    assert!(fx.set_vec2_param_constant(1, [-1.0, 1.0]).is_ok());
}

fn all_15_param_specs() -> Vec<EffectParamSpec> {
    vec![
        EffectParamSpec {
            name: "scalar_p".into(),
            label: "Scalar".into(),
            kind: EffectParamKind::Scalar,
            default: 0.25,
            min: 0.0,
            max: 1.0,
            soft_min: Some(0.1),
            soft_max: Some(0.9),
            unit: Some("px".into()),
            display: Some(EffectParamDisplay::Percentage),
            tooltip: Some("scalar tip".into()),
            ..Default::default()
        },
        EffectParamSpec {
            name: "int_p".into(),
            label: "Int".into(),
            kind: EffectParamKind::Int,
            default_int: 3,
            int_min: 1,
            int_max: 10,
            ..Default::default()
        },
        EffectParamSpec {
            name: "bool_p".into(),
            label: "Bool".into(),
            kind: EffectParamKind::Bool,
            default_bool: false,
            ..Default::default()
        },
        EffectParamSpec {
            name: "choice_p".into(),
            label: "Choice".into(),
            kind: EffectParamKind::Choice,
            choices: vec![
                EffectChoiceOption::new("fast", "Fast"),
                EffectChoiceOption::new("quality", "Quality"),
            ],
            default_choice: Some("fast".into()),
            ..Default::default()
        },
        EffectParamSpec {
            name: "color_p".into(),
            label: "Color".into(),
            kind: EffectParamKind::Color,
            default_color: [255, 0, 0, 255],
            ..Default::default()
        },
        EffectParamSpec {
            name: "vec2_p".into(),
            label: "Vec2".into(),
            kind: EffectParamKind::Vec2,
            default_vec2: [0.1, 0.2],
            min: -1.0,
            max: 1.0,
            ..Default::default()
        },
        EffectParamSpec {
            name: "point3_p".into(),
            label: "Point3".into(),
            kind: EffectParamKind::Point3,
            default_vec3: [0.1, 0.2, 0.3],
            min: -1.0,
            max: 1.0,
            ..Default::default()
        },
        EffectParamSpec {
            name: "size_p".into(),
            label: "Size".into(),
            kind: EffectParamKind::Size,
            default_vec2: [640.0, 360.0],
            min: 1.0,
            max: 4096.0,
            ..Default::default()
        },
        EffectParamSpec {
            name: "angle_p".into(),
            label: "Angle".into(),
            kind: EffectParamKind::Angle,
            default: 45.0,
            min: -360.0,
            max: 360.0,
            display: Some(EffectParamDisplay::AngleDial),
            visible_when: Some(EffectVisibilityCondition::new("bool_p", "true")),
            ..Default::default()
        },
        EffectParamSpec {
            name: "curve_p".into(),
            label: "Curve".into(),
            kind: EffectParamKind::Curve,
            default_curve: vec![
                EffectCurvePoint::new(0.0, 0.0),
                EffectCurvePoint::new(1.0, 1.0),
            ],
            ..Default::default()
        },
        EffectParamSpec {
            name: "grad_p".into(),
            label: "Gradient".into(),
            kind: EffectParamKind::Gradient,
            default_gradient: vec![
                EffectGradientStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
                EffectGradientStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
            ],
            ..Default::default()
        },
        EffectParamSpec {
            name: "text_p".into(),
            label: "Text".into(),
            kind: EffectParamKind::Text,
            default_text: "hello".into(),
            ..Default::default()
        },
        EffectParamSpec {
            name: "font_p".into(),
            label: "Font".into(),
            kind: EffectParamKind::Font,
            default_text: "Inter".into(),
            ..Default::default()
        },
        EffectParamSpec {
            name: "file_p".into(),
            label: "File".into(),
            kind: EffectParamKind::File,
            default_text: "lut.cube".into(),
            ..Default::default()
        },
        EffectParamSpec {
            name: "layer_p".into(),
            label: "Layer".into(),
            kind: EffectParamKind::Layer,
            ..Default::default()
        },
    ]
}

#[test]
fn all_15_param_kinds_roundtrip_on_effect_and_transition_and_respect_visibility() {
    let (_effect_reg, _) = crate::register_effect(EffectSpec {
        id: "test.models.all_15_v2".into(),
        label: "All 15".into(),
        category: "Stylize".into(),
        params: all_15_param_specs(),
        groups: vec![EffectGroupSpec {
            id: "adv".into(),
            label: "Advanced".into(),
            collapsed: true,
        }],
        generator: false,
        stateful: false,
        ..Default::default()
    })
    .expect("register test effect");
    let (_tr_reg, _) = crate::register_transition(crate::TransitionSpec {
        id: "test.models.tr_15_v2".into(),
        label: "Tr 15".into(),
        params: all_15_param_specs(),
    })
    .expect("register test transition");

    let mut fx = EffectInstance::new("test.models.all_15_v2");
    let spec = fx.spec().expect("registered spec");
    // Initially `bool_p` is false, so `angle_p` (slot 8) is hidden.
    assert!(!fx.is_param_visible(&spec.params[8], &spec, 0.0));

    fx.set_param_constant(0, 0.75).unwrap();
    fx.set_int_param_constant(1, 7).unwrap();
    fx.set_bool_param(2, true).unwrap();
    fx.set_choice_param(3, "quality").unwrap();
    fx.set_linear_color_param_constant(4, [0.2, 0.4, 0.6, 1.0])
        .unwrap();
    fx.set_vec2_param_constant(5, [-0.5, 0.5]).unwrap();
    fx.set_vec3_param_keyframe(6, 0, [0.0, 0.0, 0.0], Easing::Linear)
        .unwrap();
    fx.set_vec3_param_keyframe(6, 20, [0.4, 0.6, 0.8], Easing::Linear)
        .unwrap();
    fx.set_vec2_param_constant(7, [1920.0, 1080.0]).unwrap();
    fx.set_param_constant(8, 180.0).unwrap();
    fx.set_curve_param(
        9,
        vec![
            EffectCurvePoint::new(0.0, 1.0),
            EffectCurvePoint::new(1.0, 0.0),
        ],
    )
    .unwrap();
    fx.set_gradient_param(
        10,
        vec![
            EffectGradientStop::new(0.0, [1.0, 0.0, 0.0, 1.0]),
            EffectGradientStop::new(1.0, [0.0, 0.0, 1.0, 1.0]),
        ],
    )
    .unwrap();
    fx.set_text_param(11, "updated").unwrap();
    fx.set_text_param(12, "Roboto").unwrap();
    fx.set_text_param(13, "grade.cube").unwrap();
    fx.set_layer_param(14, Some(crate::ClipId::from_raw(42)))
        .unwrap();

    // With `bool_p` = true, `angle_p` is now visible!
    assert!(fx.is_param_visible(&spec.params[8], &spec, 0.0));
    fx.validate().unwrap();

    let json = serde_json::to_string(&fx).unwrap();
    let loaded: EffectInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, fx);
    assert_eq!(
        loaded.sample_vec3_param("point3_p", 10.0),
        Some([0.2, 0.3, 0.4])
    );
    let floats = loaded.sample_flattened_floats(10.0, None).unwrap();
    let expected_floats: usize = spec.params.iter().map(|p| p.kind.shader_floats()).sum();
    assert_eq!(floats.len(), expected_floats);

    // Verify `clip_params_for` hides `angle_p` when `bool_p` is false and shows it when true.
    let mut clip = crate::Clip::generated(
        crate::Generator::SolidColor {
            rgba: [0, 0, 0, 255],
        },
        crate::TimeRange::at_rate(0, 48, crate::Rational::FPS_24),
    );
    clip.effects.push(loaded);
    let rows_when_true = crate::clip_params_for(&clip);
    assert!(
        rows_when_true.iter().any(|r| r.param
            == crate::ClipParam::Effect {
                effect: 0,
                param: 8
            }),
        "angle_p must be present in clip_params_for when bool_p is true"
    );
    clip.effects[0].set_bool_param(2, false).unwrap();
    let rows_when_false = crate::clip_params_for(&clip);
    assert!(
        !rows_when_false.iter().any(|r| r.param
            == crate::ClipParam::Effect {
                effect: 0,
                param: 8
            }),
        "angle_p must be omitted from clip_params_for when bool_p is false"
    );

    // Transition round-trip across all 15 kinds.
    let mut tr = crate::Transition::new(
        crate::ClipId::from_raw(1),
        crate::ClipId::from_raw(2),
        "test.models.tr_15_v2",
        12,
    );
    tr.set_param_constant(0, 0.5).unwrap();
    tr.set_int_param(1, 5).unwrap();
    tr.set_bool_param(2, true).unwrap();
    tr.set_choice_param(3, "quality").unwrap();
    tr.set_linear_color_param(4, [0.1, 0.2, 0.3, 1.0]).unwrap();
    tr.set_vec2_param(5, [0.25, -0.25]).unwrap();
    tr.set_vec3_param(6, [0.1, 0.2, 0.3]).unwrap();
    tr.set_vec2_param(7, [1280.0, 720.0]).unwrap();
    tr.set_param_constant(8, 90.0).unwrap();
    tr.set_curve_param(
        9,
        vec![
            EffectCurvePoint::new(0.0, 0.5),
            EffectCurvePoint::new(1.0, 0.5),
        ],
    )
    .unwrap();
    tr.set_gradient_param(
        10,
        vec![
            EffectGradientStop::new(0.0, [0.0, 1.0, 0.0, 1.0]),
            EffectGradientStop::new(1.0, [1.0, 0.0, 1.0, 1.0]),
        ],
    )
    .unwrap();
    tr.set_text_param(11, "tr_text").unwrap();
    tr.set_text_param(12, "Mono").unwrap();
    tr.set_text_param(13, "tr.cube").unwrap();
    tr.set_layer_param(14, Some(crate::ClipId::from_raw(9)))
        .unwrap();
    tr.validate().unwrap();
    let tr_json = serde_json::to_string(&tr).unwrap();
    let tr_loaded: crate::Transition = serde_json::from_str(&tr_json).unwrap();
    assert_eq!(tr_loaded, tr);
    let tr_spec = crate::transition_spec("test.models.tr_15_v2").unwrap();
    assert_eq!(
        tr_loaded.sample_flattened_floats().unwrap().len(),
        tr_spec
            .params
            .iter()
            .map(|p| p.kind.shader_floats())
            .sum::<usize>()
    );
}
