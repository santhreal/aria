//! Every status line the editor writes reads as its sentences do: it starts
//! with a capital, and it ends with a period only when it holds more than one
//! sentence, each of which then ends with one.
//!
//! The class closed here: a status line or a menu row's disabled reason that
//! started in lower case, as `a bin needs a name` and `select a pool item to
//! preview it` did, or that ended one way beside lines ending the other, as
//! `Select a clip with sound.` did beside `Select a clip first`. The lines are
//! read from the crate's sources with test items and comments blanked: the
//! literal each write to the status or error line starts with, and each reason
//! a menu guard returns, which is a disabled row's tooltip and the status line
//! when the row runs anyway. Each reason a command declines for is walked from
//! [`Decline::ALL`]. A line added later is checked on the run that adds it.
//! An error from below the editor is started with a capital where it is
//! posted, by [`super::as_sentence`].
//!
//! Not covered: a status line set from a value the census cannot read, such
//! as a reason another function returns, and the end of a template whose last
//! part is a placeholder.

use std::path::Path;

use super::{as_sentence, lowercase_start};
use crate::app::decline::Decline;
use crate::test_support::collect_rs;
use crate::test_support::control_sites::strip_comments;
use crate::test_support::sources::{string_literals, without_test_items};

/// The writes whose argument the status strip draws.
const SINKS: [&str; 2] = ["status_message = Some(", "report_error("];

/// The returns of a menu guard.
const GUARD_RETURNS: [&str; 2] = ["then_some(", "Some("];

/// The file the menu guards are defined in, relative to `src`.
const GUARDS: &str = "chrome/menu_guards.rs";

/// How `line` departs from the shape of a status line, or `None`.
fn misshapen(line: &str) -> Option<String> {
    let text = line.trim();
    if let Some(word) = lowercase_start(text) {
        return Some(format!("starts with {word:?}"));
    }
    // A template that ends in a placeholder ends in a value, and an ellipsis
    // is an action still running.
    if text.ends_with('}') || text.ends_with('…') || text.ends_with("...") {
        return None;
    }
    let several = text
        .as_bytes()
        .windows(3)
        .any(|w| matches!(w[0], b'.' | b'?' | b'!') && w[1] == b' ' && !w[2].is_ascii_whitespace());
    if !several && text.ends_with('.') {
        Some("is one sentence and ends with a period".into())
    } else if several && !text.ends_with(['.', '?', '!']) {
        Some("holds several sentences and the last ends with no period".into())
    } else {
        None
    }
}

/// A literal's contents as the program holds them: a line continuation and
/// the indent after it removed, and an escape read as its character.
fn as_written(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\n') => while chars.next_if(|c| c.is_whitespace()).is_some() {},
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The end of the string literal whose opening quote is at `open`: the byte
/// after its closing quote.
fn literal_end(code: &[u8], open: usize) -> usize {
    let mut at = open + 1;
    while at < code.len() {
        match code[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    code.len()
}

/// The end of the argument that starts at `start`, the byte after a call's
/// `(`: the byte of the `)` that closes the call.
fn argument_end(code: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut at = start;
    while at < code.len() {
        match code[at] {
            b'"' => {
                at = literal_end(code, at);
                continue;
            }
            // A character literal, which may be a bracket; a lifetime is not one.
            b'\'' if code.get(at + 1) == Some(&b'\\') => at += 3,
            b'\'' if code.get(at + 2) == Some(&b'\'') => at += 2,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            _ => {}
        }
        at += 1;
    }
    code.len()
}

/// The literal a line starts with at `at`, after optional whitespace, a `&`,
/// and a `format!(` or `String::from(`: its offset and contents.
fn literal_at(code: &str, mut at: usize) -> Option<(usize, String)> {
    let bytes = code.as_bytes();
    let skip_space = |at: &mut usize| {
        while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    };
    skip_space(&mut at);
    if bytes.get(at) == Some(&b'&') {
        at += 1;
    }
    for head in ["format!(", "String::from("] {
        if code[at..].starts_with(head) {
            at += head.len();
            skip_space(&mut at);
            break;
        }
    }
    (bytes.get(at) == Some(&b'"')).then(|| {
        let end = literal_end(bytes, at);
        (at, as_written(&code[at + 1..end - 1]))
    })
}

/// Each literal a line starts with in the argument at `start..end`: the
/// argument itself, or each arm of the `match` or `if` it is.
fn line_starts(code: &str, start: usize, end: usize) -> Vec<(usize, String)> {
    if let Some(found) = literal_at(code, start) {
        return vec![found];
    }
    let argument = code[start..end].trim_start();
    if !(argument.starts_with("match ") || argument.starts_with("if ")) {
        return Vec::new();
    }
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut at = start;
    while at < end {
        match bytes[at] {
            b'"' => {
                at = literal_end(bytes, at);
                continue;
            }
            b'{' => found.extend(literal_at(code, at + 1)),
            b'=' if bytes.get(at + 1) == Some(&b'>') => found.extend(literal_at(code, at + 2)),
            _ => {}
        }
        at += 1;
    }
    found
}

/// Each line the sources write at one of `calls`, in source order, beside
/// where it is written as `file:line`.
fn written_lines(files: &[(String, String)], calls: &[&str]) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for (rel, code) in files {
        let mut found: Vec<(usize, String)> = calls
            .iter()
            .flat_map(|call| {
                code.match_indices(call).flat_map(move |(open, _)| {
                    let start = open + call.len();
                    line_starts(code, start, argument_end(code.as_bytes(), start))
                })
            })
            .collect();
        found.sort_by_key(|(at, _)| *at);
        lines.extend(found.into_iter().map(|(at, text)| {
            let number = code[..at].matches('\n').count() + 1;
            (format!("{rel}:{number}"), text)
        }));
    }
    lines
}

/// Every production source in the crate, relative to `src`, with test items
/// and comments blanked.
fn production_sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {src:?}, which cannot be this crate",
        files.len()
    );
    files
        .iter()
        .filter_map(|file| {
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with("test_support/") || rel.ends_with("_tests.rs") {
                return None;
            }
            let raw = std::fs::read_to_string(file)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
            Some((rel, strip_comments(&without_test_items(&raw))))
        })
        .collect()
}

