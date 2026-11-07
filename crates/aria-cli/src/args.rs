//! Command-line parsing.
//!
//! Hand-rolled, because the whole grammar is `aria <verb> [path] [--flag value]`
//! and the workspace has no argument parser to share. Two rules keep it honest:
//!
//! - **An unknown flag is a usage error, never a default.** A misspelled
//!   `--fps` that silently exported at the project rate is worse than a refusal.
//! - **A flag that takes a value fails when the value is missing or unparseable**,
//!   naming the flag.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Flags across the CLI surface that take no value (`[--flag]` in `USAGE`).
pub(crate) const VALUELESS_FLAGS: &[&str] = &[
    "no-audio", "quiet", "clips", "beats", "silence", "loudness", "shots", "no-build",
];
/// The requested operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Verb {
    /// Composite a demo scene on a headless GPU.
    Demo,
    /// Assemble a project file from a list of media.
    New,
    /// Render one timeline instant to a PNG.
    Frame,
    /// Encode a span of the timeline to a video file.
    Export,
    /// Print what a project file contains.
    Inspect,
    /// Run a detector over a media file.
    Analyze,
    /// Create, run, check, or pack a plugin package.
    Plugin,
}

impl Verb {
    /// Every verb, in the order the usage block lists them.
    ///
    /// One table, so a verb that parses is a verb the tests check for
    /// documentation and the parser finds without a second `match` to forget.
    pub(crate) const ALL: [Verb; 7] = [
        Verb::Demo,
        Verb::New,
        Verb::Frame,
        Verb::Export,
        Verb::Inspect,
        Verb::Analyze,
        Verb::Plugin,
    ];

    /// The verb spelled as it is typed.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Verb::Demo => "demo",
            Verb::New => "new",
            Verb::Frame => "frame",
            Verb::Export => "export",
            Verb::Inspect => "inspect",
            Verb::Analyze => "analyze",
            Verb::Plugin => "plugin",
        }
    }

    /// Flags accepted by this verb on the top-level command line (`Plugin`
    /// dispatches to [`crate::plugin::Subcommand::flags`]).
    pub(crate) fn flags(self) -> &'static [&'static str] {
        match self {
            Verb::Demo => crate::demo::FLAGS,
            Verb::New => crate::new::FLAGS,
            Verb::Frame => crate::frame::FLAGS,
            Verb::Export => crate::export::FLAGS,
            Verb::Inspect => crate::inspect::FLAGS,
            Verb::Analyze => crate::analyze::FLAGS,
            Verb::Plugin => &[],
        }
    }

    /// Maximum number of positional arguments accepted by fixed-arity verbs.
    /// `New` (`<out.aria> [media...]`) is variadic, and `Plugin` validates
    /// arity per sub-command.
    pub(crate) fn max_positionals(self) -> Option<usize> {
        match self {
            Verb::Demo | Verb::Inspect | Verb::Analyze => Some(1),
            Verb::Frame | Verb::Export => Some(2),
            Verb::New | Verb::Plugin => None,
        }
    }
    /// The verb spelled `word`, or `None` for a word that names no verb.
    pub(crate) fn named(word: &str) -> Option<Verb> {
        Verb::ALL.iter().copied().find(|verb| verb.key() == word)
    }

    /// The positional argument naming the file this verb writes, and the
    /// extension of the one format it writes there. `None` for a verb that
    /// writes no file named by a positional argument.
    pub(crate) fn writes(self) -> Option<(usize, &'static str)> {
        match self {
            Verb::Demo => Some((0, "png")),
            Verb::New => Some((0, aria_models::PROJECT_FILE_EXTENSION)),
            Verb::Frame => Some((1, "png")),
            Verb::Export => Some((1, aria_encoder::capability().extension)),
            Verb::Inspect | Verb::Analyze | Verb::Plugin => None,
        }
    }
}

/// A parsed command line: one verb, its positional arguments, and its flags.
#[derive(Debug)]
pub(crate) struct Args {
    pub verb: Verb,
    pub positional: Vec<String>,
    flags: BTreeMap<String, Option<String>>,
}

/// What the parser can refuse.
#[derive(Debug, PartialEq)]
pub(crate) enum ArgsError {
    /// No verb at all: print the usage block on stdout, exit 0.
    NoVerb,
    /// `--help` or `-h` anywhere: after a command, that command's `USAGE`
    /// entries ([`synopsis`]); otherwise the usage block. Exit 0.
    Help(Option<Verb>),
    /// `--version` as the first word: print the version on stdout, exit 0.
    Version,
    /// A usage mistake, exit 2.
    Usage(String),
}

