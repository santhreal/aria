//! WHY: closes the class of a color a script writes that the binding reads
//! wrong. Every color argument passes through `convert::parse_color`, so each
//! defect below was shared by every call that takes a color:
//!
//! - `"aé123"`, six bytes with a character across the second byte, panicked
//!   slicing the string by byte offsets and raised `PanicException`, which
//!   escapes `except Exception`;
//! - `"+12345"` read as `#012345`, since `from_str_radix` takes a sign;
//! - `(300, 0, 0)`, `(-1, 0, 0)`, `[255, 0, 0]` and a five-tuple all raised
//!   one generic message that stated neither the component at fault nor its
//!   range.
//!
//! The census test derives every parameter named `color` or `background`
//! from the module's signatures, so a new color argument without a row turns
//! the suite red. Not caught: a color argument under another name, and the
//! setters and keyword dictionaries listed in `UNSIGNED`, which no signature
//! shows.

use std::collections::BTreeSet;
use std::ffi::CString;

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// What `code` leaves in `result`, after binding `aria` and running it; a
/// raised exception is `Type: message`.
fn run(code: &str) -> String {
    Python::attach(|py| {
        let globals = PyDict::new(py);
        globals
            .set_item("aria", pyo3::wrap_pymodule!(crate::aria)(py))
            .expect("binding a module into a fresh dict cannot fail");
        let indented = code.replace('\n', "\n    ");
        let script = CString::new(format!(
            "try:\n    {indented}\n\
             except BaseException as err:\n    result = type(err).__name__ + ': ' + str(err)\n"
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

/// Every call that takes a color, with `COLOR` where the color goes, keyed by
/// the `Owner.parameter` its signature shows.
const SIGNED: [(&str, &str); 14] = [
    ("Solid.color", "aria.Solid(COLOR)"),
    ("Text.color", "aria.Text('x', color = COLOR)"),
    ("TextStroke.color", "aria.TextStroke(COLOR)"),
    ("TextBackground.color", "aria.TextBackground(COLOR)"),
    ("TextShadow.color", "aria.TextShadow(COLOR)"),
    ("ShapeStroke.color", "aria.ShapeStroke(COLOR)"),
    ("Rect.color", "aria.Rect(color = COLOR)"),
    ("Ellipse.color", "aria.Ellipse(color = COLOR)"),
    ("Polygon.color", "aria.Polygon(6, color = COLOR)"),
    ("Star.color", "aria.Star(5, color = COLOR)"),
    ("Line.color", "aria.Line(color = COLOR)"),
    ("Arrow.color", "aria.Arrow(color = COLOR)"),
    ("Heart.color", "aria.Heart(color = COLOR)"),
    (
        "Project.background",
        "aria.Project('c', background = COLOR)",
    ),
];

/// Color arguments no signature shows: a property setter and a keyword
/// dictionary.
const UNSIGNED: [&str; 2] = [
    "setattr(aria.Project('c'), 'background', COLOR)",
    "aria.Project('c').add_track('text').add(aria.Text('x'), 0.0, 1.0).set_style(color = COLOR)",
];

/// Parameters named like a color that take something else, with what.
const NOT_A_COLOR: [(&str, &str); 2] = [
    ("Text.background", "a TextBackground"),
    ("Project.add_marker.color", "a marker palette name"),
];

/// The tail every color refusal ends with.
const FORMS: &str = "write (r, g, b) or (r, g, b, a) with each component a whole number \
                     from 0 to 255, \"#rrggbb\", \"#rrggbbaa\", or one of white, black, red, \
                     green, blue, transparent";

/// A color a script may write, and what each call raises for it.
const REFUSED: [(&str, &str); 14] = [
    (
        "(300, 0, 0)",
        "invalid color (300, 0, 0): 300 is not a whole number from 0 to 255",
    ),
    (
        "(-1, 0, 0)",
        "invalid color (-1, 0, 0): -1 is not a whole number from 0 to 255",
    ),
    (
        "(0, 0, 0, 256)",
        "invalid color (0, 0, 0, 256): 256 is not a whole number from 0 to 255",
    ),
    (
        "(255.0, 0, 0)",
        "invalid color (255.0, 0, 0): 255.0 is not a whole number from 0 to 255",
    ),
    ("(1, 2)", "invalid color (1, 2): a tuple of 2 components"),
    (
        "(1, 2, 3, 4, 5)",
        "invalid color (1, 2, 3, 4, 5): a tuple of 5 components",
    ),
    (
        "[255, 0, 0]",
        "invalid color [255, 0, 0]: a color is a tuple or a string",
    ),
    (
        "'aé123'",
        "invalid color \"aé123\": not a hex color or a color name",
    ),
    (
        "'#aé123'",
        "invalid color \"#aé123\": not a hex color or a color name",
    ),
    (
        "'+12345'",
        "invalid color \"+12345\": not a hex color or a color name",
    ),
    (
        "'#ff80'",
        "invalid color \"#ff80\": not a hex color or a color name",
    ),
    (
        "'#ff8000f'",
        "invalid color \"#ff8000f\": not a hex color or a color name",
    ),
    (
        "'#ff8000ff00'",
        "invalid color \"#ff8000ff00\": not a hex color or a color name",
    ),
    (
        "'mauve'",
        "invalid color \"mauve\": not a hex color or a color name",
    ),
];

/// Every parameter named `color` or `background` on a callable of the module
/// or a method of one of its classes, as `Owner.parameter`.
fn signed_color_parameters() -> BTreeSet<String> {
    let listed = run("import inspect\n\
         found = []\n\
         for name in dir(aria):\n\
         \x20   owner = getattr(aria, name)\n\
         \x20   members = [(name, owner)]\n\
         \x20   if isinstance(owner, type) and not issubclass(owner, BaseException):\n\
         \x20       members += [(name + '.' + key, value) for key, value in vars(owner).items() if callable(value) and not key.startswith('_')]\n\
         \x20   for label, member in members:\n\
         \x20       try:\n\
         \x20           parameters = inspect.signature(member).parameters\n\
         \x20       except (TypeError, ValueError):\n\
         \x20           continue\n\
         \x20       found += [label + '.' + key for key in parameters if key in ('color', 'background')]\n\
         result = '\\n'.join(found)");
    listed.lines().map(str::to_owned).collect()
}

#[test]
fn every_color_parameter_has_a_row() {
    let found = signed_color_parameters();
    let declared: BTreeSet<String> = SIGNED
        .iter()
        .map(|(key, _)| *key)
        .chain(NOT_A_COLOR.iter().map(|(key, _)| *key))
        .map(str::to_owned)
        .collect();
    assert!(
        found.len() >= SIGNED.len(),
        "the census read {found:?}: the module's signatures did not introspect"
    );
    assert_eq!(
        found, declared,
        "a color parameter without a row in SIGNED, or a row for one that is gone"
    );
}

#[test]
fn every_call_that_takes_a_color_rejects_a_malformed_one_with_its_rule() {
    let calls = SIGNED.iter().map(|(_, call)| *call).chain(UNSIGNED);
    for call in calls {
        for (color, message) in REFUSED {
            let code = format!("{}\nresult = 'accepted'", call.replace("COLOR", color));
            assert_eq!(
                run(&code),
                format!("ValueError: {message}; {FORMS}"),
                "{call} with {color}"
            );
        }
    }
}

#[test]
fn every_written_form_reads_as_the_color_it_spells() {
    let read = [
        ("(255, 128, 0)", "Solid(r=255, g=128, b=0, a=255)"),
        ("(255, 128, 0, 200)", "Solid(r=255, g=128, b=0, a=200)"),
        ("(True, 0, 0)", "Solid(r=1, g=0, b=0, a=255)"),
        ("'#FF8000'", "Solid(r=255, g=128, b=0, a=255)"),
        ("'ff8000c8'", "Solid(r=255, g=128, b=0, a=200)"),
        ("'#0a0B0c'", "Solid(r=10, g=11, b=12, a=255)"),
        ("'Transparent'", "Solid(r=0, g=0, b=0, a=0)"),
        ("'BLUE'", "Solid(r=0, g=0, b=255, a=255)"),
    ];
    for (color, solid) in read {
        assert_eq!(
            run(&format!("result = repr(aria.Solid({color}))")),
            solid,
            "{color}"
        );
    }
    for call in SIGNED.iter().map(|(_, call)| *call).chain(UNSIGNED) {
        assert_eq!(
            run(&format!(
                "{}\nresult = 'accepted'",
                call.replace("COLOR", "'#3060c0'")
            )),
            "accepted",
            "{call}"
        );
    }
}
