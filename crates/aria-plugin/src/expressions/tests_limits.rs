use aria_models::DriverCode;

use super::super::eval::{MAX_STEPS, MAX_STRING};
use super::super::{EXPRESSIONS_LANGUAGE, ExpressionsPlugin};
use super::*;
use crate::PluginContext;

#[test]
fn a_runaway_program_ends_with_an_error() {
    // The clock runs out: the program stops at the next read.
    let mut scope = rotation();
    scope.clock.set(Some(2));
    let message = error("while (true) {}", &mut scope);
    assert_eq!(message, "the driver ran past its time budget");
    assert_eq!(scope.clock_reads.get(), 3);

    // A clock that never runs out still ends.
    let message = error("for (;;) {}", &mut rotation());
    assert_eq!(
        message,
        format!("the driver took more than {MAX_STEPS} steps")
    );

    // A string cannot outgrow its limit, and an array does not grow.
    let message = error(
        "let s = \"ab\"; while (true) { s = s + s }",
        &mut rotation(),
    );
    assert_eq!(
        message,
        format!("line 1, column 36: a string grew past {MAX_STRING} bytes")
    );
    let message = error("let a = [1]; a[5] = 2", &mut rotation());
    assert!(
        message.contains("index 5 is past the end of a, which holds 1"),
        "{message}"
    );
}

#[test]
fn the_deepest_accepted_nesting_evaluates_on_a_default_thread_stack() {
    let shapes: [(&str, fn(usize) -> String); 12] = [
        ("parentheses", |n| {
            format!("{}1{}", "(".repeat(n), ")".repeat(n))
        }),
        ("negation", |n| format!("{}1", "- ".repeat(n))),
        ("not", |n| format!("{}1", "!".repeat(n))),
        ("calls", |n| {
            format!("{}1{}", "Math.abs(".repeat(n), ")".repeat(n))
        }),
        ("arrays", |n| format!("{}1{}", "[".repeat(n), "]".repeat(n))),
        ("ternaries", |n| {
            format!("{}1{}", "1 ? ".repeat(n), " : 0".repeat(n))
        }),
        ("blocks", |n| format!("{}1{}", "{".repeat(n), "}".repeat(n))),
        ("ifs", |n| format!("{}1", "if (1) ".repeat(n))),
        ("indexes", |n| format!("[1]{}", "[0]".repeat(n))),
        ("sums", |n| format!("1{}", " + 1".repeat(n))),
        ("assignments", |n| {
            format!("let a = 0; {}1", "a = ".repeat(n))
        }),
        ("members", |n| format!("thisProperty{}", ".value".repeat(n))),
    ];
    for (shape, build) in shapes {
        let deepest = (1..2000)
            .take_while(|&depth| compile(&build(depth)).is_ok())
            .last()
            .unwrap_or_else(|| panic!("{shape}: even depth 1 fails"));
        let refusal = compile(&build(deepest + 1)).err().unwrap_or_default();
        assert!(
            refusal.contains("nests more than"),
            "{shape} at {}: {refusal}",
            deepest + 1
        );
        let source = build(deepest);
        // The default stack of a spawned thread, which render threads use.
        let evaluated = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let mut scope = rotation();
                run(&source, &mut scope).map(|_| ()).err()
            })
            .unwrap_or_else(|err| panic!("{err}"))
            .join();
        assert!(evaluated.is_ok(), "{shape} at depth {deepest} panicked");
    }
}

#[test]
fn errors_state_the_line_and_column() {
    for (source, wanted) in [
        ("let a = 1\nfoo + a", "line 2, column 1: foo is not defined"),
        (
            "thisComp.layer(\"X\")",
            "line 1, column 15: thisComp.layer(\"Name\") is a layer",
        ),
        (
            "Math.nope(1)",
            "line 1, column 10: Math has no function nope",
        ),
        (
            "thisComp.depth",
            "line 1, column 9: thisComp has no property depth",
        ),
        (
            "wiggle(1)",
            "line 1, column 7: wiggle takes 2 to 5 arguments, not 1",
        ),
        ("thisLayer.scale.foo(1)", "a property has no method foo"),
        ("thisComp", "line 1, column 1: thisComp is not a value"),
        ("1 < \"a\"", "the number 1 < a string is not defined"),
        ("effect(\"Blur\")", "effect(\"Name\")(\"Parameter\")"),
    ] {
        let message = error(source, &mut rotation());
        assert!(message.contains(wanted), "{source}: {message}");
    }
    let message = compile("1 +").err().unwrap_or_default();
    assert!(message.starts_with("line 1, column 4:"), "{message}");
}

