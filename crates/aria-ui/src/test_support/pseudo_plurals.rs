//! A count is written with the noun that agrees with it, `1 frame` and
//! `3 frames`, never `frame(s)`.
//!
//! WHY: status lines, job details, error messages and command-line output wrote
//! counts beside pseudo-plurals such as `frame(s)` and `entit(ies)`, and the
//! messages that did agree chose the suffix with an inline
//! `if n == 1 { "" } else { "s" }` or a local helper. `aria_core::count` is the
//! single definition of the agreement. This walks every `.rs` file under
//! `crates/`, `apps/` and `examples/` on each run and fails on a string literal
//! that writes a pseudo-plural, in test code as well as production code so no
//! assertion pins the old wording, and on a comparison with one that picks
//! between a word and its plural.
//!
//! Not caught: a noun that is always plural, such as `"{n} bytes"`; a
//! comparison written `1 == n`, as a `match`, or against a number other than
//! one; and an irregular plural picked inline, such as `"child"` and
//! `"children"`.

use std::path::Path;

use super::collect_rs;
use super::sources::string_literals;

/// What follows a letter in a pseudo-plural.
const SUFFIXES: [&str; 3] = ["(s)", "(es)", "(ies)"];

/// The comparisons with one that open an agreement ternary, whitespace removed.
const COMPARISONS: [&str; 2] = ["==1{\"", "!=1{\""];

/// Files where a pseudo-plural is the subject of the line rather than product
/// text, and why. A row whose file stops containing one fails the test, so an
/// exemption cannot outlive its reason.
const EXEMPT: &[(&str, &str)] = &[(
    "crates/aria-ui/src/test_support/pseudo_plurals.rs",
    "this rule writes the forms it rejects",
)];

/// Each pseudo-plural in the contents of one string literal, with the word it
/// ends.
fn pseudo_plurals(literal: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for suffix in SUFFIXES {
        for (at, _) in literal.match_indices(suffix) {
            let stem = literal[..at].trim_end_matches(|c: char| c.is_ascii_alphabetic());
            if stem.len() < at {
                found.push(&literal[stem.len()..at + suffix.len()]);
            }
        }
    }
    found
}

/// Whether `many` is `one` with a plural suffix: `""` and `"s"`, `"frame"` and
/// `"frames"`, `"match"` and `"matches"`, `"entity"` and `"entities"`.
fn is_plural_of(many: &str, one: &str) -> bool {
    matches!(many.strip_prefix(one), Some("s" | "es"))
        || one
            .strip_suffix('y')
            .is_some_and(|stem| many.strip_prefix(stem) == Some("ies"))
}

/// Each comparison with one in `text` that picks between a word and its
/// plural: the line it starts on, counted from 1, and the comparison with its
/// whitespace removed. Comments are read too, so a commented-out form counts.
fn agreement_ternaries(text: &str) -> Vec<(usize, String)> {
    let mut squeezed = String::with_capacity(text.len());
    let mut line_of = Vec::with_capacity(text.len());
    let mut line = 1;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
        } else if !c.is_whitespace() {
            squeezed.push(c);
            line_of.extend(std::iter::repeat_n(line, c.len_utf8()));
        }
    }
    let mut found = Vec::new();
    for comparison in COMPARISONS {
        for (at, _) in squeezed.match_indices(comparison) {
            let rest = &squeezed[at + comparison.len()..];
            let Some((first, rest)) = rest.split_once('"') else {
                continue;
            };
            let Some(rest) = rest.strip_prefix("}else{\"") else {
                continue;
            };
            let Some((second, rest)) = rest.split_once('"') else {
                continue;
            };
            if rest.starts_with('}') && (is_plural_of(second, first) || is_plural_of(first, second))
            {
                let end = squeezed.len() - rest.len() + 1;
                found.push((line_of[at], squeezed[at..end].to_owned()));
            }
        }
    }
    found
}

#[test]
fn every_count_agrees_with_its_noun() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps", "examples"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    files.sort();
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let mut exempt_hit = vec![false; EXEMPT.len()];
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let mut found = Vec::new();
        for (line, literal) in string_literals(&text) {
            for word in pseudo_plurals(&literal) {
                found.push(format!("{rel}:{line}: `{word}`"));
            }
        }
        for (line, ternary) in agreement_ternaries(&text) {
            found.push(format!("{rel}:{line}: `{ternary}`"));
        }
        if found.is_empty() {
            continue;
        }
        match EXEMPT.iter().position(|(name, _)| rel == *name) {
            Some(row) => exempt_hit[row] = true,
            None => offenders.extend(found),
        }
    }
    let stale: Vec<&str> = EXEMPT
        .iter()
        .zip(&exempt_hit)
        .filter(|(_, hit)| !**hit)
        .map(|((name, _), _)| *name)
        .collect();
    assert!(
        stale.is_empty(),
        "exempt files that no longer contain a pseudo-plural; delete their rows: {stale:?}"
    );
    assert!(
        offenders.is_empty(),
        "pseudo-plurals and inline noun picks; write the count with \
         `aria_core::count::counted`, or `noun` for the noun or verb alone:\n  {}",
        offenders.join("\n  ")
    );
}

/// The literal matcher at its boundaries: each suffix, a literal that spans
/// lines, and a parenthesis that follows no letter.
#[test]
fn the_literal_matcher_finds_each_suffix_after_a_letter() {
    let cases: [(&str, &[&str]); 5] = [
        ("Exported 3 frame(s)", &["frame(s)"]),
        ("2 match(es) and 1 entit(ies)", &["match(es)", "entit(ies)"]),
        ("the clip \\\n         layer(s) above", &["layer(s)"]),
        ("Settings draws no \"(s)\" label", &[]),
        ("3 frames, 1 entity", &[]),
    ];
    for (literal, expected) in cases {
        assert_eq!(pseudo_plurals(literal), expected, "{literal:?}");
    }
}

/// The ternary matcher at its boundaries: both comparisons, both orders, a
/// suffix, a word, a `y` plural, a form spread over lines, and lookalikes that
/// pick between unrelated words or compare with another number.
#[test]
fn the_ternary_matcher_finds_a_word_picked_against_its_plural() {
    let cases: [(&str, &[(usize, &str)]); 7] = [
        (
            r#"let s = if n == 1 { "" } else { "s" };"#,
            &[(1, r#"==1{""}else{"s"}"#)],
        ),
        (
            "let a = 0;\nif n != 1 {\n    \"frames\"\n} else {\n    \"frame\"\n}",
            &[(2, r#"!=1{"frames"}else{"frame"}"#)],
        ),
        (
            r#"f(if looks == 1 { "s" } else { "" })"#,
            &[(1, r#"==1{"s"}else{""}"#)],
        ),
        (
            r#"if n == 1 { "entity" } else { "entities" }"#,
            &[(1, r#"==1{"entity"}else{"entities"}"#)],
        ),
        (
            r#"if n == 1 { "match" } else { "matches" }"#,
            &[(1, r#"==1{"match"}else{"matches"}"#)],
        ),
        (r#"if n == 1 { "left" } else { "right" }"#, &[]),
        (r#"if n == 2 { "" } else { "s" }"#, &[]),
    ];
    for (text, expected) in cases {
        let found = agreement_ternaries(text);
        let found: Vec<(usize, &str)> = found
            .iter()
            .map(|(line, form)| (*line, form.as_str()))
            .collect();
        assert_eq!(found, expected, "{text:?}");
    }
}
