//! Source prose names the control, setting, file, session or caller that acts,
//! never a person.
//!
//! WHY: the persona nouns "the operator", "an operator" and "the user" stood in
//! for the actor in about six hundred comments and messages across the
//! workspace. A comment that says "the operator cannot see it" leaves the reader
//! to work out which surface fails to draw what, and the documentation register
//! that the docs test enforces already rejects the same phrases in `docs/`.
//! Rewriting the instances leaves the class open, so this walks every `.rs` file
//! under `crates/`, `apps/` and `examples/` on each run and fails on a new one.
//!
//! "the user" stays legal under `crates/aria-ai/`, where it is the chat role a
//! provider protocol defines ("the user prompt", "the user message") and the
//! system prompt addresses the model in that vocabulary. Not caught: a persona
//! phrased another way, such as "users", "the editor" meaning a person, or
//! "you" in a comment.

use std::path::Path;

use super::collect_rs;

/// Phrases rejected in every source line, matched lowercase.
const PERSONA: [&str; 2] = ["the operator", "an operator"];

/// Rejected outside [`CHAT_ROLE_ROOT`], matched lowercase.
const USER: &str = "the user";

/// The crate whose provider protocols name a chat role "user".
const CHAT_ROLE_ROOT: &str = "crates/aria-ai/";

/// Files where a phrase is the subject of the line rather than prose about the
/// product, and why. A row that stops containing a phrase fails the test, so an
/// exemption cannot outlive its reason.
const EXEMPT: &[(&str, &str)] = &[
    (
        "crates/aria-ui/src/keys/reference_doc_tests.rs",
        "the docs register test lists the phrases it rejects in docs/",
    ),
    (
        "crates/aria-ui/src/test_support/persona_nouns.rs",
        "this rule lists the phrases it rejects",
    ),
];

fn banned_in(rel: &str, line: &str) -> Vec<&'static str> {
    let lower = line.to_lowercase();
    let mut found: Vec<&'static str> = PERSONA
        .into_iter()
        .filter(|phrase| lower.contains(phrase))
        .collect();
    if !rel.starts_with(CHAT_ROLE_ROOT) && lower.contains(USER) {
        found.push(USER);
    }
    found
}

#[test]
fn source_prose_names_no_persona() {
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
        let exemption = EXEMPT.iter().position(|(name, _)| rel == *name);
        for (number, line) in text.lines().enumerate() {
            let found = banned_in(&rel, line);
            if found.is_empty() {
                continue;
            }
            match exemption {
                Some(row) => exempt_hit[row] = true,
                None => offenders.push(format!(
                    "{rel}:{}: {found:?} in {:?}",
                    number + 1,
                    line.trim()
                )),
            }
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
        "exempt files that no longer contain a persona phrase; delete their rows: {stale:?}"
    );
    assert!(
        offenders.is_empty(),
        "source prose names a person instead of the control, setting, file, session or \
         caller that acts:\n  {}",
        offenders.join("\n  ")
    );
}

/// The matcher itself, at its boundaries: case, the chat-role crate, and the
/// phrases that only look similar.
#[test]
fn the_matcher_separates_persona_from_chat_role_and_lookalikes() {
    let cases: [(&str, &str, &[&str]); 8] = [
        (
            "crates/aria-ui/src/a.rs",
            "// so the operator can fix it",
            &["the operator"],
        ),
        (
            "crates/aria-ui/src/a.rs",
            "/// What An Operator presses",
            &["an operator"],
        ),
        (
            "crates/aria-ui/src/a.rs",
            "// shown to the user as written",
            &["the user"],
        ),
        (
            "apps/aria-desktop/src/a.rs",
            "\"The User sees a splash\"",
            &["the user"],
        ),
        (
            "crates/aria-ai/src/a.rs",
            "// the user prompt and every tool call",
            &[],
        ),
        (
            "crates/aria-ai/src/a.rs",
            "// asks the operator to allow it",
            &["the operator"],
        ),
        (
            "crates/aria-plugin/src/a.rs",
            "/// `name op= value` with its binary operator",
            &[],
        ),
        (
            "crates/aria-ui/src/a.rs",
            "// imports into the preset library",
            &[],
        ),
    ];
    for (rel, line, expected) in cases {
        assert_eq!(banned_in(rel, line), expected, "{rel}: {line}");
    }
}
