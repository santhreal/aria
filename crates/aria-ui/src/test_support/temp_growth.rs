//! Temp paths that grow unbounded are refused, by census.

use std::path::Path;

use super::collect_rs;

/// A file allowed to name a temp path near a clock or a pid, and why.
///
/// One entry: the rule's own vocabulary. Everything else that trips this is a
/// defect, and a new entry is a deliberate act with a reason attached.
const GROWING_NAMES_ALLOWED: &[(&str, &str)] = &[
    (
        "aria-ui/src/test_support.rs",
        "the walker the censuses share, named here so the entry below has a home",
    ),
    (
        "aria-ui/src/test_support/temp_growth.rs",
        "the rule spells out what it refuses, and would match itself",
    ),
];

/// The same class as the pool above, at workspace scope: **the set of paths a
/// run can name is fixed, so a thousand runs create what one run creates.**
///
/// The pool fixed this crate. Every other crate kept naming fixtures
/// `…_{nanos}` or `…-{pid}-{n}`, which is one new entry per run per case, in
/// a temp root nothing empties — 27 leftovers in the repo's own scratch
/// directory before this rule existed. A clock or a pid in a temp path is the
/// whole defect: both are unique per run *by design*, so the name can never
/// be reused and the directory can only grow. A counter is fine, because a
/// counter restarts at zero.
///
/// Derived from the sources at run time, one crate-relative path per
/// offender, so a crate added tomorrow is covered without touching this
/// list, and a new `{nanos}` fixture turns the suite red on the run that
/// introduces it.
#[test]
fn no_crate_names_a_temp_path_that_grows() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("this crate sits inside the crates directory")
        .to_path_buf();
    let mut files = Vec::new();
    collect_rs(&crates, &mut files);
    assert!(
        files.len() > 500,
        "found {} source files under {crates:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    // What makes a name unique per run, and therefore unreusable. A pid or a
    // clock far from any temp path is somebody else's business — a
    // transactional sibling beside a real config file, say — so the window is
    // the naming site, not the file.
    const PER_RUN: &[&str] = &["SystemTime::now()", "process::id()", "UNIX_EPOCH"];
    const WINDOW: usize = 8;
    let mut naming = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&crates)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if GROWING_NAMES_ALLOWED.iter().any(|(name, _)| rel == *name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains("temp_dir") {
            continue;
        }
        naming += 1;
        let lines: Vec<&str> = text.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            if !line.contains("temp_dir") {
                continue;
            }
            let from = number.saturating_sub(WINDOW);
            let to = (number + WINDOW + 1).min(lines.len());
            for source in PER_RUN {
                if lines[from..to].iter().any(|near| near.contains(source)) {
                    offenders.push(format!(
                        "{rel}:{}: names a temp path within {WINDOW} lines of {source}",
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(
        naming >= 10,
        "only {naming} files name a temp path, so this scan is reading the wrong tree"
    );
    assert!(
        offenders.is_empty(),
        "a temp path must be reusable: use a fixed name, or a counter that restarts \
         at zero, never a clock or a pid (exceptions declared: {}):\n  {}",
        GROWING_NAMES_ALLOWED.len(),
        offenders.join("\n  ")
    );
}
