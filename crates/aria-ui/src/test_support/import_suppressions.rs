//! No Rust source silences the `unused_imports` lint.
//!
//! WHY: modules split into children that open with `use super::*` left their
//! parents holding imports for those children alone, 17 of them under
//! `#[allow(unused_imports)]` in the `project` and `clip` modules of
//! `aria-models` and the `model` module of `aria-transcription`. Several of
//! them named items that neither the parent nor any child read. The attribute
//! removes the warning that reports a dead import, so an import kept for a
//! child stays after the last child stops reading it. A module imports what it
//! uses.
//!
//! This walks every `.rs` file under `crates/`, `apps/`, `examples/` and
//! `fuzz/` on each run, with comments and the contents of literals blanked,
//! and fails on an `allow` or `expect` attribute, inner or outer, alone or
//! inside `cfg_attr`, whose lint list includes `unused_imports`, the `unused`
//! group, or `warnings`.
//!
//! Not caught: an unused import with no attribute, which rustc reports as a
//! warning the CI build does not fail on; a lint level set in `RUSTFLAGS` or a
//! `[lints]` table; and an import kept in a parent for a child that does read
//! it.

use std::path::Path;

use super::collect_rs;
use super::sources::code_lines;

/// The lint levels that silence a lint.
const SILENCING: [&str; 2] = ["allow", "expect"];

/// The lints whose silencing hides an unused import: the lint, the `unused`
/// group it belongs to, and `warnings`.
const HIDING: [&str; 3] = ["unused_imports", "unused", "warnings"];

/// Fewest attributes in the workspace that silence some lint. A scan that
/// finds fewer has stopped reading attributes.
const SILENCING_FLOOR: usize = 50;

/// Each attribute in `text`, `#[...]` or `#![...]`: the line its `#` is on,
/// counted from 1, and its code between the outer brackets with whitespace
/// removed. A `#` in a comment or a literal opens none.
fn attributes(text: &str) -> Vec<(usize, String)> {
    let code = code_lines(text).join("\n");
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut line = 1;
    let mut counted = 0;
    let mut at = 0;
    while let Some(offset) = code[at..].find('#') {
        let hash = at + offset;
        at = hash + 1;
        let open = at + usize::from(bytes.get(at) == Some(&b'!'));
        if bytes.get(open) != Some(&b'[') {
            continue;
        }
        let mut depth = 0usize;
        let Some(close) = (open..bytes.len()).find(|&i| {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            depth == 0
        }) else {
            break;
        };
        line += code[counted..hash].matches('\n').count();
        counted = hash;
        found.push((line, code[open + 1..close].split_whitespace().collect()));
        at = close + 1;
    }
    found
}

/// Each lint `attribute` silences: the entries of its `allow(...)` and
/// `expect(...)` lists, inside `cfg_attr` or not.
fn silenced(attribute: &str) -> Vec<&str> {
    let mut lints = Vec::new();
    for level in SILENCING {
        let mut from = 0;
        while let Some(offset) = attribute[from..].find(level) {
            let start = from + offset;
            from = start + level.len();
            let bounded = attribute[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let Some(list) = attribute[from..].strip_prefix('(') else {
                continue;
            };
            if !bounded {
                continue;
            }
            let mut depth = 1usize;
            let end = list
                .find(|c| {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })
                .unwrap_or(list.len());
            lints.extend(list[..end].split(',').filter(|lint| !lint.is_empty()));
        }
    }
    lints
}

/// The lines of `text` that hold an attribute silencing a lint in [`HIDING`].
fn hiding_lines(text: &str) -> Vec<usize> {
    attributes(text)
        .into_iter()
        .filter(|(_, attribute)| silenced(attribute).iter().any(|lint| HIDING.contains(lint)))
        .map(|(line, _)| line)
        .collect()
}

/// Each form an attribute silencing an unused import takes is found on the
/// line its `#` is on, and an attribute that silences other lints, sets a
/// stricter level, or sits in a comment or a literal is not.
#[test]
fn each_form_that_silences_an_unused_import_is_found() {
    let cases: &[(&str, &[usize])] = &[
        ("#[allow(unused_imports)]\nuse a::b;\n", &[1]),
        ("//! Docs.\n\n#![allow(unused_imports)]\n", &[3]),
        ("#[expect(unused_imports)]\nuse a::b;\n", &[1]),
        (
            "#[cfg(test)]\n#[cfg_attr(test, allow(unused_imports))]\nuse a::b;\n",
            &[2],
        ),
        (
            "fn f() {}\n#[allow(\n    dead_code,\n    unused_imports,\n)]\nuse a::b;\n",
            &[2],
        ),
        (
            "#[allow(dead_code, unused_imports, reason = \"x\")]\nuse a::b;\n",
            &[1],
        ),
        ("#[allow(unused)]\nuse a::b;\n", &[1]),
        ("#![expect(warnings)]\n", &[1]),
        (
            "let q = b'\"';\n#[allow(unused_imports)]\nuse a::b;\n",
            &[2],
        ),
        (
            "let s = r#\"\"[\"#;\n#[allow(unused_imports)]\nuse a::b;\n",
            &[2],
        ),
        ("#[deny(unused_imports)]\nuse a::b;\n", &[]),
        (
            "#[allow(unused_variables, clippy::unused_io_amount)]\nfn f() {}\n",
            &[],
        ),
        (
            "#[cfg_attr(test, deny(unused), allow(dead_code))]\nfn f() {}\n",
            &[],
        ),
        ("#[disallow(unused_imports)]\nfn f() {}\n", &[]),
        (
            "// #[allow(unused_imports)]\n/* #![allow(unused)] */\n",
            &[],
        ),
        ("const S: &str = \"#[allow(unused_imports)]\";\n", &[]),
    ];
    for (text, lines) in cases {
        assert_eq!(hiding_lines(text), *lines, "in {text:?}");
    }
}

/// **No Rust source in the workspace silences `unused_imports`.**
#[test]
fn no_source_silences_unused_imports() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps", "examples", "fuzz"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    files.sort();
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let mut silencing = 0;
    let mut offenders = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        for (line, attribute) in attributes(&text) {
            let lints = silenced(&attribute);
            silencing += usize::from(!lints.is_empty());
            if lints.iter().any(|lint| HIDING.contains(lint)) {
                let rel = file.strip_prefix(&workspace).unwrap_or(file);
                offenders.push(format!("{}:{line}: #[{attribute}]", rel.display()));
            }
        }
    }
    assert!(
        silencing >= SILENCING_FLOOR,
        "found {silencing} attributes that silence a lint, fewer than \
         {SILENCING_FLOOR}: the scan stopped reading attributes"
    );
    assert!(
        offenders.is_empty(),
        "import what each module uses in that module, and delete the attribute:\n{}",
        offenders.join("\n")
    );
}
