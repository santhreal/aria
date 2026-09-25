//! **A registry dispatch has no catch-all arm.**
//!
//! Every file beside this one declares a closed set: a pick, a gesture, a row, a
//! unit. The panels draw controls from those rows and hand the pressed row back
//! here to be turned into a command. That round trip is only as complete as the
//! `match` in the middle, and a `_ =>` arm is how a set stops being closed: the
//! variant added in one place compiles everywhere, does nothing when pressed,
//! and turns nothing red.
//!
//! So the arms here are exhaustive by rule, not by habit. The cost of a feature
//! is meant to be one row plus the arms the compiler then demands; with a
//! catch-all present the compiler demands nothing and the row is silently dead.
//!
//! The rule is derived from the sources and fails by default. It reads the module
//! list out of `surfaces.rs`, so a new surface is measured on the run that adds
//! it, and it carries no exemptions, so there is nothing to widen instead of
//! writing the arm. It also refuses to pass on an empty measurement: if the
//! module ever stops dispatching, the floors below go red rather than quietly
//! agreeing that zero catch-alls were found in zero arms.

use std::path::Path;

/// The smallest dispatch population that still means something. Both were far
/// exceeded when the rule landed (42 match sites, 281 arms); the floors exist so
/// a walk that finds nothing cannot report success.
const MATCH_FLOOR: usize = 20;
/// See [`MATCH_FLOOR`].
const ARM_FLOOR: usize = 100;

/// One surface file, read once.
struct Source {
    /// Path as the failure message should spell it.
    name: String,
    /// The file's text.
    text: String,
}

/// Reads `surfaces.rs` and every module it declares.
///
/// The module list comes out of the source rather than a directory listing, so a
/// file that exists but is not declared, and a declaration whose file is gone,
/// both fail here instead of being skipped.
fn sources() -> Vec<Source> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/surfaces");
    let root = dir.with_extension("rs");
    let root_text = std::fs::read_to_string(&root).expect("surfaces.rs is part of this crate");

    let mut declared = Vec::new();
    for line in root_text.lines() {
        let code = line.trim();
        if let Some(rest) = code.strip_prefix("pub mod ") {
            declared.push(
                rest.trim_end_matches(';')
                    .trim_end_matches('}')
                    .trim()
                    .to_owned(),
            );
        }
    }
    assert!(
        declared.len() > 15,
        "surfaces.rs declares {} modules, which cannot be this module: the rule just went \
         vacuous",
        declared.len()
    );

    let mut on_disk = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("the surfaces directory is part of this crate") {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .expect("a .rs file has a stem")
                .to_string_lossy()
                .into_owned();
            on_disk.push(stem);
        }
    }
    on_disk.sort();

    let mut undeclared: Vec<&String> = on_disk
        .iter()
        .filter(|stem| !declared.contains(stem) && *stem != "census_tests")
        .collect();
    undeclared.sort();
    assert!(
        undeclared.is_empty(),
        "a file under surfaces/ is not declared in surfaces.rs, so nothing compiles it and \
         this rule would not read it: {undeclared:?}"
    );

    let mut sources = vec![Source {
        name: "crates/aria-ui/src/surfaces.rs".to_owned(),
        text: root_text,
    }];
    for module in declared {
        let path = dir.join(format!("{module}.rs"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("surfaces.rs declares {module}, so {path:?} must exist"));
        sources.push(Source {
            name: format!("crates/aria-ui/src/surfaces/{module}.rs"),
            text,
        });
    }
    sources
}

/// Whether a line of code opens a catch-all arm.
///
/// A catch-all is the pattern `_`, alone or as the last alternative of an `or`
/// pattern, with or without a guard. A binding arm (`Pick::Row(id) =>`) is not
/// one: it still names the variant, so adding a variant still fails to compile.
fn is_catch_all(code: &str) -> bool {
    code.starts_with("_ =>")
        || code.starts_with("_ if ")
        || code.contains("| _ =>")
        || code.contains("| _ if ")
}

#[test]
fn no_surface_dispatch_falls_through_to_a_catch_all() {
    let sources = sources();
    let mut match_sites = 0usize;
    let mut arms = 0usize;
    let mut offenders = Vec::new();

    for source in &sources {
        for (number, line) in source.text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("match ") {
                match_sites += 1;
            }
            if code.contains("=>") {
                arms += 1;
            }
            if is_catch_all(code) {
                offenders.push(format!("{}:{}: {}", source.name, number + 1, code.trim()));
            }
        }
    }

    assert!(
        match_sites >= MATCH_FLOOR && arms >= ARM_FLOOR,
        "walked {} file(s) and found {match_sites} match site(s) and {arms} arm(s), under the \
         {MATCH_FLOOR}/{ARM_FLOOR} floor: the surfaces stopped dispatching, or this walk stopped \
         reading them",
        sources.len()
    );
    assert!(
        offenders.is_empty(),
        "a surface dispatch falls through to a catch-all, so a row added to its registry \
         compiles and then does nothing when pressed. Write the arm instead: the point of a \
         closed set is that the compiler names every place a new variant has to be \
         handled.\n  {}",
        offenders.join("\n  ")
    );
}
