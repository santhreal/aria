//! Every integration test file in the workspace is built by a test target.
//!
//! A crate with `autotests = false` builds only the `[[test]]` targets its
//! manifest declares, so a file added under `tests/` without a declaration
//! compiles in no target and its tests never run while the suite stays green.
//! The crates and their `tests/` directories are walked at run time, so a new
//! crate or a new file is covered with no edit here. A top-level file is
//! accounted for when a `[[test]]` table names it or another file under
//! `tests/` mounts it as a module. Does not check that a declared target's
//! tests pass, and does not cover crates that discover their tests.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// `path` with `.` and `..` folded away, so two spellings of one file compare
/// equal without touching the file system.
fn fold(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The value of a `key = "value"` line, if `line` is one.
fn string_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    rest.trim().strip_prefix('"')?.split('"').next()
}

/// Every file a `[[test]]` table in `manifest` builds, relative to the crate:
/// its `path`, or `tests/<name>.rs` when it states none.
fn declared(manifest: &str) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut table: Option<(Option<String>, Option<String>)> = None;
    let mut close = |table: &mut Option<(Option<String>, Option<String>)>| match table.take() {
        Some((_, Some(path))) => {
            out.insert(fold(Path::new(&path)));
        }
        Some((Some(name), None)) => {
            out.insert(PathBuf::from(format!("tests/{name}.rs")));
        }
        Some((None, None)) | None => {}
    };
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            close(&mut table);
            if line == "[[test]]" {
                table = Some((None, None));
            }
            continue;
        }
        if let Some((name, path)) = table.as_mut() {
            if let Some(value) = string_value(line, "name") {
                *name = Some(value.to_owned());
            } else if let Some(value) = string_value(line, "path") {
                *path = Some(value.to_owned());
            }
        }
    }
    close(&mut table);
    out
}

/// Every `.rs` file under `dir`, at any depth.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Every file another file under `tests/` mounts as a module, relative to the
/// crate: a `#[path = ".."]` beside the mounting file, or `mod name;` as
/// `name.rs` or `name/mod.rs` beside it.
fn mounted(krate: &Path, files: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for file in files {
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let dir = file.parent().unwrap_or(krate);
        for line in text.lines().map(str::trim) {
            if let Some(path) = line
                .strip_prefix("#[path")
                .and_then(|rest| string_value(&format!("path{rest}"), "path").map(str::to_owned))
            {
                out.insert(fold(&dir.join(path)));
                continue;
            }
            let line = line.strip_prefix("pub ").unwrap_or(line);
            if let Some(name) = line
                .strip_prefix("mod ")
                .and_then(|rest| rest.strip_suffix(';'))
            {
                let name = name.trim();
                out.insert(fold(&dir.join(format!("{name}.rs"))));
                out.insert(fold(&dir.join(name).join("mod.rs")));
            }
        }
    }
    out.into_iter()
        .filter_map(|path| path.strip_prefix(krate).ok().map(Path::to_path_buf))
        .collect()
}

#[test]
fn every_integration_test_file_is_built_by_a_target() {
    let crates = fold(&Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    let mut checked = 0;
    let mut orphans = Vec::new();
    let mut members: Vec<PathBuf> = fs::read_dir(&crates)
        .expect("read the crates directory")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    members.sort();
    for krate in members {
        let Ok(manifest) = fs::read_to_string(krate.join("Cargo.toml")) else {
            continue;
        };
        if !manifest
            .lines()
            .any(|line| line.trim() == "autotests = false")
        {
            continue;
        }
        let tests = krate.join("tests");
        let mut files = Vec::new();
        sources(&tests, &mut files);
        let declared = declared(&manifest);
        let mounted = mounted(&krate, &files);
        for file in &files {
            if file.parent() != Some(tests.as_path()) {
                continue;
            }
            let Ok(relative) = file.strip_prefix(&krate) else {
                continue;
            };
            checked += 1;
            if !declared.contains(relative) && !mounted.contains(relative) {
                orphans.push(file.strip_prefix(&crates).unwrap_or(file).to_path_buf());
            }
        }
    }
    assert!(
        checked >= 50,
        "the walk found {checked} top-level test files in crates that declare their targets"
    );
    assert!(
        orphans.is_empty(),
        "no test target builds {orphans:?}: add a [[test]] table to the crate's Cargo.toml \
         or mount the file from a declared target"
    );
}

#[test]
fn a_table_without_a_path_builds_the_file_named_after_it() {
    let manifest = "[package]\nname = \"x\"\n\n[[test]]\nname = \"alpha\"\npath = \"tests/a.rs\"\n\n[[test]]\nname = \"beta\"\n\n[dependencies]\npath = \"tests/not-a-test.rs\"\n";
    let found: Vec<PathBuf> = declared(manifest).into_iter().collect();
    assert_eq!(
        found,
        vec![PathBuf::from("tests/a.rs"), PathBuf::from("tests/beta.rs")]
    );
}
