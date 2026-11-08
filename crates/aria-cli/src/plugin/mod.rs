//! `aria plugin` — create, run, check, and pack external plugin packages.

pub(crate) mod build;
pub(crate) mod check;
pub(crate) mod dev;
pub(crate) mod new;
pub(crate) mod pack;
pub(crate) mod process;

#[cfg(test)]
#[path = "path_tests.rs"]
mod path_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::args::Args;
use crate::fail::Failure;

/// Fail unless `path` is a directory, stating a missing path apart from one
/// that is something else; `what` is what the sub-command reads at `path`.
/// `check` read a missing directory as one with "no plugin.toml", a file as
/// "plugin.toml: Not a directory (os error 20)", and `dev` printed the OS
/// error with its code.
pub(crate) fn require_dir(path: &Path, what: &str) -> Result<(), Failure> {
    if path.is_dir() {
        return Ok(());
    }
    let why = if path.exists() {
        format!("not {what}")
    } else {
        "no such file or directory".to_owned()
    };
    Err(Failure::Failed(format!("{}: {why}", path.display())))
}

/// Sub-commands under `aria plugin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subcommand {
    /// Create a new plugin source package directory.
    New,
    /// Build, stage, launch the editor with `ARIA_PLUGIN_DEV`, and watch sources.
    Dev,
    /// Validate a package directory or `.ariaplugin` archive.
    Check,
    /// Build, check, and pack a `.ariaplugin` archive.
    Pack,
}

impl Subcommand {
    /// Every plugin sub-command, in usage order.
    pub(crate) const ALL: [Subcommand; 4] = [
        Subcommand::New,
        Subcommand::Dev,
        Subcommand::Check,
        Subcommand::Pack,
    ];

    /// The sub-command spelled as it is typed on the command line.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Subcommand::New => "new",
            Subcommand::Dev => "dev",
            Subcommand::Check => "check",
            Subcommand::Pack => "pack",
        }
    }

    /// The verb and sub-command as typed, such as `plugin check`.
    pub(crate) fn command(self) -> &'static str {
        match self {
            Subcommand::New => "plugin new",
            Subcommand::Dev => "plugin dev",
            Subcommand::Check => "plugin check",
            Subcommand::Pack => "plugin pack",
        }
    }

    /// Flags accepted by this sub-command.
    pub(crate) fn flags(self) -> &'static [&'static str] {
        match self {
            Subcommand::New => new::FLAGS,
            Subcommand::Dev => dev::FLAGS,
            Subcommand::Check => check::FLAGS,
            Subcommand::Pack => pack::FLAGS,
        }
    }

    /// Parse a sub-command word, or return `None`.
    pub(crate) fn parse(word: &str) -> Option<Subcommand> {
        Subcommand::ALL
            .iter()
            .copied()
            .find(|sub| sub.key() == word)
    }
}

/// Dispatch `aria plugin <new|dev|check|pack> ...`.
pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let Some(raw) = args.positional.first() else {
        return Err(Failure::Usage(
            "a plugin sub-command is required (new, dev, check, pack)".into(),
        ));
    };
    let sub = Subcommand::parse(raw).ok_or_else(|| {
        Failure::Usage(format!(
            "unknown plugin sub-command \"{raw}\" (expected new, dev, check, or pack)"
        ))
    })?;
    args.reject_unknown_in(sub.command(), sub.flags())?;
    match sub {
        Subcommand::New => new::run(args),
        Subcommand::Dev => dev::run(args),
        Subcommand::Check => check::run(args),
        Subcommand::Pack => pack::run(args),
    }
}
