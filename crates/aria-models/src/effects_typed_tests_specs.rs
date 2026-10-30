use super::*;

#[test]
fn kinds_lists_every_kind_and_the_fixture_holds_each() {
    let params = every_kind();
    for (at, kind) in K::ALL.into_iter().enumerate() {
        assert_eq!(position(kind), at, "{kind:?}");
        assert!(
            params.iter().any(|p| p.kind == kind),
            "every_kind has no {kind:?}"
        );
    }
}

#[test]
fn kind_names_round_trip_and_aliases_resolve() {
    for kind in K::ALL {
        let name = kind.as_str();
        assert_eq!(EffectParamKind::from_name(name), Some(kind));
        let json = format!("\"{name}\"");
        assert_eq!(serde_json::to_string(&kind).expect("serializes"), json);
        assert_eq!(
            serde_json::from_str::<EffectParamKind>(&json).ok(),
            Some(kind)
        );
    }
    for (alias, kind) in [
        ("float", K::Scalar),
        ("integer", K::Int),
        ("boolean", K::Bool),
        ("enum", K::Choice),
        ("colour", K::Color),
        ("point2d", K::Vec2),
        ("point_2d", K::Vec2),
        ("vec3", K::Point3),
        ("point3d", K::Point3),
        ("point_3d", K::Point3),
        ("string", K::Text),
        ("path", K::File),
        ("layer_ref", K::Layer),
    ] {
        assert_eq!(EffectParamKind::from_name(alias), Some(kind), "{alias}");
    }
    for alias in [
        "float",
        "point2d",
        "point_2d",
        "vec3",
        "point3d",
        "point_3d",
        "layer_ref",
    ] {
        let parsed: EffectParamKind =
            serde_json::from_str(&format!("\"{alias}\"")).expect("a manifest alias parses");
        assert_eq!(Some(parsed), EffectParamKind::from_name(alias), "{alias}");
    }
    assert_eq!(EffectParamKind::from_name("Scalar"), None);
    assert_eq!(EffectParamKind::from_name(""), None);
}

