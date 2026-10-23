//! One owner for temp paths, and the census that keeps it.

use std::path::Path;

use super::collect_rs;

/// Every module that can name a path in the system temp dir, and why.
///
/// The rule below fails on any other one. A second caller is how the disk
/// filled three times: each of those was one line, added in good faith, in a
/// module nobody thought of as owning storage. Growing this table is a
/// deliberate act with a reason attached, which is the point.
const TEMP_OWNERS: &[(&str, &str)] = &[
    (
        "scratch_pool.rs",
        "the pool itself: five fixed slots, reused by every run",
    ),
    (
        "test_support/temp_ownership.rs",
        "this rule names the needle it refuses, and would match itself",
    ),
    (
        "test_support/temp_growth.rs",
        "the growing-names rule also names the needle it refuses",
    ),
    (
        "plugins/media.rs",
        "EncoderConfig carries no destination, so the encoder names one fixed \
         file per output shape and overwrites it",
    ),
    (
        "settings.rs",
        "the fallback when the platform reports no home directory",
    ),
];

/// The class fix for a disk filled three times: one owner for temp paths, and
/// a rule that goes red the moment a second module names one.
///
/// Source-derived rather than a list of known call sites, so a new module is
/// covered the moment it exists. Every offender is collected and reported at
/// once: a rule that panics on the first turns one run into a queue of runs.
#[test]
fn the_pool_is_the_only_temp_owner() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 50,
        "found {} source files under {src:?}, which cannot be this crate: the rule \
         just went vacuous",
        files.len()
    );
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if TEMP_OWNERS.iter().any(|(owner, _)| rel == *owner) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if line.contains("temp_dir") {
                offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these name a path in the system temp dir, and only the pool may \
         (owners declared: {}):\n  {}",
        TEMP_OWNERS.len(),
        offenders.join("\n  ")
    );
}
