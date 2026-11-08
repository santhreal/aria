//! An I/O failure is worded by `aria_core::io_reason`, which `aria_models`
//! re-exports, never by the standard library's `Display`.
//!
//! WHY: `std::io::Error` displays as "No such file or directory (os error 2)"
//! and "Permission denied (os error 13)". Formatting the error itself into a
//! message put that errno into the keymap, theme, preset, skill, plugin and
//! assistant-history errors the editor shows, while the import and recents
//! paths already read through `io_reason`. Fixing the sites leaves the class
//! open, so this reads every source file of every crate that depends on
//! `aria-core` or `aria-models` on each run and fails on a new one.
//!
//! A site is a closure `.map_err(|err| format!("...{err}..."))`, its body in a
//! block or not, whose receiver, the expression since the previous `;` or
//! brace, calls the file system or
//! starts a process, and a `match` over such a call whose `Err(err)` arm
//! formats `{err}`. Not caught: an error passed positionally (`"{}", err`) or
//! through `to_string()`, one held in a value and formatted later, a receiver
//! that reaches the file system through a function of its own, and crates that
//! do not depend on `aria-core`, which has the only `io_reason`.

use std::path::Path;

use super::collect_rs;

/// Calls whose error is a `std::io::Error`. The process call is spelled with
/// `concat!` so that the spawn census does not read this file as starting one.
const IO_CALLS: [&str; 16] = [
    concat!("Command", "::new"),
    "fs::",
    "File::open",
    "File::create",
    "OpenOptions",
    ".read_to_string(",
    ".read_to_end(",
    ".write_all(",
    ".sync_all(",
    ".read_dir(",
    ".metadata(",
    ".spawn()",
    ".create_dir",
    ".remove_file",
    ".rename(",
    ".canonicalize(",
];

/// Crates that depend on `aria-core` or `aria-models` and are not read, and why.
const EXEMPT: &[(&str, &str)] = &[
    (
        "aria-bench",
        "the benchmark harness: its errors end a run a developer reads in a log, \
         never a message the editor shows",
    ),
    (
        "aria-plugin-wasm",
        "host-call errors are results handed to plugin code, which states them in \
         its own words",
    ),
];

/// The expression a `.map_err` at `at` is called on: back to the previous
/// statement end or brace.
fn receiver(text: &str, at: usize) -> &str {
    let head = &text[..at];
    let start = [";", "{\n", "}\n"]
        .iter()
        .filter_map(|end| head.rfind(end).map(|i| i + end.len()))
        .max()
        .unwrap_or(0);
    &head[start..]
}

/// The string literal opening `rest`, after leading whitespace.
fn literal(rest: &str) -> Option<&str> {
    let rest = rest.trim_start().strip_prefix('"')?;
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some(&rest[..i]),
            _ => escaped = false,
        }
    }
    None
}

/// Whether the `format!` whose arguments open `args` interpolates `ident` by
/// name and does not bind that name to `io_reason`.
fn formats_raw(args: &str, ident: &str) -> bool {
    let Some(lit) = literal(args) else {
        return false;
    };
    if !lit.contains(&format!("{{{ident}}}")) && !lit.contains(&format!("{{{ident}:")) {
        return false;
    }
    let mut depth = 1usize;
    let end = args
        .char_indices()
        .find(|&(_, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth == 0
        })
        .map_or(args.len(), |(i, _)| i);
    !args[..end].contains("io_reason(")
}

fn calls_io(code: &str) -> bool {
    IO_CALLS.iter().any(|call| code.contains(call))
}

/// Byte offsets in `text` of every site that formats an I/O error itself.
fn sites(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices(".map_err(|") {
        let rest = &text[at + ".map_err(|".len()..];
        let Some(bar) = rest.find('|') else { continue };
        let ident = rest[..bar].split(':').next().unwrap_or("").trim();
        let body = rest[bar + 1..].trim_start();
        let body = body.strip_prefix('{').map_or(body, str::trim_start);
        let Some(body) = body.strip_prefix("format!(") else {
            continue;
        };
        if formats_raw(body, ident) && calls_io(receiver(text, at)) {
            found.push(at);
        }
    }
    for (at, _) in text.match_indices("Err(") {
        let rest = &text[at + "Err(".len()..];
        let Some(close) = rest.find(')') else {
            continue;
        };
        let ident = &rest[..close];
        if ident.is_empty() || !ident.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(arm) = rest[close + 1..].trim_start().strip_prefix("=>") else {
            continue;
        };
        let arm = &arm[..arm.len().min(240)];
        let Some(open) = arm.find("format!(") else {
            continue;
        };
        if !formats_raw(&arm[open + "format!(".len()..], ident) {
            continue;
        }
        let Some(matched) = text[..at].rfind("match ") else {
            continue;
        };
        if at - matched > 1500 {
            continue;
        }
        let scrutinee = &text[matched..];
        let scrutinee = &scrutinee[..scrutinee.find('{').unwrap_or(0)];
        if calls_io(scrutinee) {
            found.push(at);
        }
    }
    found
}

