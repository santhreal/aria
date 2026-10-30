//! Aria desktop editor: mount plugins and open the GPUI shell.

mod bootstrap;

// What a start costs, beside the file that runs one.
#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;

use std::path::PathBuf;

#[global_allocator]
static ALLOCATOR: aria_ui::Allocator = aria_ui::Allocator;

/// Where the session keeps settings, caches, and the recovery file.
///
/// `ARIA_DATA_DIR` overrides it, which is what makes a second instance on a
/// scratch profile possible: the default is one directory, and two editors
/// writing the same settings file would fight over it.
const DATA_DIR_ENV: &str = "ARIA_DATA_DIR";

/// A plugin package directory to load for development beside the installed
/// plugins, over an installed one with the same id, reloading when its files
/// change. `aria plugin dev` sets it.
const PLUGIN_DEV_ENV: &str = "ARIA_PLUGIN_DEV";

fn main() {
    bootstrap::setup_tracing();
    let ctx = aria_plugin::PluginContext::new();
    // Resolved once and handed to both: the mount reads external plugins and
    // catalog overrides from this directory and the session writes its settings,
    // caches and recovery file into it, so a scratch profile that only moved one
    // of the two would run half on the default profile.
    // A relative override is resolved here, before the mount reads it: the mount
    // and the session must name the same directory, and the storage layout under
    // it refuses a relative root outright.
    let data_dir = std::env::var_os(DATA_DIR_ENV)
        .map(PathBuf::from)
        .map_or_else(
            || Ok(aria_ui::settings::default_data_dir()),
            |dir| aria_ui::settings::absolute_data_dir(&dir),
        );
    let data_dir = match data_dir {
        Ok(dir) => dir,
        Err(reason) => {
            eprintln!("aria cannot start: {reason}");
            std::process::exit(1);
        }
    };
    // Before the mount, which reads the development package beside the installed
    // ones. Absolute, so the reload watch does not follow a later change of the
    // working directory.
    if let Some(dir) = std::env::var_os(PLUGIN_DEV_ENV).filter(|dir| !dir.is_empty()) {
        let dir = match std::path::absolute(&dir) {
            Ok(dir) => dir,
            Err(err) => {
                eprintln!(
                    "aria cannot start: {PLUGIN_DEV_ENV}={}: {err}",
                    PathBuf::from(dir).display()
                );
                std::process::exit(1);
            }
        };
        if let Err(reason) = aria_ui::plugins::installed::develop(dir) {
            eprintln!("aria cannot start: {reason}");
            std::process::exit(1);
        }
    }
    // A failure per plugin, not one abort: the editor starts with whatever mounted
    // and the Plugins section in Settings shows the same reasons.
    for failure in aria_ui::plugins::mount_all(&ctx, &data_dir) {
        eprintln!("plugin {} did not mount: {}", failure.id, failure.reason);
    }
    aria_ui::run_app_with(aria_ui::UiConfig {
        data_dir,
        ctx,
        // Everything after the binary name. No flags: a path is the only argument
        // the editor takes, and it means the same thing as dropping that file on
        // the window.
        open_paths: std::env::args_os().skip(1).map(PathBuf::from).collect(),
    });
}
