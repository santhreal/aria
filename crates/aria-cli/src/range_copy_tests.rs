//! A range the command line, the editor, and a script print reads as words:
//! `from 0 to 1`, `outside 0 to 4`.
//!
//! WHY: refusals printed Rust range syntax to every surface: `opacity must be
//! in 0..=1`, `lane pan envelope 1.5 is outside -1..=1`, `clip marker tick
//! (5) is outside clip source range [0..48)`, and from Python `ramp position
//! 2 is outside 0.0…1.0`; effect, look and mask refusals printed intervals
//! as math, `radius = 2 out of range [0, 1]`. This reads every source file
//! under `crates/` and `apps/` on each run and fails on a string literal that
//! holds a number or a placeholder, then `..`, `..=` or `…`, then a number or
//! a placeholder; or `range`, `outside`, `within` or `in`, then a bracketed
//! pair of numbers or placeholders.
//!
//! Not caught: a literal that spans lines, a raw string, a range assembled
//! from parts, text in panics, assertions, and debug and trace logs, and test
//! code: a file named for tests, a file under `tests/`, `benches/` or
//! `examples/`, a file that declares a `#[test]`, and an inline test module.

use std::fs;
use std::path::Path;

use crate::count_tests::{outside_tests, rust_files};

/// The calls whose text no person reads in the product: panics, assertions,
/// and logs below `info`.
const UNREAD: [&str; 8] = [
    "expect(",
    "assert!(",
    "assert_eq!(",
    "assert_ne!(",
    "panic!(",
    "unreachable!(",
    "debug!(",
    "trace!(",
];

/// The string literals that open and close on `line`, as written between
/// their quotes. A quote in a char literal (`'"'`) opens none.
fn literals(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"'\"'") {
            at += 3;
        } else if bytes[at] == b'"' {
            let start = at + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'"' {
                end += if bytes[end] == b'\\' { 2 } else { 1 };
            }
            if end >= bytes.len() {
                break;
            }
            found.push(&line[start..end]);
            at = end + 1;
        } else {
            at += 1;
        }
    }
    found
}

/// Whether `text` holds a range written as code: a digit or a closing brace,
/// then `..`, `..=` or `…`, then a digit, a minus and a digit, or an opening
/// brace.
fn holds_range(text: &str) -> bool {
    text.char_indices().any(|(at, c)| {
        if !c.is_ascii_digit() && c != '}' {
            return false;
        }
        let rest = &text[at + c.len_utf8()..];
        let Some(after) = rest
            .strip_prefix("..=")
            .or_else(|| rest.strip_prefix(".."))
            .or_else(|| rest.strip_prefix('…'))
        else {
            return false;
        };
        let mut next = after.chars();
        match next.next() {
            Some('-') => next.next().is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit() || c == '{',
            None => false,
        }
    })
}

/// Whether `text` holds an interval written as math: `range`, `outside`,
/// `within` or `in`, then `[` or `(`, two numbers or placeholders split by a
/// comma, then `]` or `)`. A point such as `(got [1, 2])` follows no such word.
fn holds_interval(text: &str) -> bool {
    text.match_indices(['[', '(']).any(|(at, _)| {
        let word = text[..at].trim_end().rsplit(' ').next().unwrap_or_default();
        if !matches!(word, "range" | "outside" | "within" | "in") {
            return false;
        }
        let rest = &text[at + 1..];
        let Some(close) = rest.find([']', ')']) else {
            return false;
        };
        let ends: Vec<&str> = rest[..close].split(',').map(str::trim).collect();
        ends.len() == 2
            && ends.iter().all(|end| {
                end.parse::<f64>().is_ok() || (end.starts_with('{') && end.ends_with('}'))
            })
    })
}

#[test]
fn no_string_a_surface_prints_writes_a_range_as_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["crates", "apps"] {
        rust_files(&root.join(dir), &mut files);
    }
    let mut read = 0_usize;
    let mut found = Vec::new();
    for file in &files {
        let shown = file.strip_prefix(&root).unwrap_or(file);
        let test_path = shown.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("tests" | "benches" | "examples")
            )
        }) || shown
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("test"));
        if test_path {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let code = outside_tests(&text);
        if code.lines().any(|line| line.trim() == "#[test]") {
            continue;
        }
        for (index, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") || UNREAD.iter().any(|call| line.contains(call))
            {
                continue;
            }
            for literal in literals(line) {
                read += 1;
                if holds_range(literal) || holds_interval(literal) {
                    found.push(format!("{}:{}: \"{literal}\"", shown.display(), index + 1));
                }
            }
        }
    }
    assert!(
        read > 10_000,
        "read {read} string literals under crates/ and apps/"
    );
    assert!(
        found.is_empty(),
        "write each range as words, such as `from 0 to 1` or `outside 0 to 4`:\n{}",
        found.join("\n")
    );
}

/// The matchers at their boundaries: each separator, a placeholder on either
/// side, a negative end, a bracketed span, an interval after each word; and a
/// range in words, a point, a single or triple bracket, words in brackets, a
/// relative path, a trailing ellipsis, a version, and an open end. A char
/// literal quote opens no string, an escaped quote ends none, and an unclosed
/// string is not read.
#[test]
fn the_matchers_find_a_range_written_as_code_and_nothing_else() {
    for (text, range) in [
        ("opacity must be in 0..=1", true),
        ("{what} must be in {min}..={max}", true),
        ("pan 1.5 is outside -1..=1", true),
        ("tick 5 is outside [0..48)", true),
        ("radius = 2 out of range [0, 1]", true),
        ("{what} = {v} out of range [{lo}, {hi}]", true),
        ("animation speed 9 outside [0.25, 4]", true),
        ("confidence must be in (0, 1]", true),
        ("{name} must lie within [-10, 10]", true),
        ("mask center = [1, 2] must be finite", false),
        ("got [{}, {}]", false),
        ("in [{}]", false),
        ("outside [a, b]", false),
        ("outside [1, 2, 3]", false),
        ("ramp position 2 is outside 0.0…1.0", true),
        ("width must be in 0..={MAX}", true),
        ("speed in {MIN}..-1", true),
        ("opacity must be from 0 to 1", false),
        ("../../data/easings.toml", false),
        ("Loading…", false),
        ("version 1.2.3", false),
        ("waited 3…", false),
        ("{name}..", false),
        ("1..x", false),
    ] {
        assert_eq!(holds_range(text) || holds_interval(text), range, "{text}");
    }
    assert_eq!(
        literals(r#"format!("a {}", '"', "b\"c", x) "open"#),
        vec!["a {}", r#"b\"c"#]
    );
}
