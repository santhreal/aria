//! One module starts another program, and the census that keeps it.

use std::path::Path;

use super::collect_rs;

/// A file allowed to start another program, and why.
///
/// Six entries and a self-reference. Everything else that trips this is a
/// defect: a second spawn site is a second policy, and the policy here is
/// that a launch needs the application's permission.
const SPAWN_OWNERS: &[(&str, &str)] = &[
    (
        "crates/aria-ui/src/app/launch.rs",
        "the owner: one call, disabled unless the headed editor enabled it",
    ),
    (
        "crates/aria-cli/src/plugin/process.rs",
        "`aria plugin` runs cargo and the editor, only on the command that names them",
    ),
    (
        "crates/aria-cli/tests/support/mod.rs",
        "runs the `aria` binary this crate builds for its integration tests and reads \
         what it prints; the crate has no library to call in process",
    ),
    (
        "crates/aria-bench/src/process.rs",
        "benchmark harness: probes the host and drives the built editor binary, never runs under the editor",
    ),
    (
        "crates/aria-decoder/src/exit_tests.rs",
        "re-runs its own test binary as the child whose exit it times; starts no other program",
    ),
    (
        "crates/aria-ui/src/test_support/catalogs.rs",
        "re-runs its own test binary as the child a catalog-changing test runs in; starts \
         no other program",
    ),
    (
        "crates/aria-ui/src/test_support/processes.rs",
        "the rule below names the call it refuses, and would match itself",
    ),
];

/// **Starting another application is one module's job, and it is off by
/// default.**
///
/// The defect this closes was not a wrong path, it was a second spawn site.
/// Two modules opened a file manager — the jobs dialog's "Reveal" and Home's
/// recents — so neither could be switched off in one place, and the menu
/// reach test, which runs every menu item, opened a file-manager window on
/// the machine running the suite. A test may not touch the operator's
/// desktop, and the only shape that guarantees it is a capability the
/// *application* turns on, since no test runs the application.
///
/// Derived from the sources, so a spawn added tomorrow in any crate turns
/// the suite red on the run that introduces it, and a rename of the owner
/// fails loudly rather than leaving a rule that matches nothing.
#[test]
fn only_one_module_starts_another_program() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    for root in ["crates", "apps"] {
        collect_rs(&workspace.join(root), &mut files);
    }
    assert!(
        files.len() > 500,
        "found {} source files under {workspace:?}, which cannot be this workspace: \
         the rule just went vacuous",
        files.len()
    );

    let mut owners_seen = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let declared = SPAWN_OWNERS.iter().any(|(name, _)| rel == *name);
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains("Command::new") {
            continue;
        }
        if declared {
            owners_seen += 1;
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            if line.contains("Command::new") {
                offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert_eq!(
        owners_seen,
        SPAWN_OWNERS.len(),
        "{} of the {} declared spawn owners still spawn; a renamed owner leaves a \
         rule that matches nothing",
        owners_seen,
        SPAWN_OWNERS.len()
    );
    assert!(
        offenders.is_empty(),
        "starting another application belongs to aria-ui/src/app/launch.rs, which \
         refuses until the headed editor allows it ({} owner(s) declared):\n  {}",
        SPAWN_OWNERS.len(),
        offenders.join("\n  ")
    );
}
