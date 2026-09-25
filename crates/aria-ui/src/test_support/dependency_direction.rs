//! Dependencies run from the UI toward the plugin boundary, never back.

use std::path::Path;

use super::collect_rs;

/// The files allowed to name a panel module, and why.
///
/// Registration is the one direction that has to know the panels exist:
/// something must hand the shipped `render` functions to the registries a
/// plugin also adds to. Everything else asks a registry.
const PANEL_REGISTRARS: &[(&str, &str)] = &[(
    "crates/aria-ui/src/plugins/ui.rs",
    "the mount: it hands every shipped panel, overlay and command to the hosts",
)];

/// **The dependency runs one way: the UI reaches the plugin boundary, and
/// only registration reaches a panel.**
///
/// The measured instance was `loaders_progress.rs` and `shell_route.rs`
/// reaching into `crate::panels::timeline::glide` for the eased-value store,
/// which made a loader depend on the timeline: a panel could not be removed
/// without breaking a loader, and the shell named nine dialog modules by
/// hand, so a plugin's dialog had nowhere to appear. The stores moved to
/// their owners (`motion::glide`, `crate::pending`, `crate::drag_hint`,
/// `crate::commands`, `app::recents`) and the registries to
/// `crate::plugins`, so the shell now asks by location and by condition.
///
/// Derived from the sources, and fails by default: a new file that names a
/// panel module turns this red on the run that adds it, and a registrar that
/// stops registering turns it red too, so the allowlist cannot rot into an
/// exemption for a file that does nothing.
#[test]
fn dependency_direction_runs_from_the_ui_to_the_plugin_boundary() {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate sits two levels inside the workspace")
        .to_path_buf();
    let mut files = Vec::new();
    collect_rs(&crate_src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {crate_src:?}, which cannot be this crate: the rule just \
         went vacuous",
        files.len()
    );

    let mut registrars_seen = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // A panel naming a panel is the allowed direction, and this file
        // spells the path it refuses, so it would match itself.
        if rel.contains("/panels/") || rel.starts_with("crates/aria-ui/src/test_support") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let names_panel = |line: &str| {
            let code = line.trim_start();
            !code.starts_with("//") && code.contains("crate::panels::")
        };
        if !text.lines().any(names_panel) {
            continue;
        }
        if PANEL_REGISTRARS.iter().any(|(name, _)| rel == *name) {
            registrars_seen += 1;
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            if names_panel(line) {
                offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert_eq!(
        registrars_seen,
        PANEL_REGISTRARS.len(),
        "{} of the {} declared registrars still name a panel; a mount that stopped \
         registering leaves a rule that matches nothing",
        registrars_seen,
        PANEL_REGISTRARS.len()
    );
    assert!(
        offenders.is_empty(),
        "a file outside panels/ names a panel module: ask a registry instead — panels \
         resolve through crate::plugins::panel_host, dialogs and overlays through \
         crate::plugins::overlay_host, commands through crate::commands, and shared \
         state through its own owner ({} registrar(s) declared):\n  {}",
        PANEL_REGISTRARS.len(),
        offenders.join("\n  ")
    );
}