/// **Every status line the crate writes reads as its sentences do.**
///
/// Fails when fewer lines are read than the crate writes, so a renamed sink
/// does not leave a rule that matches nothing.
#[test]
fn every_status_line_the_crate_writes_reads_as_its_sentences() {
    let sources = production_sources();
    let posted = written_lines(&sources, &SINKS);
    let guards: Vec<_> = sources
        .iter()
        .filter(|(rel, _)| rel == GUARDS)
        .cloned()
        .collect();
    let reasons = written_lines(&guards, &GUARD_RETURNS);
    assert!(
        posted.len() > 400,
        "read only {} posted lines",
        posted.len()
    );
    assert!(
        reasons.len() > 30,
        "read only {} guard reasons",
        reasons.len()
    );
    let declined = Decline::ALL.map(|why| (format!("{why:?}"), why.reason().to_string()));
    let offenders: Vec<String> = posted
        .iter()
        .chain(&reasons)
        .chain(&declined)
        .filter_map(|(at, text)| misshapen(text).map(|why| format!("{at}: {text:?} {why}")))
        .collect();
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

#[test]
fn a_status_line_is_judged_by_its_start_and_its_sentences() {
    for (line, judged) in [
        ("Copied", None),
        ("No clip is selected", None),
        ("Nothing to render. Your timeline is empty.", None),
        ("Export failed: {err}", None),
        ("Cancelling…", None),
        ("{name}: In and Out trim the clip", None),
        ("Delete this clip?", None),
        ("a bin needs a name", Some("starts with \"a\"")),
        (
            "Select a clip with sound.",
            Some("is one sentence and ends with a period"),
        ),
        (
            "Select a clip to apply {}.",
            Some("is one sentence and ends with a period"),
        ),
        (
            "Nothing to commit. Transcribe first",
            Some("holds several sentences and the last ends with no period"),
        ),
        ("v1.2.0 is installed", None),
        ("", None),
    ] {
        assert_eq!(misshapen(line).as_deref(), judged, "{line:?}");
    }
}

/// Each form a write to the status strip takes is read where its line starts:
/// a literal, a template on the next line, the arms of a `match`, a template
/// whose arguments hold literals of their own, a line continuation, and an
/// arm whose pattern is a bracket. A value is read as nothing, and so is a
/// literal after the call.
#[test]
fn a_line_is_read_where_the_status_strip_draws_it() {
    let code = concat!(
        "self.status_message = Some(\"Copied\".into());\n",
        "state.report_error(format!(\n    \"Export failed: {e}\"\n));\n",
        "self.status_message = Some(match found {\n",
        "    Some(_) => \"Found it\".into(),\n",
        "    None => format!(\"No match for {q}\"),\n",
        "});\n",
        "self.status_message = Some(format!(\"{} · {}\", if a { \"with audio\" } else { \"video only\" }, x));\n",
        "self.report_error(err);\n",
        "self.status_message = Some(\"Ends at \\\n    the cut\".into());\n",
        "self.status_message = Some(match c { ')' => \"Closed\", _ => \"Open\" });\n",
        "let m = match k { _ => \"Not a line\" };\n",
    );
    let files = [("f.rs".to_string(), code.to_string())];
    assert_eq!(
        written_lines(&files, &SINKS),
        [
            ("f.rs:1", "Copied"),
            ("f.rs:3", "Export failed: {e}"),
            ("f.rs:6", "Found it"),
            ("f.rs:7", "No match for {q}"),
            ("f.rs:9", "{} · {}"),
            ("f.rs:11", "Ends at the cut"),
            ("f.rs:13", "Closed"),
            ("f.rs:13", "Open"),
        ]
        .map(|(at, text)| (at.to_string(), text.to_string()))
    );
}

#[test]
fn a_line_that_starts_with_a_lower_case_word_is_found() {
    for (line, found) in [
        ("included", Some("included")),
        ("stored, 3 characters", Some("stored")),
        ("  in use", Some("in")),
        (
            "not measured; Measure walks every cache directory",
            Some("not"),
        ),
        ("Stored, 3 characters", None),
        ("Video only", None),
        ("dB", None),
        ("dBFS", None),
        ("v0.1.0 · built-in · active", None),
        ("com.example.fx", None),
        ("/var/data", None),
        ("1920 × 1080", None),
        ("—", None),
        ("fx", None),
        ("ƒ Driver", None),
        ("fxaa on", Some("fxaa")),
        ("", None),
    ] {
        assert_eq!(lowercase_start(line), found, "{line:?}");
    }
}

/// An error written in lower case is posted with a capital, and every other
/// line is posted as it is written: a unit, a version, a path, a line that
/// already starts with a capital, and a line that is empty.
#[test]
fn an_error_from_below_is_posted_as_a_sentence() {
    for (line, posted) in [
        ("cancelled", "Cancelled"),
        (
            "no audio output device. Plug in speakers or a headset.",
            "No audio output device. Plug in speakers or a headset.",
        ),
        (
            "  could not list /data: denied",
            "  Could not list /data: denied",
        ),
        ("élan lost", "Élan lost"),
        ("Couldn't read that file", "Couldn't read that file"),
        ("dB out of range", "dB out of range"),
        ("v0.2.0 is not installed", "v0.2.0 is not installed"),
        ("/data is full", "/data is full"),
        ("", ""),
    ] {
        assert_eq!(as_sentence(line.to_string()), posted, "{line:?}");
    }
}

/// The error line is written through `report_error`, so an error a lower layer
/// returns in lower case reaches the strip as a sentence, whichever form it is
/// reported in.
#[test]
fn every_reported_form_reaches_the_error_line_as_a_sentence() {
    let (mut state, _clip) = super::super::premiere_tests::with_clip();
    let owned = String::from("the take is offline");
    state.report_error("the take is offline");
    assert_eq!(state.last_error.as_deref(), Some("The take is offline"));
    state.report_error(&owned);
    assert_eq!(state.last_error.as_deref(), Some("The take is offline"));
    state.report_error(owned);
    assert_eq!(state.last_error.as_deref(), Some("The take is offline"));
    state.report_error(aria_engine::EngineError::Refused(
        "preset name cannot be empty".into(),
    ));
    assert_eq!(
        state.last_error.as_deref(),
        Some("Preset name cannot be empty")
    );
}

/// **No line the crate writes joins two parts with a spaced dash.**
///
/// The class closed here: a readout, a status line or a row detail that set
/// two facts apart with ` — `, as `Peak -0.3 dBFS — clipping`, `{name} —
/// {category}` and the cache sizes joined by ` — ` did, beside the ` · ` the
/// other readouts join facts with. Facts join with ` · `, and a clause about a
/// fact follows a comma, a semicolon or a colon. Every string literal in the
/// crate's production sources is read, so a line added later is checked on
/// the run that adds it. A lone `—`, the mark for a value that is not there,
/// is not a join.
///
/// Not covered: a dash a line takes from data, such as a clip name, and a
/// join built from a separator held in a variable.
#[test]
fn no_line_the_crate_writes_joins_two_parts_with_a_spaced_dash() {
    let mut read = 0;
    let mut offenders = Vec::new();
    for (rel, code) in production_sources() {
        for (line, text) in string_literals(&code) {
            read += 1;
            if text.contains(" — ") || text.ends_with(" —") || text.starts_with("— ") {
                offenders.push(format!("{rel}:{line}: {text:?}"));
            }
        }
    }
    assert!(
        read > 5000,
        "read {read} literals, fewer than the crate writes: the rule just went vacuous"
    );
    assert!(
        offenders.is_empty(),
        "lines that join two parts with a spaced dash; join facts with ` · ` and \
         set a clause off with a comma, a semicolon or a colon:\n  {}",
        offenders.join("\n  ")
    );
}
