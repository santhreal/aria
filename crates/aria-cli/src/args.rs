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
use std::path::PathBuf;

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
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
}

impl Verb {
    /// Every verb, in the order the usage block lists them.
    ///
    /// One table, so a verb that parses is a verb the tests check for
    /// documentation and the parser finds without a second `match` to forget.
    pub const ALL: [Verb; 6] = [
        Verb::Demo,
        Verb::New,
        Verb::Frame,
        Verb::Export,
        Verb::Inspect,
        Verb::Analyze,
    ];

    /// The verb spelled as it is typed.
    pub fn key(self) -> &'static str {
        match self {
            Verb::Demo => "demo",
            Verb::New => "new",
            Verb::Frame => "frame",
            Verb::Export => "export",
            Verb::Inspect => "inspect",
            Verb::Analyze => "analyze",
        }
    }

    fn parse(word: &str) -> Option<Verb> {
        Verb::ALL.iter().copied().find(|verb| verb.key() == word)
    }
}

/// A parsed command line: one verb, its positional arguments, and its flags.
#[derive(Debug)]
pub struct Args {
    pub verb: Verb,
    pub positional: Vec<String>,
    flags: BTreeMap<String, Option<String>>,
}

/// What the parser can refuse.
#[derive(Debug, PartialEq)]
pub enum ArgsError {
    /// No verb at all: print the usage block on stdout, exit 0.
    NoVerb,
    /// `--help` anywhere: same block, exit 0.
    Help,
    /// A usage mistake, exit 2.
    Usage(String),
}

