use super::*;

#[test]
fn statements_and_operators_follow_javascript() {
    let cases = [
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("10 % 4 - -2", 4.0),
        ("let x = 2; x += 3; x * 2", 10.0),
        ("let a = [1, 2]; a[1] = 5; a[0] + a[1]", 6.0),
        ("let b = [1, 2]; let c = b; c[0] = 9; b[0]", 1.0),
        (
            "let s = 0; for (let i = 0; i < 4; i++) { if (i == 2) continue; s += i } s",
            4.0,
        ),
        ("let n = 0; while (true) { n++; if (n >= 3) break } n", 3.0),
        ("let n = 5; let m = n--; m * 10 + n", 54.0),
        ("let n = 5; let m = --n; m * 10 + n", 44.0),
        ("true ? 4 : 5", 4.0),
        ("0 || 6", 6.0),
        ("2 && 8", 8.0),
        ("return 3; 4", 3.0),
        ("if (1 > 2) 7; else 8", 8.0),
        ("1 == 1 && 2 === 2 && 1 != 2 && \"a\" < \"b\" ? 1 : 0", 1.0),
        ("[1, 2] == [1, 2] ? 1 : 0", 1.0),
        (
            "Math.max(1, 5, 3) + Math.min(2) + Math.round(2.5) + Math.round(-2.5)",
            8.0,
        ),
        ("\"ab\".length + [1, 2, 3].length", 5.0),
        ("(\"x\" + 1 + 2).length", 3.0),
        ("+\"3\" + +true", 4.0),
        ("Math.PI", std::f64::consts::PI),
        ("Math.atan2(1, 1) * 4", std::f64::consts::PI),
        ("Math.pow(2, 10) + Math.sqrt(16) + Math.hypot(3, 4)", 1033.0),
        ("let v = 1\nv + 1", 2.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
}

#[test]
fn values_convert_to_pixels_and_percent_at_the_model_boundary() {
    // Position: the anchor's offset from the canvas centre as a fraction,
    // read as pixels from the top-left corner.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.25, -0.5]));
    assert_eq!(
        run(
            "value[0] == 1440 && value[1] == 0 ? [960, 540] : [1, 1]",
            &mut position
        ),
        Ok(ParamValue::Vec2([0.0, 0.0]))
    );
    assert_eq!(
        run("value", &mut position),
        Ok(ParamValue::Vec2([0.25, -0.5]))
    );
    assert_eq!(
        run("[value[0] / 2, value[1]]", &mut position),
        Ok(ParamValue::Vec2([-0.125, -0.5]))
    );

    // Opacity and scale: fractions, read as percent.
    let mut opacity = scope(ClipParam::Opacity, ParamValue::Scalar(0.5));
    assert_eq!(
        run("value == 50 ? 25 : 0", &mut opacity),
        Ok(ParamValue::Scalar(0.25))
    );
    let mut scale = scope(ClipParam::Scale, ParamValue::Vec2([1.0, 2.0]));
    assert_eq!(
        run("value[1] == 200 ? [50, 50] : [0, 0]", &mut scale),
        Ok(ParamValue::Vec2([0.5, 0.5]))
    );

    // Everything else is read in the model's own units.
    let mut rotation = scope(ClipParam::Rotation, ParamValue::Scalar(45.0));
    assert_eq!(
        run("value + 1", &mut rotation),
        Ok(ParamValue::Scalar(46.0))
    );
    let mut anchor = scope(ClipParam::AnchorPoint, ParamValue::Vec2([0.5, 0.5]));
    assert_eq!(
        run("[value[0] + 0.25, value[1]]", &mut anchor),
        Ok(ParamValue::Vec2([0.75, 0.5]))
    );

    // Colours are four channels from 0 to 1; out-of-range channels clamp and
    // a missing alpha is opaque.
    let mut colour = scope(ClipParam::Rotation, ParamValue::Color([255, 0, 51, 255]));
    assert_eq!(
        run("value[2] == 0.2 ? value : [0, 0, 0, 0]", &mut colour),
        Ok(ParamValue::Color([255, 0, 51, 255]))
    );
    assert_eq!(
        run("[0, 1, 0]", &mut colour),
        Ok(ParamValue::Color([0, 255, 0, 255]))
    );
    assert_eq!(
        run("[2, -1, 0.5, 0.5]", &mut colour),
        Ok(ParamValue::Color([255, 0, 128, 128]))
    );
    let mut crop = scope(ClipParam::Crop, ParamValue::Rect([0.0, 0.0, 1.0, 1.0]));
    assert_eq!(
        run("[0.25, 0, value[2] / 2, 1]", &mut crop),
        Ok(ParamValue::Rect([0.25, 0.0, 0.5, 1.0]))
    );
}

#[test]
fn a_result_of_the_wrong_kind_or_out_of_range_is_an_error() {
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    let mut opacity = scope(ClipParam::Opacity, ParamValue::Scalar(1.0));
    let cases: [(&str, &mut Scope, &str); 9] = [
        (
            "5",
            &mut position,
            "the property takes an array of 2 numbers",
        ),
        ("[1, 2]", &mut opacity, "the property takes a number"),
        ("0 / 0", &mut rotation(), "NaN"),
        ("1 / 0", &mut rotation(), "Infinity"),
        ("1e300", &mut rotation(), "out of range"),
        ("let x = 1", &mut rotation(), "ends without a value"),
        ("seedRandom(1)", &mut rotation(), "returned null"),
        ("break", &mut rotation(), "break is outside a loop"),
        ("\"text\"", &mut rotation(), "returned a string"),
    ];
    for (source, scope, wanted) in cases {
        let message = error(source, scope);
        assert!(message.contains(wanted), "{source}: {message}");
    }
    let empty = compile("  // nothing\n").err().unwrap_or_default();
    assert!(empty.contains("empty"), "{empty}");
}

#[test]
fn references_declare_inputs_once_in_first_use_order() {
    let named = || InputClip::Named("Logo".into());
    let effect = |clip: InputClip, effect: &str, param: &str| DriverInput {
        clip,
        property: InputProperty::Effect {
            effect: effect.into(),
            param: param.into(),
        },
    };
    let source = "thisComp.layer(\"Logo\").transform.position[0] \
        + effect(\"Blur\")(\"Radius\") + thisLayer.opacity + scale[1] \
        + thisLayer.property(\"text-size\") + audio.volume \
        + thisComp.layer(\"Logo\").transform.position[1] \
        + thisComp.layer(\"Logo\").effect(\"Glow\").param(\"Intensity\") \
        + thisLayer.audio.pan + thisComp.layer(\"Logo\").prop(\"opacity\")";
    assert_eq!(
        inputs_of(source),
        vec![
            input(named(), "position"),
            effect(InputClip::This, "Blur", "Radius"),
            input(InputClip::This, "opacity"),
            input(InputClip::This, "scale"),
            input(InputClip::This, "text-size"),
            input(InputClip::This, "volume"),
            effect(named(), "Glow", "Intensity"),
            input(InputClip::This, "pan"),
            input(named(), "opacity"),
        ]
    );
    // Every transform name, bare, in the group, and on the layer; zRotation is
    // rotation.
    assert_eq!(
        inputs_of(
            "rotation + transform.rotation + thisLayer.zRotation \
             + thisLayer.transform.anchorPoint[0] + anchorPoint[1] + opacity"
        ),
        vec![
            input(InputClip::This, "rotation"),
            input(InputClip::This, "anchor-point"),
            input(InputClip::This, "opacity"),
        ]
    );
    // The driven property and time read no input.
    assert_eq!(
        inputs_of("value + thisProperty.valueAtTime(time - 1)"),
        Vec::new()
    );
}

#[test]
fn a_declared_name_shadows_the_property_of_that_name() {
    assert_eq!(inputs_of("let scale = 2; scale * 3"), Vec::new());
    assert_eq!(inputs_of("position = 4; position"), Vec::new());
    assert_eq!(number("let scale = 2; scale * 3", &mut rotation()), 6.0);
}

#[test]
fn a_reference_that_cannot_be_declared_is_refused_at_compile_time() {
    let cases = [
        (
            "let n = \"Logo\"; thisComp.layer(n).transform.position[0]",
            "line 1, column 31: thisComp.layer takes the layer's name in quotes",
        ),
        (
            "effect(1)(\"Radius\")",
            "effect takes the effect's name in quotes",
        ),
        (
            "effect(\"Blur\")(2)",
            "an effect parameter is named in quotes",
        ),
        (
            "thisLayer.property(\"a\", \"b\")",
            "a property is named by its id",
        ),
        ("transform.skew", "transform has no property skew"),
        ("audio.gain", "audio has no property gain"),
        (
            "function f() { return 1 }",
            "function is not part of aria.expressions",
        ),
    ];
    for (source, wanted) in cases {
        let message = compile(source).err().unwrap_or_default();
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn inputs_and_layers_read_through_the_scope_in_expression_units() {
    let mut scope = rotation();
    scope.time = 1.0;
    scope
        .table
        .insert(input(InputClip::Named("Logo".into()), "position"), |_| {
            ParamValue::Vec2([0.0, 0.0])
        });
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    scope.table.insert(
        DriverInput {
            clip: InputClip::This,
            property: InputProperty::Effect {
                effect: "Blur".into(),
                param: "Radius".into(),
            },
        },
        |seconds| ParamValue::Scalar(seconds as f32 * 10.0),
    );
    close(
        number(
            "thisComp.layer(\"Logo\").transform.position[0] + thisLayer.opacity \
             + effect(\"Blur\")(\"Radius\").valueAtTime(2) + effect(\"Blur\")(\"Radius\")",
            &mut scope,
        ),
        960.0 + 40.0 + 20.0 + 10.0,
        "inputs",
    );
    close(
        number(
            "effect(\"Blur\")(\"Radius\").velocityAtTime(0.5)",
            &mut scope,
        ),
        10.0,
        "input velocity",
    );
    // An input the scope cannot answer fails with the reference's position.
    let message = error("1 + scale[0]", &mut scope);
    assert!(
        message.starts_with("line 1, column 5: nothing answers"),
        "{message}"
    );

    scope.clips.insert(
        InputClip::This,
        DriverClipInfo {
            name: "Me".into(),
            in_point: 1.0,
            out_point: 3.0,
        },
    );
    scope.clips.insert(
        InputClip::Named("Logo".into()),
        DriverClipInfo {
            name: "Logo".into(),
            in_point: 0.5,
            out_point: 2.5,
        },
    );
    close(
        number(
            "thisLayer.outPoint - thisLayer.inPoint + thisComp.layer(\"Logo\").inPoint + inPoint",
            &mut scope,
        ),
        3.5,
        "extents",
    );
    close(
        number(
            "(thisLayer.name + thisComp.layer(\"Logo\").name).length",
            &mut scope,
        ),
        6.0,
        "names",
    );
}

#[test]
fn time_and_composition_values() {
    let mut scope = rotation();
    scope.time = 1.5;
    let cases = [
        ("time * 2", 3.0),
        ("thisComp.frameDuration * 24", 1.0),
        ("thisComp.width + thisComp.height", 3000.0),
        ("thisComp.duration", 10.0),
        ("timeToFrames()", 36.0),
        ("timeToFrames(0.1, 30)", 3.0),
        ("timeToFrames(0.7, 10)", 7.0),
        ("framesToTime(48)", 2.0),
        ("framesToTime(15, 30)", 0.5),
    ];
    for (source, expected) in cases {
        close(number(source, &mut scope), expected, source);
    }
}
