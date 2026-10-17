//! What `aria plugin dev` and `aria plugin pack` report when the plugin crate
//! fails to build.
//!
//! Closes the class "a failed build reports only cargo's exit status": cargo
//! colours its diagnostics when `CARGO_TERM_COLOR=always` (set by CI toolchain
//! actions and by many shells), so an error line started with an escape
//! sequence instead of `error` and the reason was dropped, along with the
//! `help:` line holding the fix. Not covered: diagnostics cargo prints in a
//! format other than rustc's human-readable one.

use super::{first_error, strip_escapes};

/// Stderr of a wasm plugin build on a toolchain without the wasm target, as
/// cargo prints it with `CARGO_TERM_COLOR=always`.
const MISSING_TARGET: &str = "\u{1b}[1m\u{1b}[92m   Compiling\u{1b}[0m bitflags v2.10.0\n\
\u{1b}[1m\u{1b}[91merror[E0463]\u{1b}[0m\u{1b}[1m: can't find crate for `core`\u{1b}[0m\n\
  \u{1b}[1m\u{1b}[94m|\u{1b}[0m\n\
  \u{1b}[1m\u{1b}[94m= \u{1b}[0m\u{1b}[1mnote\u{1b}[0m: the `wasm32-unknown-unknown` target may not be installed\n\
  \u{1b}[1m\u{1b}[94m= \u{1b}[0m\u{1b}[1mhelp\u{1b}[0m: consider downloading the target with `rustup target add wasm32-unknown-unknown`\n\
\n\
\u{1b}[1mFor more information about this error, try `rustc --explain E0463`.\u{1b}[0m\n\
\u{1b}[1m\u{1b}[91merror\u{1b}[0m\u{1b}[1m:\u{1b}[0m could not compile `bitflags` (lib) due to 1 previous error\n";

#[test]
fn a_coloured_build_failure_reports_the_error_and_its_fix() {
    assert_eq!(
        first_error(MISSING_TARGET).as_deref(),
        Some(
            "error[E0463]: can't find crate for `core` \
             (note: the `wasm32-unknown-unknown` target may not be installed; \
             help: consider downloading the target with `rustup target add wasm32-unknown-unknown`)"
        )
    );
}

#[test]
fn the_first_error_is_reported_with_only_its_own_advice() {
    let cases: [(&str, Option<&str>); 5] = [
        // No advice: the error line alone.
        (
            "   Compiling demo v0.1.0\nerror[E0425]: cannot find value `x` in this scope\n --> src/lib.rs:1:1\n",
            Some("error[E0425]: cannot find value `x` in this scope"),
        ),
        // A warning before the error is not the error, and its help is not
        // the error's.
        (
            "warning: unused import\n  = help: remove it\nerror: expected `;`\n  = note: found `}`\n",
            Some("error: expected `;` (note: found `}`)"),
        ),
        // Advice stops at the next diagnostic.
        (
            "error[E1]: first\n  = help: fix first\nerror[E2]: second\n  = help: fix second\n",
            Some("error[E1]: first (help: fix first)"),
        ),
        // A build that printed no error reports nothing, and the caller falls
        // back to the exit status.
        ("   Compiling demo v0.1.0\nwarning: unused\n", None),
        ("", None),
    ];
    for (stderr, expected) in cases {
        assert_eq!(first_error(stderr).as_deref(), expected, "{stderr:?}");
    }
}

#[test]
fn escape_sequences_are_removed_and_text_is_kept() {
    let cases = [
        ("\u{1b}[1m\u{1b}[91merror\u{1b}[0m: x", "error: x"),
        // An OSC 8 hyperlink, closed by ESC \ and by BEL.
        (
            "see \u{1b}]8;;https://doc.rust-lang.org\u{1b}\\E0463\u{1b}]8;;\u{1b}\\ now",
            "see E0463 now",
        ),
        ("a\u{1b}]8;;file:///x\u{7}b\u{1b}]8;;\u{7}c", "abc"),
        (
            "plain `text` [with] brackets",
            "plain `text` [with] brackets",
        ),
        ("ü\u{1b}[0mé", "üé"),
        // A sequence cut off at the end of the text.
        ("tail\u{1b}[1", "tail"),
        ("tail\u{1b}", "tail"),
    ];
    for (text, expected) in cases {
        assert_eq!(strip_escapes(text), expected, "{text:?}");
    }
}
