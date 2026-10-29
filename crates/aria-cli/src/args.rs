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

    fn parse(word: &str) -> Option<Verb> {
        Verb::ALL.iter().copied().find(|verb| verb.key() == word)
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
    /// `--help` anywhere: same block, exit 0.
    Help,
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
    pub(crate) fn required_path(&self, index: usize, what: &str) -> Result<PathBuf, ArgsError> {
        self.positional
            .get(index)
            .map(PathBuf::from)
            .ok_or_else(|| ArgsError::Usage(format!("{} is required", what)))
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
pub(crate) const USAGE: &str = "\
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
    /// The class closed here is "a verb dispatched without its flag check".
    /// `run` checks every verb's flags against [`Verb::flags`] before the verb
    /// reads its arguments, so an unknown flag is the failure even when the
    /// positionals are wrong too. A verb that skipped the check would reach its
    /// own argument handling and fail on the missing directory instead.
    /// `aria plugin` checks its sub-command's flags, which `plugin::tests`
    /// covers; the documentation test pins which flags each verb maps to.
    #[test]
    fn run_refuses_an_unknown_flag_for_every_verb() {
        for verb in Verb::ALL {
            if verb == Verb::Plugin {
                continue;
            }
            let key = verb.key();
            let args = Args::parse([key, "no-such-dir/target", "--bogus", "1"]).unwrap();
            assert_eq!(
                crate::run(&args),
                Err(crate::fail::Failure::Usage(format!(
                    "{key} does not take --bogus"
                ))),
                "verb {key}"
            );
        }
    }

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

    /// Every verb and flag the usage block documents parses, and every parsed
    /// verb, sub-command, and flag is documented in both `USAGE` and
    /// `docs/command-line.md`.
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

    fn extract_flag_tokens(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for token in text.split(|c: char| {
            c.is_whitespace() || matches!(c, '[' | ']' | '|' | '`' | ',' | '(' | ')' | ':' | ';')
        }) {
            if let Some(name) = token.strip_prefix("--")
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !out.iter().any(|existing| existing == name)
            {
                out.push(name.to_owned());
            }
        }
        out
    }

    fn doc_section<'a>(doc: &'a str, heading: &str) -> &'a str {
        let marker = format!("## {heading}\n");
        let start = doc
            .find(&marker)
            .unwrap_or_else(|| panic!("missing section `## {heading}` in docs/command-line.md"));
        let rest = &doc[start + marker.len()..];
        match rest.find("\n## ") {
            Some(end) => &rest[..end],
            None => rest,
        }
    }

    fn doc_subsection<'a>(section: &'a str, heading: &str) -> &'a str {
        let marker = format!("### {heading}\n");
        let start = section.find(&marker).unwrap_or_else(|| {
            panic!("missing subsection `### {heading}` in docs/command-line.md")
        });
        let rest = &section[start + marker.len()..];
        match rest.find("\n### ") {
            Some(end) => &rest[..end],
            None => rest,
        }
    }

    fn first_column_cells(section: &str) -> Vec<String> {
        let mut cells = Vec::new();
        let mut in_fence = false;
        for line in section.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence || !trimmed.starts_with('|') {
                continue;
            }
            let row: Vec<&str> = trimmed
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if row.is_empty()
                || row
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
                || matches!(row[0], "Code" | "Flag" | "Variable")
            {
                continue;
            }
            cells.push(row[0].trim_matches('`').to_owned());
        }
        cells
    }

    fn first_fence(section: &str) -> &str {
        let (_, after_open) = section
            .split_once("```\n")
            .expect("section must start with a synopsis code fence");
        let (fence, _) = after_open
            .split_once("\n```")
            .expect("synopsis code fence must close");
        fence
    }

    /// Closes the defect class where a CLI verb, `aria plugin` sub-command,
    /// flag, rate, codec, lane flag, detector limit, check step, archive source
    /// exclusion, exit code, or `aria-desktop` environment variable is added,
    /// renamed, or removed in code while `USAGE` or `docs/command-line.md`
    /// retains the old surface or omits the new one.
    #[test]
    fn command_line_doc_and_usage_match_cli_registries_bidirectionally() {
        let command_line_doc_owned = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/command-line.md"),
        )
        .expect("read docs/command-line.md");
        #[allow(non_snake_case)]
        let COMMAND_LINE_DOC: &str = &command_line_doc_owned;
        // 1. Top-level H2 sections in `docs/command-line.md` must be Exit codes,
        // every verb in `Verb::ALL`, and the `aria-desktop` section.
        let mut h2_sections = Vec::new();
        let mut in_fence = false;
        for line in COMMAND_LINE_DOC.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if !in_fence && let Some(title) = line.strip_prefix("## ") {
                h2_sections.push(title.trim().to_owned());
            }
        }
        let mut expected_h2 = vec!["Exit codes".to_owned()];
        for verb in [
            Verb::New,
            Verb::Frame,
            Verb::Export,
            Verb::Inspect,
            Verb::Analyze,
            Verb::Demo,
            Verb::Plugin,
        ] {
            expected_h2.push(verb.key().to_owned());
        }
        expected_h2.push("Desktop binary (`aria-desktop`)".to_owned());
        assert_eq!(
            h2_sections, expected_h2,
            "H2 sections in docs/command-line.md drifted from Verb::ALL"
        );
        let verb_set: std::collections::BTreeSet<&str> =
            Verb::ALL.iter().map(|v| v.key()).collect();
        let h2_verb_set: std::collections::BTreeSet<&str> = h2_sections[1..h2_sections.len() - 1]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            h2_verb_set, verb_set,
            "H2 verb sections in docs/command-line.md must match Verb::ALL"
        );

        // 2. Exit codes table in `docs/command-line.md`.
        let exit_sec = doc_section(COMMAND_LINE_DOC, "Exit codes");
        assert_eq!(
            first_column_cells(exit_sec),
            vec!["0".to_owned(), "1".to_owned(), "2".to_owned()]
        );
        assert!(exit_sec.contains("`--help`") && exit_sec.contains("`-h`"));

        // 3. Every non-plugin verb's flags in `USAGE` and `docs/command-line.md`
        // must match `verb.flags()` in both directions.
        for verb in Verb::ALL {
            if verb == Verb::Plugin {
                continue;
            }
            let key = verb.key();
            let sec = doc_section(COMMAND_LINE_DOC, key);
            let fence = first_fence(sec);
            let fence_flags = extract_flag_tokens(fence);
            let table_flags: Vec<String> = first_column_cells(sec)
                .into_iter()
                .map(|cell| {
                    cell.split_whitespace()
                        .next()
                        .unwrap_or("")
                        .strip_prefix("--")
                        .unwrap_or("")
                        .to_owned()
                })
                .collect();
            let expected_flags: Vec<String> =
                verb.flags().iter().map(|flag| (*flag).to_owned()).collect();
            assert_eq!(
                fence_flags, expected_flags,
                "synopsis flags for `{key}` in docs/command-line.md drifted from {key}::FLAGS"
            );
            assert_eq!(
                table_flags, expected_flags,
                "flag table for `{key}` in docs/command-line.md drifted from {key}::FLAGS"
            );

            // Verify `USAGE` block entry for this verb.
            let usage_marker = format!("    {key}");
            let usage_start = USAGE.find(&usage_marker).expect("verb in USAGE");
            let usage_rest = &USAGE[usage_start..];
            let usage_block = usage_rest.split("\n\n").next().unwrap_or(usage_rest);
            // Only extract flags from the synopsis lines (before the 8-space indented description).
            let usage_synopsis: String = usage_block
                .lines()
                .take_while(|line| {
                    line.starts_with(&usage_marker) || line.starts_with("          ")
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(
                extract_flag_tokens(&usage_synopsis),
                expected_flags,
                "USAGE synopsis flags for `{key}` drifted from {key}::FLAGS"
            );
        }

        // 4. Verb-specific registries and constants in `docs/command-line.md`.
        let new_sec = doc_section(COMMAND_LINE_DOC, "new");
        let new_fence = first_fence(new_sec);
        let rate_list = new_fence
            .split_once("--fps ")
            .and_then(|(_, rest)| rest.strip_suffix(']'))
            .expect("--fps rate list in `new` synopsis");
        let doc_rates: Vec<&str> = rate_list.split('|').collect();
        let code_rates: Vec<&str> = crate::new::RATES.iter().map(|(k, _)| *k).collect();
        assert_eq!(doc_rates, code_rates, "`aria new --fps` rates drifted");

        let frame_sec = doc_section(COMMAND_LINE_DOC, "frame");
        assert!(
            frame_sec.contains(&format!("`{}`", crate::frame::DEFAULT_OUT)),
            "`frame` section must document default output {}",
            crate::frame::DEFAULT_OUT
        );

        let export_sec = doc_section(COMMAND_LINE_DOC, "export");
        let export_fence = first_fence(export_sec);
        let codec_list = export_fence
            .split_once("--codec ")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(codecs, _)| codecs)
            .expect("--codec list in `export` synopsis");
        let doc_codecs: Vec<&str> = codec_list.split('|').collect();
        let code_codecs: Vec<&str> = aria_core::VideoCodec::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(
            doc_codecs, code_codecs,
            "`aria export --codec` keys drifted"
        );

        let inspect_sec = doc_section(COMMAND_LINE_DOC, "inspect");
        let (_, lane_line) = inspect_sec
            .split_once("Lane state flags appear in brackets:")
            .expect("lane state flags sentence in `inspect` section");
        let mut doc_lane_flags = Vec::new();
        let mut rem = lane_line;
        while let Some((_, after_open)) = rem.split_once('`') {
            let Some((flag, tail)) = after_open.split_once('`') else {
                break;
            };
            doc_lane_flags.push(flag);
            rem = tail;
        }
        assert_eq!(
            doc_lane_flags,
            crate::inspect::LANE_FLAGS.as_slice(),
            "`aria inspect` lane flags drifted"
        );

        let analyze_sec = doc_section(COMMAND_LINE_DOC, "analyze");
        assert!(
            analyze_sec.contains(&format!("`{}`", crate::analyze::DEFAULT_LIMIT)),
            "`analyze` section must document DEFAULT_LIMIT"
        );
        assert!(
            analyze_sec.contains(&format!("`{}` fps", aria_detect::SHOT_SAMPLE_FPS))
                && analyze_sec.contains(&format!(
                    "`{}` minutes (`{}`\nseconds)",
                    (aria_detect::MAX_SHOT_SECONDS as u64) / 60,
                    aria_detect::MAX_SHOT_SECONDS as u64
                ))
                && analyze_sec.contains(&format!("`{}` Hz", aria_detect::ANALYSIS_SAMPLE_RATE)),
            "`analyze` section constants drifted from aria_detect"
        );

        let demo_sec = doc_section(COMMAND_LINE_DOC, "demo");
        assert!(
            demo_sec.contains(&format!("`{}`", crate::demo::DEFAULT_OUT))
                && demo_sec.contains(&format!("`{}x{}`", crate::demo::WIDTH, crate::demo::HEIGHT)),
            "`demo` section constants drifted from demo.rs"
        );

        // 5. `aria plugin` sub-commands and flags in `USAGE` and `docs/command-line.md`.
        let plugin_sec = doc_section(COMMAND_LINE_DOC, "plugin");
        let plugin_fence = first_fence(plugin_sec);
        let mut h3_subs = Vec::new();
        for line in plugin_sec.lines() {
            if let Some(title) = line.strip_prefix("### ") {
                h3_subs.push(title.trim().to_owned());
            }
        }
        let expected_h3: Vec<String> = crate::plugin::Subcommand::ALL
            .iter()
            .map(|sub| format!("plugin {}", sub.key()))
            .collect();
        assert_eq!(
            h3_subs, expected_h3,
            "`### plugin <sub>` subsections drifted from Subcommand::ALL"
        );

        for sub in crate::plugin::Subcommand::ALL {
            let key = sub.key();
            let sub_sec = doc_subsection(plugin_sec, &format!("plugin {key}"));
            let table_flags: Vec<String> = first_column_cells(sub_sec)
                .into_iter()
                .map(|cell| {
                    cell.split_whitespace()
                        .next()
                        .unwrap_or("")
                        .strip_prefix("--")
                        .unwrap_or("")
                        .to_owned()
                })
                .collect();
            let expected_flags: Vec<String> =
                sub.flags().iter().map(|flag| (*flag).to_owned()).collect();
            assert_eq!(
                table_flags, expected_flags,
                "flag table for `plugin {key}` drifted from plugin::{key}::FLAGS"
            );

            let fence_line = plugin_fence
                .lines()
                .find(|line| line.starts_with(&format!("aria plugin {key} ")))
                .unwrap_or_else(|| panic!("missing `aria plugin {key}` line in plugin synopsis"));
            assert_eq!(
                extract_flag_tokens(fence_line),
                expected_flags,
                "synopsis flags for `aria plugin {key}` drifted"
            );

            let usage_line = USAGE
                .lines()
                .find(|line| line.starts_with(&format!("    plugin {key} ")))
                .unwrap_or_else(|| panic!("missing `plugin {key}` in USAGE"));
            assert_eq!(
                extract_flag_tokens(usage_line),
                expected_flags,
                "USAGE flags for `plugin {key}` drifted"
            );
        }

        // 6. `plugin new`, `plugin dev`, `plugin check`, and `plugin pack` specifics.
        let p_new_sec = doc_subsection(plugin_sec, "plugin new");
        let (_, after_tier) = p_new_sec
            .split_once("NewTier::ALL` in `crates/aria-cli/src/plugin/new.rs`): ")
            .expect("NewTier::ALL cell in `plugin new` table");
        let (tier_cell, _) = after_tier
            .split_once(" |")
            .expect("closing pipe of `--tier` row");
        let mut doc_tiers = Vec::new();
        let mut rem = tier_cell;
        while let Some((_, after_open)) = rem.split_once('`') {
            let Some((tok, tail)) = after_open.split_once('`') else {
                break;
            };
            doc_tiers.push(tok);
            rem = tail;
        }
        let expected_tiers: Vec<&str> = crate::plugin::new::NewTier::ALL
            .iter()
            .map(|t| t.key())
            .collect();
        assert_eq!(
            doc_tiers, expected_tiers,
            "`plugin new` `--tier` table cell drifted from NewTier::ALL"
        );

        let p_dev_sec = doc_subsection(plugin_sec, "plugin dev");
        assert!(
            p_dev_sec.contains(&format!(
                "`{}` ms",
                crate::plugin::dev::POLL_INTERVAL.as_millis()
            )),
            "`plugin dev` poll interval drifted"
        );
        for watched in crate::plugin::dev::WATCHED_ROOTS {
            assert!(
                p_dev_sec.contains(&format!("`{watched}`"))
                    || p_dev_sec.contains(&format!("`{watched}/`")),
                "`plugin dev` must document watched root `{watched}`"
            );
        }

        let p_check_sec = doc_subsection(plugin_sec, "plugin check");
        let mut doc_check_steps = Vec::new();
        for line in p_check_sec.lines() {
            let trimmed = line.trim_start();
            if let Some((_, rest)) = trimmed.split_once(". `")
                && let Some((step, _)) = rest.split_once("`:")
            {
                doc_check_steps.push(step);
            }
        }
        assert_eq!(
            doc_check_steps,
            crate::plugin::check::CHECK_SECTIONS.as_slice(),
            "`plugin check` ordered output sections drifted"
        );

        let p_pack_sec = doc_subsection(plugin_sec, "plugin pack");
        let (_, after_excluding) = p_pack_sec
            .split_once("excluding crate build entries\n(")
            .expect("excluding crate build entries list in `plugin pack`");
        let (excl_line, _) = after_excluding
            .split_once(')')
            .expect("closing paren for excluded entries");
        let doc_excluded: Vec<&str> = excl_line
            .split(',')
            .map(|s| s.trim().trim_matches('`'))
            .collect();
        assert_eq!(
            doc_excluded,
            aria_plugin::package::SOURCE_ENTRIES.as_slice(),
            "`plugin pack` excluded entries drifted from SOURCE_ENTRIES"
        );

        // 7. `aria-desktop` environment variables table.
        let desktop_sec = doc_section(COMMAND_LINE_DOC, "Desktop binary (`aria-desktop`)");
        assert_eq!(
            first_column_cells(desktop_sec),
            vec![
                "ARIA_DATA_DIR".to_owned(),
                "ARIA_PLUGIN_DEV".to_owned(),
                "ARIA_MOTION_SCALE".to_owned(),
                "RUST_LOG".to_owned(),
            ]
        );
    }
}
