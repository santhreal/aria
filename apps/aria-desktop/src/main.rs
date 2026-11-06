//! Aria desktop editor: mount plugins and open the GPUI shell.

mod bootstrap;

// What a start costs, beside the file that runs one.
#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;

use std::ffi::OsString;
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

/// The `--help` text.
const USAGE: &str = "\
usage: aria-desktop [path...]

Start the Aria editor. Each path opens a .aria project or imports a media or
timeline file, as dropping it on the window does. `aria --help` lists the
command-line tools.
";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Launch {
    /// Start the editor and open these paths.
    Open(Vec<PathBuf>),
    /// Print `text` and exit with `code`: on stdout for 0, on stderr otherwise.
    Exit { text: String, code: i32 },
}

/// Read the arguments after the binary name. A path is the only argument the
/// editor takes, and a word with a leading dash is not one: `aria-desktop
/// --help` started the editor and reported a missing file named `--help`.
fn launch(args: impl IntoIterator<Item = OsString>) -> Launch {
    let args: Vec<OsString> = args.into_iter().collect();
    if args.iter().any(|word| word == "--help" || word == "-h") {
        return Launch::Exit {
            text: USAGE.to_owned(),
            code: 0,
        };
    }
    if args.first().is_some_and(|word| word == "--version") {
        return Launch::Exit {
            text: format!("aria-desktop {}\n", env!("CARGO_PKG_VERSION")),
            code: 0,
        };
    }
    if let Some(word) = args
        .iter()
        .find(|word| word.as_encoded_bytes().first() == Some(&b'-'))
    {
        let word = word.to_string_lossy();
        return Launch::Exit {
            text: format!(
                "aria-desktop: \"{word}\" is not a flag: aria-desktop takes paths only, and a \
                 path that starts with - is written ./{word}\n\n{USAGE}"
            ),
            code: 2,
        };
    }
    Launch::Open(args.into_iter().map(PathBuf::from).collect())
}

fn main() {
    let open_paths = match launch(std::env::args_os().skip(1)) {
        Launch::Open(paths) => paths,
        Launch::Exit { text, code } => {
            if code == 0 {
                print!("{text}");
            } else {
                eprint!("{text}");
            }
            std::process::exit(code);
        }
    };
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
        // Everything after the binary name, each the same as dropping that
        // file on the window.
        open_paths,
    });
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// WHY: every argument was a path to open, so `aria-desktop --help` and
    /// `aria-desktop --version` started the editor and reported a missing
    /// file. Help and the version print and exit 0, any other dash word is a
    /// usage error (exit 2) before a window opens, and paths, including
    /// `./-name`, still open in order. Not caught: a path that names a missing
    /// file, which the editor reports.
    #[test]
    fn help_version_and_dash_words_exit_and_paths_open() {
        for args in [&["--help"][..], &["-h"], &["cut.aria", "--help"]] {
            assert_eq!(
                launch(words(args)),
                Launch::Exit {
                    text: USAGE.to_owned(),
                    code: 0
                },
                "{args:?}"
            );
        }
        assert_eq!(
            launch(words(&["--version"])),
            Launch::Exit {
                text: format!("aria-desktop {}\n", env!("CARGO_PKG_VERSION")),
                code: 0
            }
        );
        for (args, word) in [
            (&["cut.aria", "--version"][..], "--version"),
            (&["-o"], "-o"),
            (&["cut.aria", "-"], "-"),
            (&["--new-window"], "--new-window"),
        ] {
            match launch(words(args)) {
                Launch::Exit { text, code: 2 } => assert!(
                    text.starts_with(&format!(
                        "aria-desktop: \"{word}\" is not a flag: aria-desktop takes paths only, \
                         and a path that starts with - is written ./{word}\n"
                    )),
                    "{args:?}: {text}"
                ),
                other => panic!("{args:?}: {other:?}"),
            }
        }
        assert_eq!(
            launch(words(&["cut.aria", "./-take.mp4", "b.mp4"])),
            Launch::Open(vec![
                PathBuf::from("cut.aria"),
                PathBuf::from("./-take.mp4"),
                PathBuf::from("b.mp4"),
            ])
        );
        assert_eq!(launch(words(&[])), Launch::Open(Vec::new()));
    }
}