/// The sources a census reads in one crate: no test module, file, or folder.
fn shipped(rel: &str, text: &str) -> Option<String> {
    if rel.ends_with("_tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/benches/")
        || rel.contains("/test_support")
    {
        return None;
    }
    let cut = text.find("#[cfg(test)]\nmod tests {").unwrap_or(text.len());
    Some(text[..cut].to_owned())
}

/// **An I/O error is worded by `io_reason`.**
///
/// Derived from the workspace: a crate that starts depending on `aria-core` or
/// `aria-models` is read on the run that adds the dependency, and an exemption for a crate
/// that no longer depends on it fails.
#[test]
fn an_io_error_is_worded_by_io_reason() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut crates = Vec::new();
    for root in ["crates", "apps"] {
        let Ok(entries) = std::fs::read_dir(workspace.join(root)) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
            let depends = manifest.lines().any(|line| {
                let line = line.trim_start();
                line.starts_with("aria-core") || line.starts_with("aria-models")
            });
            if depends || name == "aria-core" {
                crates.push((name, dir));
            }
        }
    }
    for (name, _) in EXEMPT {
        assert!(
            crates.iter().any(|(listed, _)| listed == name),
            "{name} is exempt but no longer depends on aria-core or aria-models; delete its row"
        );
    }
    crates.retain(|(name, _)| !EXEMPT.iter().any(|(exempt, _)| exempt == name));
    assert!(
        crates.len() >= 10
            && ["aria-core", "aria-settings", "aria-ui"]
                .iter()
                .all(|read| crates.iter().any(|(name, _)| name == read)),
        "read {} crates, which cannot be this workspace: the rule just went vacuous",
        crates.len()
    );

    let mut read = 0usize;
    let mut offenders = Vec::new();
    for (_, dir) in &crates {
        let mut files = Vec::new();
        collect_rs(dir, &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&workspace)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let Some(text) = shipped(&rel, &text) else {
                continue;
            };
            read += 1;
            for at in sites(&text) {
                let line = text[..at].matches('\n').count() + 1;
                offenders.push(format!("{rel}:{line}"));
            }
        }
    }
    assert!(
        read > 500,
        "read {read} source files: the rule just went vacuous"
    );
    assert!(
        offenders.is_empty(),
        "an I/O error formatted with its own Display shows the OS's errno, as in \
         \"Permission denied (os error 13)\"; name it \
         `err = io_reason(&err)` in the format! arguments, the `aria_core` function \
         `aria_models` re-exports:\n  {}",
        offenders.join("\n  ")
    );
}

/// The site shapes the census reads, each beside the shape it lets through.
#[test]
fn the_census_reads_both_site_shapes_and_passes_a_reasoned_one() {
    let cases = [
        (
            "let f = std::fs::File::open(p).map_err(|err| format!(\"{}: {err}\", p))?;",
            1,
        ),
        (
            "let f = std::fs::File::open(p)\n    .map_err(|err| format!(\"{}: {err}\", p, \
             err = aria_models::io_reason(&err)))?;",
            0,
        ),
        (
            "std::fs::create_dir_all(p).map_err(|err| {\n    format!(\n        \"{}: {err}\",\n        \
             p.display()\n    )\n})?;",
            1,
        ),
        (
            "let f = std::fs::File::open(p).map_err(|err| format!(\"{}: {}\", p, \
             aria_models::io_reason(&err)))?;",
            0,
        ),
        (
            "let x = toml::from_str(t).map_err(|err| format!(\"bad: {err}\"))?;",
            0,
        ),
        (
            "let text = fs::read_to_string(p)?;\nlet x = toml::from_str(&text).map_err(|err| \
             format!(\"bad: {err}\"))?;",
            0,
        ),
        (
            "let d = match std::fs::read(p) {\n    Ok(d) => d,\n    Err(err) => return \
             format!(\"{}: {err}\", p),\n};",
            1,
        ),
        (
            "let d = match parse(p) {\n    Ok(d) => d,\n    Err(err) => return \
             format!(\"{}: {err}\", p),\n};",
            0,
        ),
    ];
    for (code, expected) in cases {
        assert_eq!(sites(code).len(), expected, "{code}");
    }
}
