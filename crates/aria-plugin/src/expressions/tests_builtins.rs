use super::*;

#[test]
fn interpolation_follows_its_curves_and_clamps() {
    let cases = [
        ("linear(0.5, 0, 2, 0, 100)", 25.0),
        ("linear(0.5, 0, 2, 100, 0)", 75.0),
        ("linear(5, 0, 2, 0, 100)", 100.0),
        ("linear(-1, 0, 2, 0, 100)", 0.0),
        ("linear(0.25, 10, 20)", 12.5),
        ("linear(1, 1, 1, 0, 10)", 10.0),
        ("linear(0.5, 2, 0, 0, 100)", 75.0),
        ("ease(1, 0, 2, 0, 100)", 50.0),
        ("ease(0.5, 0, 2, 0, 100)", 15.625),
        ("easeIn(0.5, 0, 2, 0, 100)", 10.9375),
        ("easeOut(0.5, 0, 2, 0, 100)", 29.6875),
        ("easeIn(2, 0, 2, 0, 100) + easeOut(0, 0, 2, 0, 100)", 100.0),
        ("linear(0.5, [0, 10], [10, 30])[1]", 20.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    assert_eq!(
        run("linear(0.5, [0, 0], [960, 1080])", &mut position),
        Ok(ParamValue::Vec2([-0.25, 0.0]))
    );
    let message = error("linear(0.5, [0], 1)", &mut rotation());
    assert!(message.contains("two numbers or two arrays"), "{message}");
}

#[test]
fn vector_and_colour_functions() {
    let cases = [
        ("length([3, 4])", 5.0),
        ("length([1, 1], [4, 5])", 5.0),
        ("length(-3)", 3.0),
        ("dot([1, 2], [3, 4])", 11.0),
        ("cross([1, 0, 0], [0, 1, 0])[2]", 1.0),
        ("cross([1, 0], [0, 1])[2]", 1.0),
        ("normalize([3, 4])[0]", 0.6),
        ("normalize([0, 0])[1]", 0.0),
        ("add([1, 2], [3, 4])[1]", 6.0),
        ("([1, 2] + [3, 4])[0]", 4.0),
        ("mul([1, 2], 3)[1]", 6.0),
        ("(3 * [1, 2])[1]", 6.0),
        ("div([2, 4], 2)[1]", 2.0),
        ("sub([5], [1, 1])[1]", -1.0),
        ("(-[1, 2])[1]", -2.0),
        ("clamp(150, 0, 100)", 100.0),
        ("clamp(-1, 0, 100)", 0.0),
        ("clamp([-5, 50], 0, [10, 20])[1]", 20.0),
        ("degreesToRadians(180)", std::f64::consts::PI),
        ("radiansToDegrees(Math.PI)", 180.0),
        ("hslToRgb([0, 1, 0.5, 1])[0]", 1.0),
        ("hslToRgb([1 / 3, 1, 0.5])[1]", 1.0),
        ("hslToRgb([0, 0, 0.3])[2]", 0.3),
        ("rgbToHsl([0, 0, 1])[0]", 2.0 / 3.0),
        ("rgbToHsl([0.2, 0.4, 0.6, 0.5])[3]", 0.5),
        ("hslToRgb(rgbToHsl([0.2, 0.4, 0.6]))[2]", 0.6),
        ("noise(3)", 0.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut rotation()), expected, source);
    }
    for (source, wanted) in [
        (
            "[1, 2] * [3, 4]",
            "an array of 2 numbers * an array of 2 numbers is not defined",
        ),
        ("[1, 2] + 1", "is not defined"),
        ("dot([1], [1, 2])", "two arrays of one length"),
        ("cross([1], [1, 2])", "arrays of 2 or 3 numbers"),
        ("clamp(1, 0)", "clamp takes 3 arguments, not 2"),
        ("[\"a\"]", "an array holds numbers"),
    ] {
        let message = error(source, &mut rotation());
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn wiggle_is_seeded_smooth_and_bounded() {
    let at = |source: &str, time: f64, seed: u64| {
        let mut scope = scope(ClipParam::Rotation, ParamValue::Scalar(10.0));
        scope.time = time;
        scope.seed = seed;
        number(source, &mut scope)
    };
    let first = at("wiggle(2, 5)", 1.3, 7);
    assert_eq!(first, at("wiggle(2, 5)", 1.3, 7), "repeatable");
    assert_ne!(
        first,
        at("wiggle(2, 5)", 1.3, 8),
        "seeded per clip and property"
    );
    assert!(
        (at("wiggle(2, 5)", 1.3001, 7) - first).abs() < 0.05,
        "smooth over a tenth of a millisecond"
    );
    assert_eq!(
        at("wiggle(2, 5, 1, 0.5, 3)", 1.3, 7),
        at("wiggle(2, 5)", 3.0, 7),
        "the time argument"
    );
    assert_eq!(at("wiggle(2, 0)", 1.3, 7), 10.0);

    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for step in 0..400 {
        let shaken = at("wiggle(3, 5, 3, 0.5)", f64::from(step) * 0.025, 7);
        assert!((shaken - 10.0).abs() <= 5.0 * 1.75 + 1e-4, "{shaken}");
        low = low.min(shaken);
        high = high.max(shaken);
    }
    assert!(high - low > 2.0, "wiggle moves: {low}..{high}");

    // Each axis moves on its own.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    position.time = 0.7;
    let Ok(ParamValue::Vec2([x, y])) = run("wiggle(1, 100)", &mut position) else {
        panic!("wiggle on a position is a position");
    };
    assert_ne!(x * 1920.0, y * 1080.0);

    // An input wiggles from its own value.
    let mut scope = rotation();
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    close(
        number("thisLayer.opacity.wiggle(1, 0)", &mut scope),
        40.0,
        "input wiggle",
    );
}

#[test]
fn random_repeats_per_frame_and_reseeds() {
    let at = |source: &str, time: f64| {
        let mut scope = rotation();
        scope.time = time;
        number(source, &mut scope)
    };
    assert_eq!(at("random()", 1.0), at("random()", 1.0));
    assert_ne!(at("random()", 1.0), at("random()", 2.0));
    assert_ne!(at("random() - random()", 1.0), 0.0, "each call draws anew");
    assert_eq!(
        at("seedRandom(4, true); random()", 1.0),
        at("seedRandom(4, true); random()", 2.0),
        "timeless"
    );
    assert_ne!(
        at("seedRandom(4, true); random()", 1.0),
        at("seedRandom(5, true); random()", 1.0)
    );
    let mut inside = 0;
    let mut sum = 0.0;
    for step in 0..400 {
        let time = f64::from(step) * 0.01;
        let uniform = at("random(10, 20)", time);
        assert!((10.0..20.0).contains(&uniform), "{uniform}");
        let item = at("random([5, 5])[1]", time);
        assert!((0.0..5.0).contains(&item), "{item}");
        assert!((0.0..1.0).contains(&at("Math.random()", time)));
        let gauss = at("gaussRandom(0, 10)", time);
        sum += gauss;
        inside += usize::from((0.0..=10.0).contains(&gauss));
    }
    // Four standard errors of the mean of 400 draws.
    assert!(
        (sum / 400.0 - 5.0).abs() < 0.6,
        "gaussian mean {}",
        sum / 400.0
    );
    assert!(inside >= 340, "{inside} of 400 within range");
    let message = error("random(1, 2, 3)", &mut rotation());
    assert!(
        message.contains("random takes 0 to 2 arguments, not 3"),
        "{message}"
    );
}

#[test]
fn noise_is_continuous_and_bounded() {
    for step in 0..500 {
        let x = f64::from(step) * 0.037 - 5.0;
        let here = number(&format!("noise({x})"), &mut rotation());
        let next = number(&format!("noise({})", x + 1e-5), &mut rotation());
        assert!((-1.0..=1.0).contains(&here), "{x}: {here}");
        assert!((next - here).abs() < 2e-4, "{x}: {here} -> {next}");
        let pair = number(&format!("noise([{x}, {}])", x * 2.0), &mut rotation());
        assert!((-1.0..=1.0).contains(&pair), "{x}: {pair}");
    }
}

fn keyed(keys: &[(f64, f32)], time: f64) -> Scope {
    let mut scope = rotation();
    scope.keys = keys
        .iter()
        .map(|&(time, value)| (time, ParamValue::Scalar(value)))
        .collect();
    scope.time = time;
    scope
}

#[test]
fn loops_repeat_the_keyframes_before_and_after() {
    let ramp = [(0.0, 0.0), (1.0, 10.0)];
    let cases = [
        ("loopOut()", 1.25, 2.5),
        ("loopOut(\"cycle\")", 2.25, 2.5),
        ("loopOut(\"pingpong\")", 1.25, 7.5),
        ("loopOut(\"pingpong\")", 2.25, 2.5),
        ("loopOut(\"offset\")", 1.25, 12.5),
        ("loopOut(\"offset\")", 2.25, 22.5),
        ("loopOut(\"continue\")", 1.25, 12.5),
        ("loopOut(\"continue\")", 3.0, 30.0),
        ("loopOut()", 0.5, 5.0),
        ("loopIn()", -0.25, 7.5),
        ("loopIn(\"pingpong\")", -0.25, 2.5),
        ("loopIn(\"pingpong\")", -1.25, 7.5),
        ("loopIn(\"offset\")", -0.25, -2.5),
        ("loopIn(\"continue\")", -0.25, -2.5),
        ("loopIn()", 0.5, 5.0),
        ("thisProperty.loopOut(\"offset\")", 1.25, 12.5),
    ];
    for (source, time, expected) in cases {
        close(
            number(source, &mut keyed(&ramp, time)),
            expected,
            &format!("{source} at {time}"),
        );
    }
    // Counting keyframes back from the last, or on from the first.
    let peak = [(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
    close(
        number("loopOut(\"cycle\", 1)", &mut keyed(&peak, 2.25)),
        7.5,
        "the last segment",
    );
    close(
        number("loopOut(\"cycle\", 0)", &mut keyed(&peak, 2.25)),
        2.5,
        "every segment",
    );
    close(
        number("loopIn(\"cycle\", 1)", &mut keyed(&peak, -0.25)),
        7.5,
        "the first segment",
    );
    // Fewer than two keyframes leave the value.
    close(
        number("loopOut()", &mut keyed(&[(0.0, 3.0)], 5.0)),
        3.0,
        "one key",
    );

    let mut scope = keyed(&ramp, 2.0);
    scope.table.insert(input(InputClip::This, "opacity"), |_| {
        ParamValue::Scalar(0.4)
    });
    for (source, wanted) in [
        (
            "thisLayer.opacity.loopOut()",
            "loopOut reads the keyframes of the property the driver is on",
        ),
        ("loopOut(\"bounce\")", "loopOut has no type \"bounce\""),
        ("loopIn(3)", "the type of loopIn is a string"),
    ] {
        let message = error(source, &mut scope);
        assert!(message.contains(wanted), "{source}: {message}");
    }
}

#[test]
fn keyframes_and_rates_of_change() {
    let peak = [(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
    let cases = [
        ("numKeys", 3.0),
        ("thisProperty.numKeys", 3.0),
        ("key(2).time", 1.0),
        ("key(2).value", 10.0),
        ("key(3).index", 3.0),
        ("valueAtTime(1.5)", 5.0),
        ("velocityAtTime(0.5)", 10.0),
        ("speedAtTime(1.5)", 10.0),
        ("thisProperty.velocity", 10.0),
        ("thisProperty.speed", 10.0),
        ("value", 5.0),
    ];
    for (source, expected) in cases {
        close(number(source, &mut keyed(&peak, 0.5)), expected, source);
    }
    for source in ["key(0)", "key(4)", "key(1.5)"] {
        let message = error(source, &mut keyed(&peak, 0.5));
        assert!(message.contains("is out of range"), "{source}: {message}");
    }
    // A position's velocity is in pixels per second.
    let mut position = scope(ClipParam::Position, ParamValue::Vec2([0.0, 0.0]));
    position.keys = vec![
        (0.0, ParamValue::Vec2([0.0, 0.0])),
        (1.0, ParamValue::Vec2([0.5, 0.0])),
    ];
    position.time = 0.5;
    let Ok(ParamValue::Vec2([x, y])) = run("velocityAtTime(0.5)", &mut position) else {
        panic!("a velocity of a position is a position");
    };
    close(f64::from(x), 960.0 / 1920.0 - 0.5, "x");
    close(f64::from(y), -0.5, "y");
}
