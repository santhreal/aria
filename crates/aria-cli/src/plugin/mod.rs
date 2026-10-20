//! `aria plugin` — create, run, check, and pack external plugin packages.

pub(crate) mod build;
pub(crate) mod check;
pub(crate) mod dev;
pub(crate) mod new;
pub(crate) mod pack;
pub(crate) mod process;

#[cfg(test)]
mod tests;

use crate::args::Args;
use crate::fail::Failure;

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
    match sub {
        Subcommand::New => {
            args.reject_unknown(new::FLAGS)?;
            new::run(args)
        }
        Subcommand::Dev => {
            args.reject_unknown(dev::FLAGS)?;
            dev::run(args)
        }
        Subcommand::Check => {
            args.reject_unknown(check::FLAGS)?;
            check::run(args)
        }
        Subcommand::Pack => {
            args.reject_unknown(pack::FLAGS)?;
            pack::run(args)
        }
    }
}
