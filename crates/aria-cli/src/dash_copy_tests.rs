//! Text the command line, the editor, the assistant, and a script print joins
//! its clauses with a colon, a semicolon, or a comma, never an em dash.
//!
//! WHY: the `aria --help` banner, the assistant's routing and linked-clip
//! refusals, its tool descriptions and system prompt, the template bundler's
//! refusal, and the gallery's specimen labels each split a sentence with an
//! em dash, against the copy register every other surface follows. This reads
//! every line of code under `crates/` and `apps/` on each run, so a string
//! that spans lines is read too, and fails on an em dash anywhere but the
//! lone `"—"` a readout prints for a value the selection does not share.
//!
//! Not caught: text assembled from parts, text in panics, assertions, and
//! debug and trace logs, doc comments that a schema copies into a tool
//! description, and test code: a file named for tests, a file under `tests/`,
//! `benches/` or `examples/`, a file that declares a `#[test]`, and an inline
//! test module.

use crate::range_copy_tests::surface_lines;

/// The one string an em dash may be: a readout's mark for a value the
/// selected items do not share.
const MIXED: &str = "\"—\"";

/// Whether `code` holds an em dash outside the lone [`MIXED`] mark.
fn holds_dash(code: &str) -> bool {
    code.replace(MIXED, "").contains('—')
}

#[test]
fn no_text_a_surface_prints_holds_an_em_dash() {
    let mut found = Vec::new();
    let read = surface_lines(|file, line, code| {
        if holds_dash(code) {
            found.push(format!("{}:{line}: {}", file.display(), code.trim()));
        }
    });
    assert!(read > 100_000, "read {read} lines under crates/ and apps/");
    assert!(
        found.is_empty(),
        "join the clauses with a colon, a semicolon, or a comma:\n{}",
        found.join("\n")
    );
}

/// The matcher at its boundaries: a dash in a literal, on a continued line, and
/// beside the mixed mark; and the mark alone, the mark twice, an en dash, and a
/// hyphen.
#[test]
fn the_matcher_finds_an_em_dash_and_nothing_else() {
    for (code, dash) in [
        (r#"format!("none yet — add one")"#, true),
        ("    the state below — do not call \\", true),
        (r#"("—", "a — b")"#, true),
        (r#"MIXED: &str = "—";"#, false),
        (r#"("—", "—")"#, false),
        ("pages 3–5", false),
        ("frame-accurate", false),
    ] {
        assert_eq!(holds_dash(code), dash, "{code}");
    }
}
