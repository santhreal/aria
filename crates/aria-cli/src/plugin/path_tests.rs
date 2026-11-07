//! WHY: `aria plugin check` read a missing directory as a package with "no
//! plugin.toml" and a plain file as "plugin.toml: Not a directory (os error
//! 20)", `pack` called a missing directory "not a directory", and `dev`
//! printed the OS error with its code. Every sub-command that reads a package
//! directory states a missing path and a path that is not a directory apart,
//! before it builds or opens anything.
//!
//! Not caught: a directory that exists but cannot be read; the package open
//! reports that one.

use super::Subcommand;
use crate::args::Args;
use crate::fail::Failure;

/// What `sub` reads at its first path argument, or `None` when it reads no
/// package directory. An exhaustive match, so a new sub-command does not
/// compile until it states which.
fn reads(sub: Subcommand) -> Option<&'static str> {
    match sub {
        Subcommand::New => None,
        Subcommand::Check => Some("a plugin directory or .ariaplugin archive"),
        Subcommand::Dev | Subcommand::Pack => Some("a plugin directory"),
    }
}

#[test]
fn a_missing_path_and_a_file_are_stated_apart_for_every_package_reader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("absent");
    let file = dir.path().join("notes.txt");
    std::fs::write(&file, b"notes").expect("write a file");
    let mut readers = 0;
    for sub in Subcommand::ALL {
        let Some(what) = reads(sub) else {
            continue;
        };
        readers += 1;
        for (path, why) in [
            (&missing, "no such file or directory".to_owned()),
            (&file, format!("not {what}")),
        ] {
            let mut words = vec![
                "plugin".to_owned(),
                sub.key().to_owned(),
                path.display().to_string(),
            ];
            if sub == Subcommand::Pack {
                words.push("--no-build".to_owned());
            }
            let args = Args::parse(words).expect("parse");
            assert_eq!(
                super::run(&args),
                Err(Failure::Failed(format!("{}: {why}", path.display()))),
                "plugin {} {}",
                sub.key(),
                path.display()
            );
        }
    }
    assert_eq!(readers, 3, "the sub-commands that read a package directory");
}