impl Args {
    /// Parse `argv` without the program name.
    pub fn parse<I, S>(argv: I) -> Result<Args, ArgsError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let words: Vec<String> = argv.into_iter().map(Into::into).collect();
        if words.iter().any(|word| word == "--help" || word == "-h") {
            return Err(ArgsError::Help);
        }
        let mut words = words.into_iter();
        let Some(first) = words.next() else {
            return Err(ArgsError::NoVerb);
        };
        let verb = Verb::parse(&first)
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
                } else {
                    pending = Some(name.to_string());
                }
            } else if let Some(name) = pending.take() {
                flags.insert(name, Some(word));
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
    pub fn required_path(&self, index: usize, what: &str) -> Result<PathBuf, ArgsError> {
        self.positional
            .get(index)
            .map(PathBuf::from)
            .ok_or_else(|| ArgsError::Usage(format!("{} is required", what)))
    }

    /// Whether a valueless flag was given.
    pub fn present(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    /// A flag's raw value, or `None` when the flag was not given.
    pub fn value(&self, name: &str) -> Result<Option<&str>, ArgsError> {
        match self.flags.get(name) {
            None => Ok(None),
            Some(None) => Err(ArgsError::Usage(format!("--{name} needs a value"))),
            Some(Some(value)) => Ok(Some(value.as_str())),
        }
    }

    /// A flag parsed as `f64`.
    pub fn seconds(&self, name: &str) -> Result<Option<f64>, ArgsError> {
        match self.value(name)? {
            None => Ok(None),
            Some(raw) => raw
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Some)
                .ok_or_else(|| {
                    ArgsError::Usage(format!("--{name} wants a number of seconds, got \"{raw}\""))
                }),
        }
    }

    /// A flag parsed as `u32`.
    pub fn number(&self, name: &str) -> Result<Option<u32>, ArgsError> {
        match self.value(name)? {
            None => Ok(None),
            Some(raw) => raw.parse::<u32>().map(Some).map_err(|_| {
                ArgsError::Usage(format!("--{name} wants a whole number, got \"{raw}\""))
            }),
        }
    }

    /// A `WIDTHxHEIGHT` flag.
    pub fn size(&self, name: &str) -> Result<Option<(u32, u32)>, ArgsError> {
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
    pub fn reject_unknown(&self, known: &[&str]) -> Result<(), ArgsError> {
        for name in self.flags.keys() {
            if !known.contains(&name.as_str()) {
                return Err(ArgsError::Usage(format!(
                    "{} does not take --{name}",
                    self.verb.key()
                )));
            }
        }
        Ok(())
    }
}

/// The usage block, printed for no arguments, `--help`, and every usage error.
pub const USAGE: &str = "\
aria — the Aria editor's command line

USAGE
    aria <command> [arguments]

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
                                   [--bitrate KBPS] [--no-audio]
        Encode a span of the timeline. Defaults to the whole timeline at the
        project's canvas size, rate, and audio.

    inspect <project.aria> [--clips]
        Print the project's canvas, rate, duration, media, and lanes.
        --clips lists every clip on every lane.

    analyze <media> [--beats] [--silence] [--loudness] [--shots]
        Run detectors over a media file and print what they found. With no
        detector flag, every audio detector runs.

EXIT CODES
    0  success
    1  the command failed
    2  the command line was wrong
";

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "a flag the verb does not read is silently
    /// ignored". Every verb rejects what it does not know, and a value flag
    /// without a value is a refusal rather than a default.
    #[test]
    fn a_flag_the_verb_does_not_read_is_refused_by_name() {
        let args = Args::parse(["export", "a.aria", "b.mp4", "--frames", "10"]).unwrap();
        let err = args
            .reject_unknown(&["at", "to", "fps"])
            .expect_err("an unknown flag was accepted");
        assert_eq!(
            err,
            ArgsError::Usage("export does not take --frames".into())
        );

        let args = Args::parse(["frame", "a.aria", "--at"]).unwrap();
        assert_eq!(
            args.seconds("at"),
            Err(ArgsError::Usage("--at needs a value".into()))
        );
    }

    /// Both flag spellings mean the same thing, and positionals keep their order.
    #[test]
    fn flags_parse_either_spelling_and_positionals_keep_order() {
        let args = Args::parse([
            "export", "in.aria", "out.mp4", "--fps=30", "--size", "640x480",
        ])
        .unwrap();
        assert_eq!(args.verb, Verb::Export);
        assert_eq!(args.positional, vec!["in.aria", "out.mp4"]);
        assert_eq!(args.number("fps").unwrap(), Some(30));
        assert_eq!(args.size("size").unwrap(), Some((640, 480)));
        assert!(args.present("size"));
        assert!(!args.present("codec"));
        assert_eq!(args.value("codec").unwrap(), None);
    }

    /// A malformed value names the flag and what it wanted, so the message is
    /// actionable without reading the usage block.
    #[test]
    fn a_malformed_value_names_the_flag_and_the_shape() {
        let args = Args::parse(["export", "a.aria", "b.mp4", "--size", "1080"]).unwrap();
        let err = args.size("size").expect_err("1080 parsed as a size");
        assert_eq!(
            err,
            ArgsError::Usage("--size wants WIDTHxHEIGHT, got \"1080\"".into())
        );

        let args = Args::parse(["frame", "a.aria", "--at", "soon"]).unwrap();
        let err = args.seconds("at").expect_err("\"soon\" parsed as seconds");
        assert!(matches!(err, ArgsError::Usage(message) if message.contains("--at")));

        let args = Args::parse(["export", "a.aria", "b.mp4", "--size", "0x480"]).unwrap();
        assert!(args.size("size").is_err(), "a zero width was accepted");
        let args = Args::parse(["export", "a.aria", "b.mp4", "--size", "640x0"]).unwrap();
        assert!(args.size("size").is_err(), "a zero height was accepted");
        let args = Args::parse(["frame", "a.aria", "--fit", "0x0"]).unwrap();
        assert!(args.size("fit").is_err(), "0x0 fit was accepted");
    }

    /// No verb and `--help` are not failures: both print usage and exit 0.
    #[test]
    fn no_verb_and_help_are_not_errors() {
        assert_eq!(
            Args::parse::<[&str; 0], &str>([]).err(),
            Some(ArgsError::NoVerb)
        );
        assert_eq!(Args::parse(["--help"]).err(), Some(ArgsError::Help));
        assert_eq!(Args::parse(["export", "-h"]).err(), Some(ArgsError::Help));
        assert_eq!(
            Args::parse(["render"]).err(),
            Some(ArgsError::Usage("unknown command \"render\"".into()))
        );
    }

    /// Every verb the usage block documents parses, and every parsed verb is
    /// documented — a new verb that nobody can discover is the same as no verb.
    #[test]
    fn every_verb_is_documented() {
        for verb in Verb::ALL {
            let key = verb.key();
            assert_eq!(
                Args::parse([key]).map(|args| args.verb).ok(),
                Some(verb),
                "{key} does not parse"
            );
            assert!(
                USAGE.contains(&format!("    {key} ")) || USAGE.contains(&format!("    {key}\n")),
                "{key} is undocumented"
            );
        }
    }
}