/// The pick whip inserts what [`super::bind::reference`] writes. Every
/// property the catalog and the transform table name, on this clip and on a
/// named clip whose name needs escaping, binds back to the input it was
/// written for. A reference the binder reads as another property, or a name
/// the lexer reads back changed, fails here.
#[test]
fn every_pick_whip_reference_binds_back_to_its_input() {
    let clips = [
        InputClip::This,
        InputClip::Named("Logo".into()),
        InputClip::Named("say \"hi\" \\ back\nslash\t\u{e9}".into()),
    ];
    let mut properties: Vec<InputProperty> = aria_models::clip_param_catalog()
        .iter()
        .map(|spec| InputProperty::Id(spec.id.to_string()))
        .chain(
            super::bind::TRANSFORM
                .iter()
                .map(|(_, id)| InputProperty::Id((*id).into())),
        )
        .collect();
    properties.push(InputProperty::Id("volume".into()));
    properties.push(InputProperty::Id("pan".into()));
    properties.push(InputProperty::Effect {
        effect: "Gaussian \"Blur\"".into(),
        param: "Radius".into(),
    });
    for clip in &clips {
        for property in &properties {
            let wanted = DriverInput {
                clip: clip.clone(),
                property: property.clone(),
            };
            let source = Expressions
                .reference(&wanted)
                .unwrap_or_else(|| panic!("no reference for {wanted:?}"));
            let program = compile(&source).unwrap_or_else(|err| panic!("{source}: {err}"));
            assert_eq!(program.inputs(), std::slice::from_ref(&wanted), "{source}");
        }
    }
    // A new driver starts at the property's own value.
    let template = Expressions.template();
    let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(12.0));
    assert_eq!(run(&template, &mut scope), Ok(ParamValue::Scalar(12.0)));
}

#[test]
fn the_plugin_registers_the_language_for_every_render_path() {
    let ctx = PluginContext::new();
    ctx.mount(&ExpressionsPlugin)
        .unwrap_or_else(|err| panic!("{err}"));
    let languages = ctx.driver_languages();
    assert_eq!(languages.len(), 1);
    assert_eq!(languages[0].spec.id, EXPRESSIONS_LANGUAGE);
    assert_eq!(&*languages[0].plugin_id, EXPRESSIONS_LANGUAGE);
    assert!(aria_render::driver_languages().contains(&EXPRESSIONS_LANGUAGE.to_string()));
    let program = aria_render::compile_driver(&DriverCode::new(EXPRESSIONS_LANGUAGE, "value * 2"))
        .unwrap_or_else(|err| panic!("{err}"));
    let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(3.0));
    assert_eq!(program.evaluate(&mut scope), Ok(ParamValue::Scalar(6.0)));
    let refused = aria_render::compile_driver(&DriverCode::new(EXPRESSIONS_LANGUAGE, "1 +"));
    assert!(refused.is_err());
    // The registry forwards the pick whip and the template through the
    // plugin's timing wrapper.
    let logo = input(InputClip::Named("Logo".into()), "position");
    assert_eq!(
        aria_render::driver_reference(EXPRESSIONS_LANGUAGE, &logo).as_deref(),
        Some("thisComp.layer(\"Logo\").transform.position")
    );
    assert_eq!(
        aria_render::driver_template(EXPRESSIONS_LANGUAGE).as_deref(),
        Some("value")
    );
    assert_eq!(aria_render::driver_template("nobody.language"), None);
}
