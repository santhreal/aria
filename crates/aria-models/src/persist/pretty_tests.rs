//! The save formatter writes the bytes `serde_json`'s pretty printer writes
//! with a tab indent.
//!
//! A saved project is read back by every earlier build and diffed by version
//! control, so a formatter that drifts by one tab, one comma, or one line
//! break changes every saved file. Each case compares whole documents with
//! `serde_json`'s own pretty printer: empty and one-element containers (where
//! the closing bracket's line break depends on `has_value`), objects inside
//! arrays and arrays inside objects, strings that need escaping, and nesting
//! past the 32 levels one indent slice covers. It does not cover a document
//! `serde_json` itself cannot print, such as a non-string map key.

use serde::Serialize;
use serde_json::{Value, json};

use super::to_vec;
use crate::Project;
use crate::clip::{Clip, Generator};
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

/// `value` as `serde_json` pretty prints it with a tab indent.
fn serde_json_tabbed<T: ?Sized + Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    value
        .serialize(&mut serde_json::Serializer::with_formatter(
            &mut out, formatter,
        ))
        .unwrap();
    out
}

fn same_as_serde_json(value: &Value) {
    let expected = serde_json_tabbed(value);
    let written = to_vec(value).unwrap();
    assert_eq!(
        String::from_utf8(written).unwrap(),
        String::from_utf8(expected).unwrap(),
    );
}

/// `depth` arrays nested inside each other around an object, so the deepest
/// line sits `depth + 1` levels in.
fn nested(depth: usize) -> Value {
    let mut value = json!({ "leaf": [1, { "a": "b" }], "empty": {} });
    for level in 0..depth {
        value = if level % 2 == 0 {
            json!([value, level])
        } else {
            json!({ "k": value, "n": level })
        };
    }
    value
}

#[test]
fn containers_print_as_serde_json_prints_them() {
    for value in [
        json!(null),
        json!([]),
        json!({}),
        json!([[]]),
        json!({ "a": {} }),
        json!([1]),
        json!({ "a": 1 }),
        json!([1, 2, [3, []], { "x": [] }]),
        json!({ "a": [ { "b": { "c": [1, 2] } }, [] ], "d": "e" }),
        json!({ "quote\"key": "line\nbreak\ttab\\slash\u{1}", "unicode": "é漢🎬" }),
        json!([0.5, -1, 1e300, true, false, null, u64::MAX, i64::MIN]),
    ] {
        same_as_serde_json(&value);
    }
}

/// Every depth from 0 through three full indent slices, so the deepest line
/// lands below, on, and past each multiple of 32 levels.
#[test]
fn nesting_past_one_indent_slice_prints_as_serde_json_prints_it() {
    for depth in 0..=100 {
        same_as_serde_json(&nested(depth));
    }
}

/// A saved project, the document this formatter exists for.
#[test]
fn a_project_prints_as_serde_json_prints_it() {
    let rate = Rational::FPS_24;
    let mut project = Project::new("format", rate);
    let track = project.add_track(TrackKind::Text, "T1");
    for start in 0..3 {
        project
            .timeline_mut()
            .add_clip(
                track,
                Clip::generated(
                    Generator::text("title"),
                    TimeRange::at_rate(start * 48, 24, rate),
                ),
            )
            .unwrap();
    }
    assert_eq!(to_vec(&project).unwrap(), serde_json_tabbed(&project));
}
