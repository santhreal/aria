//! WHY: closes the class of a shape descriptor that holds a value the model
//! rejects. `aria.Rect(width=-5)` built a descriptor and the refusal came later,
//! from `Track.add`, as an `AriaError` far from the line that wrote the width;
//! `aria.Text(size=-5)` already failed at its constructor. Every shape
//! constructor now checks the generator it hands a track against the model's
//! own validator, so each row below fails where the script wrote the value,
//! with the model's words.
//!
//! Each row drives every size argument of one descriptor through both sides of
//! its bound, the corner radius where the descriptor takes one, and a stroke
//! wider than the model allows. Not caught: a shape class added to the module
//! without a row here.

use std::ffi::CString;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// `repr(expression)`, or `Type: message` for what it raised.
fn outcome(expression: &str) -> String {
    Python::attach(|py| {
        let globals = PyDict::new(py);
        globals
            .set_item("aria", pyo3::wrap_pymodule!(crate::aria)(py))
            .expect("binding a module into a fresh dict cannot fail");
        let script = CString::new(format!(
            "try:\n    result = repr({expression})\n\
             except Exception as err:\n    result = type(err).__name__ + ': ' + str(err)\n"
        ))
        .expect("no NUL in the script");
        py.run(&script, Some(&globals), None)
            .unwrap_or_else(|err| panic!("the script raised {}", err.value(py)));
        globals
            .get_item("result")
            .expect("reading a dict cannot fail")
            .expect("the script sets result")
            .extract::<String>()
            .expect("result is a str")
    })
}

/// A shape descriptor: its constructor call with `ARGS` where the keyword
/// arguments go, the repr of its defaults, its two size arguments, and
/// whether it takes a corner radius.
struct Descriptor {
    call: &'static str,
    repr: &'static str,
    sizes: [&'static str; 2],
    corner_radius: bool,
}

const DESCRIPTORS: [Descriptor; 7] = [
    Descriptor {
        call: "aria.Rect(ARGS)",
        repr: "Rect(width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: true,
    },
    Descriptor {
        call: "aria.Ellipse(ARGS)",
        repr: "Ellipse(width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: true,
    },
    Descriptor {
        call: "aria.Arrow(ARGS)",
        repr: "Arrow(width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: true,
    },
    Descriptor {
        call: "aria.Heart(ARGS)",
        repr: "Heart(width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: true,
    },
    Descriptor {
        call: "aria.Polygon(6, ARGS)",
        repr: "Polygon(6, width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: true,
    },
    Descriptor {
        call: "aria.Star(5, ARGS)",
        repr: "Star(5, inner_ratio=0.5, width=200, height=200)",
        sizes: ["width", "height"],
        corner_radius: false,
    },
    Descriptor {
        call: "aria.Line(ARGS)",
        repr: "Line(length=200, thickness=8)",
        sizes: ["length", "thickness"],
        corner_radius: false,
    },
];

const SIZE: &str =
    "ValueError: invalid parameter: shape size must be positive and at most 8192 reference px";

#[test]
fn a_shape_rejects_a_value_the_model_rejects_where_the_script_wrote_it() {
    for descriptor in &DESCRIPTORS {
        let call = |args: &str| descriptor.call.replace("ARGS", args);
        assert_eq!(outcome(&call("")), descriptor.repr, "{}", call(""));
        for size in descriptor.sizes {
            for (value, expected) in [
                ("-5", SIZE),
                ("0", SIZE),
                ("float('nan')", SIZE),
                ("float('inf')", SIZE),
                ("8193", SIZE),
                ("8192", ""),
                ("0.5", ""),
            ] {
                let made = call(&format!("{size}={value}"));
                let got = outcome(&made);
                if expected.is_empty() {
                    let class = descriptor.repr.split('(').next().unwrap_or_default();
                    assert!(got.starts_with(&format!("{class}(")), "{made} gave {got:?}");
                } else {
                    assert_eq!(got, expected, "{made}");
                }
            }
        }
        if descriptor.corner_radius {
            let made = call("corner_radius=-1");
            assert_eq!(
                outcome(&made),
                "ValueError: invalid parameter: corner radius must be from 0 to 8192 reference px",
                "{made}"
            );
        }
        let made = call("stroke=aria.ShapeStroke('red', width=513)");
        assert_eq!(
            outcome(&made),
            "ValueError: invalid parameter: stroke width must be from 0 to 512 reference px",
            "{made}"
        );
        let placed = format!(
            "aria.Project('shapes', fps=30).add_track('sticker').add({}, 0.0, 1.0).duration",
            call("")
        );
        assert_eq!(outcome(&placed), "1.0", "{placed}");
    }
}

#[test]
fn a_count_or_ratio_out_of_range_is_rejected_at_the_constructor() {
    for (made, expected) in [
        (
            "aria.Polygon(2)",
            "ValueError: invalid parameter: polygon sides must be from 3 to 20",
        ),
        (
            "aria.Polygon(21)",
            "ValueError: invalid parameter: polygon sides must be from 3 to 20",
        ),
        ("aria.Polygon(20)", "Polygon(20, width=200, height=200)"),
        (
            "aria.Star(2)",
            "ValueError: invalid parameter: star points must be from 3 to 20",
        ),
        (
            "aria.Star(5, inner_ratio=1.5)",
            "ValueError: invalid parameter: star inner_ratio must be from 0 to 1",
        ),
        (
            "aria.Star(5, inner_ratio=0)",
            "Star(5, inner_ratio=0, width=200, height=200)",
        ),
        ("aria.ShapeStroke('red')", "ShapeStroke(width=8)"),
    ] {
        assert_eq!(outcome(made), expected, "{made}");
    }
}