impl Args {
    /// Parse `argv` without the program name.
    pub(crate) fn parse<I, S>(argv: I) -> Result<Args, ArgsError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let words: Vec<String> = argv.into_iter().map(Into::into).collect();
        if words.iter().any(|word| word == "--help" || word == "-h") {
            return Err(ArgsError::Help(
                words.first().and_then(|word| Verb::named(word)),
            ));
        }
        if words.first().is_some_and(|word| word == "--version") {
            return Err(ArgsError::Version);
        }
        let mut words = words.into_iter();
        let Some(first) = words.next() else {
            return Err(ArgsError::NoVerb);
        };
        let verb = Verb::named(&first)
            .ok_or_else(|| ArgsError::Usage(format!("unknown command \"{first}\"")))?;

        let mut positional = Vec::new();
        let mut flags: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut pending: Option<String> = None;
        for word in words {
            if let Some(name) = word.strip_prefix("--") {
                if let Some(open) = pending.take() {
                    flags.insert(open, None);
                }
                if name.is_empty() {
                    return Err(ArgsError::Usage("\"--\" is not a flag".into()));
                }
                if let Some((name, value)) = name.split_once('=') {
                    flags.insert(name.to_string(), Some(value.to_string()));
                } else if VALUELESS_FLAGS.contains(&name) {
                    flags.insert(name.to_string(), None);
                } else {
                    pending = Some(name.to_string());
                }
            } else if let Some(name) = pending.take() {
                flags.insert(name, Some(word));
            } else if word.starts_with('-') {
                // Flags are spelled `--name`. A dash word left as a positional
                // became an output path: `frame p.aria -o x.png` wrote a file
                // named `-o`.
                return Err(ArgsError::Usage(format!(
                    "\"{word}\" is not a flag: flags are spelled --name, and a path \
                     that starts with - is written ./{word}"
                )));
            } else {
                positional.push(word);
            }
        }
        if let Some(open) = pending {
            flags.insert(open, None);
        }
        Ok(Args {
            verb,
            positional,
            flags,
        })
    }

    /// The positional argument at `index`, or a usage error naming `what`.
    pub(crate) fn required_path(&self, index: usize, what: &str) -> Result<PathBuf, ArgsError> {
        self.positional
            .get(index)
            .map(PathBuf::from)
            .ok_or_else(|| ArgsError::Usage(format!("{} is required", what)))
    }

    /// Refuse any positional argument at or past `max`.
    pub(crate) fn reject_extra_positional(&self, max: usize) -> Result<(), ArgsError> {
        if let Some(extra) = self.positional.get(max) {
            return Err(ArgsError::Usage(format!("unexpected argument \"{extra}\"")));
        }
        Ok(())
    }

    /// Whether a valueless flag was given, refusing `--flag=value`.
    pub(crate) fn flag(&self, name: &str) -> Result<bool, ArgsError> {
        match self.flags.get(name) {
            None => Ok(false),
            Some(None) => Ok(true),
            Some(Some(unexpected)) => Err(ArgsError::Usage(format!(
                "--{name} takes no value, got \"{unexpected}\""
            ))),
        }
    }

    /// Whether a valueless flag was given.
    pub(crate) fn present(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    /// A flag's raw value, or `None` when the flag was not given.
    pub(crate) fn value(&self, name: &str) -> Result<Option<&str>, ArgsError> {
        match self.flags.get(name) {
            None => Ok(None),
            Some(None) => Err(ArgsError::Usage(format!("--{name} needs a value"))),
            Some(Some(value)) => Ok(Some(value.as_str())),
        }
    }

    /// A flag parsed as `f64`.
    pub(crate) fn seconds(&self, name: &str) -> Result<Option<f64>, ArgsError> {
        let Some(raw) = self.value(name)? else {
            return Ok(None);
        };
        let value = raw
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                ArgsError::Usage(format!("--{name} wants a number of seconds, got \"{raw}\""))
            })?;
        if value < 0.0 {
            return Err(ArgsError::Usage(format!("--{name} cannot be negative")));
        }
        Ok(Some(value))
    }

    /// A flag parsed as `u32`.
    pub(crate) fn number(&self, name: &str) -> Result<Option<u32>, ArgsError> {
        match self.value(name)? {
            None => Ok(None),
            Some(raw) => raw.parse::<u32>().map(Some).map_err(|_| {
                ArgsError::Usage(format!("--{name} wants a whole number, got \"{raw}\""))
            }),
        }
    }

    /// A `WIDTHxHEIGHT` flag.
    pub(crate) fn size(&self, name: &str) -> Result<Option<(u32, u32)>, ArgsError> {
        let Some(raw) = self.value(name)? else {
            return Ok(None);
        };
        let bad = || ArgsError::Usage(format!("--{name} wants WIDTHxHEIGHT, got \"{raw}\""));
        let (width, height) = raw.split_once(['x', 'X']).ok_or_else(bad)?;
        let width = width.parse::<u32>().map_err(|_| bad())?;
        let height = height.parse::<u32>().map_err(|_| bad())?;
        if width == 0 || height == 0 {
            return Err(bad());
        }
        Ok(Some((width, height)))
    }

    /// Refuse any flag this verb does not read.
    ///
    /// The check is the difference between `aria export --frames 10` writing the
    /// whole timeline and telling the caller the flag does not exist.
    pub(crate) fn reject_unknown(&self, known: &[&str]) -> Result<(), ArgsError> {
        for (name, value) in &self.flags {
            if !known.contains(&name.as_str()) {
                return Err(ArgsError::Usage(format!(
                    "{} does not take --{name}",
                    self.verb.key()
                )));
            }
            if VALUELESS_FLAGS.contains(&name.as_str())
                && let Some(unexpected) = value
            {
                return Err(ArgsError::Usage(format!(
                    "--{name} takes no value, got \"{unexpected}\""
                )));
            }
        }
        Ok(())
    }

    /// Refuse an output file named for a format other than the one this verb
    /// writes. Each verb writes one format whatever the name says, so
    /// `export p.aria out.webm` wrote MP4 into `out.webm`.
    pub(crate) fn reject_misnamed_output(&self) -> Result<(), ArgsError> {
        let Some((index, extension)) = self.verb.writes() else {
            return Ok(());
        };
        let Some(out) = self.positional.get(index) else {
            return Ok(());
        };
        let path = Path::new(out);
        if path
            .extension()
            .is_some_and(|named| named.eq_ignore_ascii_case(extension))
        {
            return Ok(());
        }
        Err(ArgsError::Usage(format!(
            "{} writes a .{extension} file; name the output \"{}\" rather than \"{out}\"",
            self.verb.key(),
            path.with_extension(extension).display()
        )))
    }

    /// Reject an output file whose directory does not exist. The write came
    /// after the work and failed with "i/o error: No such file or directory
    /// (os error 2)", naming no path; `new` had imported every file first.
    pub(crate) fn reject_missing_output_directory(&self) -> Result<(), ArgsError> {
        let Some((index, _)) = self.verb.writes() else {
            return Ok(());
        };
        let Some(out) = self.positional.get(index) else {
            return Ok(());
        };
        match Path::new(out).parent() {
            Some(dir) if !dir.as_os_str().is_empty() && !dir.is_dir() => {
                Err(ArgsError::Usage(format!(
                    "there is no directory \"{}\" to write \"{out}\" in",
                    dir.display()
                )))
            }
            _ => Ok(()),
        }
    }
}

