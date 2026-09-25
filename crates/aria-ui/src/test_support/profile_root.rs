//! One place resolves the profile root, and the census that keeps it.

use std::path::Path;

use super::collect_rs;

/// Every module that may resolve the default profile directory, and why.
///
/// The rule below fails on any other one. Two of these are the whole
/// vocabulary — the function itself and the entry point that runs the app on
/// no configuration — and the third is a doc comment that names the defect.
const PROFILE_ROOT_OWNERS: &[(&str, &str)] = &[
    (
        "test_support/profile_root.rs",
        "the rule below spells out what it refuses, and would match itself",
    ),
    (
        "settings.rs",
        "the function itself, and nothing else calls it",
    ),
    (
        "lib.rs",
        "run_app: the entry point for a host that passes no configuration, \
         which is what resolving the default is for",
    ),
    (
        "plugins/mod.rs",
        "the doc comment on mount_all, naming the defect this rule closes",
    ),
];

/// **A session has one profile directory, and only its entry point resolves
/// it.**
///
/// The plugin mount used to call `default_data_dir()` itself, so a process
/// pointed at another profile with `ARIA_DATA_DIR` — a second instance, or
/// any test in this crate — wrote its settings, caches and recovery file
/// into the scratch directory while loading external plugins, catalog
/// overrides, speech models and the cloud cache out of the operator's real
/// `~/.aria`. Six test callers of `mount_all` were reading the operator's
/// profile that way.
///
/// Source-derived, so the next module that resolves its own root is caught
/// the moment it is written rather than when someone notices files in the
/// wrong directory. A path is a parameter: the caller who knows which
/// profile this is passes it, and storage is the one plugin that turns it
/// into directories.
#[test]
fn the_profile_root_is_resolved_once_by_the_entry_point() {
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
    let mut seen = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if !line.contains("default_data_dir") {
                continue;
            }
            if PROFILE_ROOT_OWNERS.iter().any(|(owner, _)| rel == *owner) {
                seen.push(rel.to_string());
                continue;
            }
            offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "these resolve the default profile directory themselves, and only an entry \
         point may ({} owner(s) declared): a session's root is a parameter, and \
         storage is what turns it into directories\n  {}",
        PROFILE_ROOT_OWNERS.len(),
        offenders.join("\n  ")
    );
    // A declared owner that no longer names it is a stale row, which is how a
    // rule quietly stops covering anything.
    for (owner, _) in PROFILE_ROOT_OWNERS {
        assert!(
            seen.iter().any(|rel| rel == owner),
            "{owner} is declared as resolving the profile root and no longer does: \
             delete the row"
        );
    }
}