#[test]
fn check_params_refuses_unnamed_repeated_inverted_and_empty_parameters() {
    let cases = [
        (
            EffectParamSpec::scalar("", "Unnamed", 0.0, 0.0, 1.0),
            "fx parameter 0 has no name",
        ),
        (
            EffectParamSpec::scalar("p", "P", 0.0, 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::vec2("p", "P", [0.0; 2], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::point3("p", "P", [0.0; 3], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::size("p", "P", [0.0; 2], 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::angle("p", "P", 0.0, 1.0, 0.0),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::int("p", "P", 0, 5, 1),
            "fx.p has an inverted range",
        ),
        (
            EffectParamSpec::scalar("p", "P", 0.5, 0.0, 1.0).with_soft_range(0.9, 0.1),
            "fx.p has an inverted soft range",
        ),
        (
            EffectParamSpec::choice("p", "P", Vec::new(), "x"),
            "fx.p choice parameter has no choices",
        ),
    ];
    for (param, message) in cases {
        assert_eq!(check_params("fx", &[param]), Err(message.to_owned()));
    }
    let twice = [
        EffectParamSpec::scalar("a", "A", 0.0, 0.0, 1.0),
        EffectParamSpec::boolean("a", "A", false),
    ];
    assert_eq!(
        check_params("fx", &twice),
        Err("fx repeats parameter a".to_owned())
    );
    for kind in K::ALL {
        let inverted = EffectParamSpec {
            name: "p".into(),
            kind,
            min: 1.0,
            max: 0.0,
            choices: vec![EffectChoiceOption::new("a", "A")],
            ..EffectParamSpec::default()
        };
        let want = if kind.has_float_range() {
            Err("fx.p has an inverted range".to_owned())
        } else {
            Ok(())
        };
        assert_eq!(check_params("fx", &[inverted]), want, "{kind:?}");
    }
    assert_eq!(check_params("fx", &every_kind()), Ok(()));
}

#[test]
fn visibility_conditions_load_from_every_spelling() {
    for (json, equals) in [
        (r#"{"param":"p","equals":"x"}"#, "x"),
        (r#"{"param":"p","equals_choice":"fast"}"#, "fast"),
        (r#"{"param":"p","equals_bool":true}"#, "true"),
        (r#"{"param":"p","equals_int":-3}"#, "-3"),
        (r#"{"param":"p"}"#, ""),
        (
            r#"{"param":"p","equals":"x","equals_choice":"y","equals_bool":false,"equals_int":1}"#,
            "x",
        ),
        (
            r#"{"param":"p","equals_choice":"y","equals_bool":false}"#,
            "y",
        ),
        (
            r#"{"param":"p","equals_bool":false,"equals_int":1}"#,
            "false",
        ),
    ] {
        let condition: EffectVisibilityCondition = serde_json::from_str(json).expect(json);
        assert_eq!(
            condition,
            EffectVisibilityCondition::new("p", equals),
            "{json}"
        );
    }
    let saved = serde_json::to_string(&EffectVisibilityCondition::new("p", "x")).expect("saves");
    assert_eq!(saved, r#"{"param":"p","equals":"x"}"#);
}

#[test]
fn color_defaults_convert_between_bytes_and_linear() {
    let linear_only = EffectParamSpec {
        name: "c".into(),
        kind: K::Color,
        default_linear_color: Some([1.0, 0.5, 0.0, 1.0]),
        ..EffectParamSpec::default()
    };
    assert_eq!(linear_only.default_color_u8(), [255, 128, 0, 255]);
    let bytes_only = EffectParamSpec {
        name: "c".into(),
        kind: K::Color,
        default_color: [255, 0, 51, 255],
        ..EffectParamSpec::default()
    };
    assert_eq!(bytes_only.default_color_linear(), [1.0, 0.0, 0.2, 1.0]);
    let bright = EffectParamSpec::linear_color("c", "C", [2.0, -1.0, 0.5, 1.0]);
    assert_eq!(bright.default_color_u8(), [255, 0, 128, 255]);
    assert_eq!(bright.default_color_linear(), [2.0, -1.0, 0.5, 1.0]);
}

#[test]
fn a_choice_falls_back_to_its_default_then_its_first_option() {
    let options = vec![
        EffectChoiceOption::new("a", "A"),
        EffectChoiceOption::new("b", "B"),
        EffectChoiceOption::new("c", "C"),
    ];
    let no_default = EffectParamSpec {
        name: "m".into(),
        kind: K::Choice,
        choices: options.clone(),
        ..EffectParamSpec::default()
    };
    assert_eq!(no_default.default_choice_id(), Some("a"));
    assert_eq!(no_default.choice_or_default(None), Some("a"));
    assert_eq!(no_default.choice_or_default(Some("b")), Some("b"));
    assert_eq!(no_default.choice_or_default(Some("gone")), Some("a"));
    let empty = EffectParamSpec {
        kind: K::Choice,
        ..EffectParamSpec::default()
    };
    assert_eq!(empty.choice_or_default(Some("x")), None);
    let built = EffectParamSpec::choice("m", "M", options.clone(), "c");
    assert_eq!((built.default, built.min, built.max), (2.0, 0.0, 2.0));
    assert_eq!(built.choice_or_default(Some("gone")), Some("c"));
    // A declared default no option has, such as the empty id a native plugin
    // sends for "the first option", reads as the first option.
    for unlisted in ["", "gone"] {
        let declared = EffectParamSpec::choice("m", "M", options.clone(), unlisted);
        assert_eq!(declared.default_choice_id(), Some("a"), "{unlisted:?}");
        assert_eq!(declared.choice_or_default(None), Some("a"), "{unlisted:?}");
    }
}

#[test]
fn numeric_constructors_mirror_ranges_and_set_units() {
    let int = EffectParamSpec::int("i", "I", 3, -5, 10);
    assert_eq!((int.default, int.min, int.max), (3.0, -5.0, 10.0));
    assert_eq!(EffectParamSpec::boolean("b", "B", true).default, 1.0);
    assert_eq!(EffectParamSpec::boolean("b", "B", false).default, 0.0);
    let size = EffectParamSpec::size("s", "S", [1.0; 2], 0.0, 2.0);
    assert_eq!(size.unit.as_deref(), Some("px"));
    let angle = EffectParamSpec::angle("a", "A", 0.0, -1.0, 1.0);
    assert_eq!(angle.unit.as_deref(), Some("deg"));
    assert_eq!(angle.display, Some(EffectParamDisplay::AngleDial));
}

#[test]
fn curves_and_gradients_hold_their_ends_and_interpolate_between() {
    assert_eq!(
        sample_effect_curve(&[], 0.25),
        0.25,
        "no points read as identity"
    );
    assert_eq!(sample_effect_curve(&[], 1.5), 1.0, "t is clamped");
    assert_eq!(sample_effect_curve(&[pt(0.5, 0.2)], 0.9), 0.2);
    let bend = [pt(0.25, 0.0), pt(0.75, 1.0), pt(1.0, 0.5)];
    assert_eq!(
        sample_effect_curve(&bend, 0.125),
        0.0,
        "before the first point"
    );
    assert_eq!(sample_effect_curve(&bend, 0.5), 0.5);
    assert_eq!(sample_effect_curve(&bend, 0.875), 0.75);
    assert_eq!(
        sample_effect_curve(&bend[..2], 0.875),
        1.0,
        "after the last point"
    );
    let step = [pt(0.0, 0.0), pt(0.5, 0.2), pt(0.5 + f32::EPSILON, 0.8)];
    assert_eq!(
        sample_effect_curve(&step, 0.5 + f32::EPSILON / 2.0),
        0.8,
        "a step narrower than 1e-6 jumps to its far value"
    );
    let lut = evaluate_effect_curve_lut(&[pt(0.0, 1.0), pt(1.0, 0.0)]);
    assert_eq!((lut[0], lut[EFFECT_CURVE_LUT_SAMPLES - 1]), (1.0, 0.0));

    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    assert_eq!(sample_effect_gradient(&[], 0.25), [0.25, 0.25, 0.25, 1.0]);
    assert_eq!(sample_effect_gradient(&[stop(0.5, red)], 0.0), red);
    let ramp = [stop(0.25, red), stop(0.75, blue)];
    assert_eq!(
        sample_effect_gradient(&ramp, 0.0),
        red,
        "before the first stop"
    );
    assert_eq!(sample_effect_gradient(&ramp, 0.5), [0.5, 0.0, 0.5, 1.0]);
    assert_eq!(
        sample_effect_gradient(&ramp, 1.0),
        blue,
        "after the last stop"
    );
    let hard = [
        stop(0.0, red),
        stop(0.5, red),
        stop(0.5 + f32::EPSILON, blue),
    ];
    assert_eq!(
        sample_effect_gradient(&hard, 0.5 + f32::EPSILON / 2.0),
        blue
    );
    let lut = evaluate_effect_gradient_lut(&[stop(0.0, red), stop(1.0, blue)]);
    assert_eq!((lut[0], lut[EFFECT_GRADIENT_LUT_SAMPLES - 1]), (red, blue));
}

#[test]
fn bounds_combine_and_grow_the_quad_toward_each_side() {
    assert_eq!(
        EffectBounds::new(1.0, 2.0, 3.0, 4.0).combine(EffectBounds::uniform(1.0)),
        EffectBounds::new(2.0, 3.0, 4.0, 5.0)
    );
    assert!(EffectBounds::ZERO.is_zero());
    assert!(!EffectBounds::new(0.0, 0.0, 0.0, 0.5).is_zero());
    let (center, size) = ([100.0, 100.0], [50.0, 20.0]);
    assert_eq!(
        EffectBounds::ZERO.apply_to_quad(center, size),
        (center, size)
    );
    assert_eq!(
        EffectBounds::new(10.0, 0.0, 30.0, 4.0).apply_to_quad(center, size),
        ([110.0, 102.0], [90.0, 24.0])
    );
    assert_eq!(
        EffectBounds::uniform(-100.0).apply_to_quad(center, size),
        (center, [0.0, 0.0]),
        "a shrink past zero leaves an empty quad"
    );
}

#[test]
fn a_spec_built_in_code_matches_its_catalog_entry() {
    let parsed: EffectSpec = toml::from_str(
        r#"
id = "test.bounds"
label = "Bounds"
reads-outside = true
time-dependent = true

[bounds]
left = 4.0
top = 4.0
right = 4.0
bottom = 4.0

[[params]]
name = "radius"
label = "Radius"
default = 4.0
max = 64.0
unit = "px"

[[groups]]
id = "adv"
label = "Advanced"
collapsed = true
"#,
    )
    .expect("parses");
    let built = EffectSpec {
        id: "test.bounds".into(),
        label: "Bounds".into(),
        params: vec![EffectParamSpec::scalar("radius", "Radius", 4.0, 0.0, 64.0).with_unit("px")],
        groups: vec![EffectGroupSpec::new("adv", "Advanced").with_collapsed(true)],
        ..EffectSpec::default()
    }
    .with_bounds(EffectBounds::uniform(4.0))
    .with_reads_outside(true)
    .with_time_dependent(true);
    assert_eq!(parsed, built);
    assert_eq!(built.param("radius"), built.param_at(0));
    assert!(built.param("radius").is_some());
    assert_eq!((built.param("ghost"), built.param_at(1)), (None, None));
    assert_eq!(built.group("adv").map(|g| g.collapsed), Some(true));
    assert_eq!(built.group("ghost"), None);
}

#[test]
fn a_point_override_replaces_one_vector_slot_of_the_buffer() {
    let id = "test.typed.fx.point";
    let _reg = EffectInstance::register(id, every_kind());
    let params = every_kind();
    let fx = EffectInstance::new(id);
    let base = fx.sample_flattened_floats(0.0, None).expect("registered");
    let moved = fx
        .sample_flattened_floats(0.0, Some((OFFSET, [0.9, -0.9])))
        .expect("registered");
    let mut want = base.clone();
    let at = offset(&params, OFFSET);
    want[at..at + 2].copy_from_slice(&[0.9, -0.9]);
    assert_eq!(moved, want);
    assert_eq!(
        fx.sample_flattened_floats(0.0, Some((SCALAR, [0.9, -0.9])))
            .ok(),
        Some(base),
        "a slot that is not a vector ignores the override"
    );
}