/// The `USAGE` entries for `verb`: each synopsis line and its description,
/// every sub-command's for `plugin`, entries separated by a blank line. A
/// usage error prints these instead of the whole block.
pub(crate) fn synopsis(verb: Verb) -> String {
    let start = format!("    {} ", verb.key());
    let mut out = String::new();
    let mut keep = false;
    for line in USAGE.lines() {
        let indent = line.len() - line.trim_start().len();
        if !line.is_empty() && indent == 0 {
            keep = false;
        } else if indent == 4 {
            keep = line.starts_with(&start);
            if keep && !out.is_empty() {
                out.push('\n');
            }
        }
        if keep && !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The usage block, printed for no arguments and `--help`.
pub(crate) const USAGE: &str = "\
aria — the Aria editor's command line

USAGE
    aria <command> [arguments]
    aria --help | --version
    aria <command> --help

COMMANDS
    demo [out.png]
        Composite a text layer on a headless GPU and write the canvas.

    new <out.aria> [media...] [--fps 23.976|24|25|29.97|30|50|60]
        Assemble a project: import each file and lay it after the previous
        one, video on V1 and audio-only files on A1.

    frame <project.aria> [out.png] [--at SECONDS] [--fit WxH]
        Render one timeline instant. --at defaults to 0, --fit scales the
        canvas down to fit a box (never up).

    export <project.aria> <out.mp4> [--at SECONDS] [--to SECONDS]
                                   [--size WxH] [--fps N] [--codec h264|h265]
                                   [--bitrate KBPS] [--no-audio] [--quiet]
        Encode a span of the timeline. Defaults to the whole timeline at the
        project's canvas size, rate, and audio.

    inspect <project.aria> [--clips]
        Print the project's canvas, rate, duration, media, and lanes.
        --clips lists every clip on every lane.

    analyze <media> [--beats] [--silence] [--loudness] [--shots] [--limit N]
        Run detectors over a media file and print what they found. With no
        detector flag, every audio detector runs.

    plugin new <id> [--tier wasm|native] [--dir PARENT] [--sdk PATH]
        Create a plugin package with a manifest, a sample effect, and a
        sample command.

    plugin dev <dir> [--editor PATH] [--data-dir PATH]
        Build and stage the plugin, start Aria with it loaded, and rebuild
        when its sources change.

    plugin check <dir|file.ariaplugin>
        Validate the manifest, shaders, permissions, and ABI or WIT world
        without starting Aria.

    plugin pack <dir> [--out FILE] [--no-build]
        Build, check, and write a .ariaplugin archive.

EXIT CODES
    0  success
    1  the command failed
    2  the command line was wrong
";

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
