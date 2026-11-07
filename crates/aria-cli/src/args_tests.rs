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

/// Closes the defect classes where:
/// 1. A valueless flag (`--no-audio`, `--quiet`, `--clips`, `--beats`,
///    `--silence`, `--loudness`, `--shots`, `--no-build`) placed before a
///    positional argument swallows the positional as its value, or accepts
///    `--flag=value` without failing on non-plugin verbs;
/// 2. A fixed-arity verb (`demo`, `frame`, `export`, `inspect`, `analyze`,
///    or `plugin` sub-commands) silently ignores unexpected extra
///    positional arguments;
/// 3. `frame --at` silently clamps a negative timestamp to 0 instead of
///    rejecting it like `export --at` / `export --to`.
#[test]
fn valueless_flags_extra_positionals_and_negative_seconds_are_validated_across_all_verbs() {
    // 1. Valueless flags derived from `USAGE` synopses (`[--flag]` with no placeholder)
    // vs value-taking flags (`[--flag VALUE]`).
    let mut valueless_by_verb: Vec<(Vec<&str>, String)> = Vec::new();
    for line in USAGE.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !line.starts_with("    ") || line.starts_with("        ") {
            continue;
        }
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let prefix: Vec<&str> = if words.first() == Some(&"plugin") {
            vec!["plugin", words[1]]
        } else {
            vec![words[0]]
        };
        let mut rest = line;
        while let Some((_, after_open)) = rest.split_once("[--") {
            let Some((inside, tail)) = after_open.split_once(']') else {
                break;
            };
            if !inside.contains(' ') {
                valueless_by_verb.push((prefix.clone(), inside.to_owned()));
            }
            rest = tail;
        }
    }
    assert!(
        !valueless_by_verb.is_empty(),
        "USAGE must declare valueless flags"
    );
    for (prefix, flag) in &valueless_by_verb {
        // Placed before positionals, a valueless flag must not swallow the positional.
        let mut argv: Vec<String> = prefix.iter().map(|s| (*s).to_owned()).collect();
        argv.push(format!("--{flag}"));
        argv.push("first.arg".to_owned());
        if prefix.as_slice() == ["export"] {
            argv.push("second.arg".to_owned());
        }
        let parsed = Args::parse(argv).expect("parse valueless flag before positional");
        let expected_pos: Vec<&str> = match prefix.as_slice() {
            ["plugin", sub] => vec![*sub, "first.arg"],
            ["export"] => vec!["first.arg", "second.arg"],
            _ => vec!["first.arg"],
        };
        assert_eq!(
            parsed.positional, expected_pos,
            "valueless flag --{flag} on {prefix:?} swallowed the positional argument"
        );

        // Passing `=value` to any valueless flag must fail with exit code 2.
        let mut bad_argv: Vec<String> = prefix.iter().map(|s| (*s).to_owned()).collect();
        bad_argv.push("first.arg".to_owned());
        if prefix.as_slice() == ["export"] {
            bad_argv.push("second.arg".to_owned());
        }
        bad_argv.push(format!("--{flag}=yes"));
        let parsed_bad = Args::parse(bad_argv).expect("parse argv");
        let err = crate::run(&parsed_bad).expect_err("--flag=value must fail");
        assert_eq!(err.code(), 2);
        assert_eq!(
            err,
            crate::fail::Failure::Usage(format!("--{flag} takes no value, got \"yes\"")),
            "valueless flag --{flag} on {prefix:?}"
        );
    }

    // 2. Every fixed-arity verb in `Verb::ALL` (all except `new`, which takes `[media...]`)
    // and every `plugin` sub-command in `Subcommand::ALL` must reject an extra positional.
    for verb in Verb::ALL {
        match verb {
            Verb::New => continue,
            Verb::Plugin => {
                for sub in crate::plugin::Subcommand::ALL {
                    let args = Args::parse(["plugin", sub.key(), "target", "extra"]).unwrap();
                    let err = crate::run(&args).expect_err("extra positional must fail");
                    assert_eq!(err.code(), 2);
                    assert_eq!(
                        err,
                        crate::fail::Failure::Usage("unexpected argument \"extra\"".into()),
                        "plugin {}",
                        sub.key()
                    );
                }
            }
            Verb::Demo | Verb::Inspect | Verb::Analyze => {
                let key = verb.key();
                let args = Args::parse([key, "target", "extra"]).unwrap();
                let err = crate::run(&args).expect_err("extra positional must fail");
                assert_eq!(err.code(), 2);
                assert_eq!(
                    err,
                    crate::fail::Failure::Usage("unexpected argument \"extra\"".into()),
                    "verb {key}"
                );
            }
            Verb::Frame | Verb::Export => {
                let key = verb.key();
                let args = Args::parse([key, "in.aria", "out.bin", "extra"]).unwrap();
                let err = crate::run(&args).expect_err("extra positional must fail");
                assert_eq!(err.code(), 2);
                assert_eq!(
                    err,
                    crate::fail::Failure::Usage("unexpected argument \"extra\"".into()),
                    "verb {key}"
                );
            }
        }
    }

    // 3. Negative seconds on `--at` / `--to` are refused across `frame` and `export`.
    for argv in [
        &["frame", "a.aria", "--at", "-1"][..],
        &["export", "a.aria", "b.mp4", "--at", "-0.5"][..],
        &["export", "a.aria", "b.mp4", "--to", "-2"][..],
    ] {
        let flag = argv[argv.len() - 2];
        let args = Args::parse(argv.iter().copied()).unwrap();
        let err = crate::run(&args).expect_err("negative seconds must fail");
        assert_eq!(err.code(), 2);
        assert_eq!(
            err,
            crate::fail::Failure::Usage(format!("{flag} cannot be negative"))
        );
    }
}

/// No verb and `--help` are not failures: both print usage and exit 0.
/// `--help` after a command names the command; before one, or after a word
/// that is no command, it names none.
#[test]
fn no_verb_and_help_are_not_errors() {
    assert_eq!(
        Args::parse::<[&str; 0], &str>([]).err(),
        Some(ArgsError::NoVerb)
    );
    assert_eq!(Args::parse(["--help"]).err(), Some(ArgsError::Help(None)));
    assert_eq!(
        Args::parse(["-h", "export"]).err(),
        Some(ArgsError::Help(None))
    );
    assert_eq!(
        Args::parse(["exprot", "--help"]).err(),
        Some(ArgsError::Help(None))
    );
    for verb in Verb::ALL {
        for help in ["--help", "-h"] {
            assert_eq!(
                Args::parse([verb.key(), "p.aria", help, "--at"]).err(),
                Some(ArgsError::Help(Some(verb))),
                "{} {help}",
                verb.key()
            );
        }
    }
    assert_eq!(
        Args::parse(["render"]).err(),
        Some(ArgsError::Usage("unknown command \"render\"".into()))
    );
}

/// WHY: `aria help frame` failed with "unknown command \"help\"", exit 2,
/// though `cargo help build` and `git help log` take that spelling. `help`
/// and `help help` are the usage block and `help <command>` that command's
/// entries for every verb in `Verb::ALL`; a word that is no command after it,
/// or two words, is a usage error. Not caught: `help` after a verb, which is
/// that verb's positional.
#[test]
fn help_as_the_first_word_names_the_command_after_it() {
    for argv in [vec!["help"], vec!["help", "help"]] {
        assert_eq!(Args::parse(argv).err(), Some(ArgsError::Help(None)));
    }
    for verb in Verb::ALL {
        assert_eq!(
            Args::parse(["help", verb.key()]).err(),
            Some(ArgsError::Help(Some(verb))),
            "help {}",
            verb.key()
        );
    }
    for (argv, says) in [
        (vec!["help", "render"], "unknown command \"render\""),
        (
            vec!["help", "frame", "export"],
            "help takes one command name",
        ),
    ] {
        assert_eq!(
            Args::parse(argv.clone()).err(),
            Some(ArgsError::Usage(says.into())),
            "{argv:?}"
        );
    }
}

/// WHY: a word with one leading dash fell through to the positionals, so
/// `aria frame p.aria -o` wrote a PNG named `-o`, `aria new -x` a project
/// named `-x`, and `aria frame p.aria -o x.png` failed on "x.png" instead of
/// the `-o`. Every verb and plugin sub-command is enumerated, with the dash
/// word in each positional slot. A dash word that is a flag's value still
/// parses (`--at -1` reaches the negative-seconds check), and `-h` is still
/// help. Not caught: a path that starts with a dash on purpose, which is
/// written `./-name`.
#[test]
fn a_single_dash_word_is_a_usage_error_and_not_a_path() {
    let mut prefixes: Vec<Vec<&str>> = Verb::ALL
        .iter()
        .filter(|verb| **verb != Verb::Plugin)
        .map(|verb| vec![verb.key()])
        .collect();
    prefixes.extend(
        crate::plugin::Subcommand::ALL
            .iter()
            .map(|sub| vec!["plugin", sub.key()]),
    );
    for prefix in &prefixes {
        for dash in ["-o", "-", "-x.png", "-1"] {
            for slot in 0..2 {
                let mut argv = prefix.clone();
                argv.extend(std::iter::repeat_n("p.aria", slot));
                argv.push(dash);
                argv.push("out.png");
                assert_eq!(
                    Args::parse(argv.clone()).err(),
                    Some(ArgsError::Usage(format!(
                        "\"{dash}\" is not a flag: flags are spelled --name, and a path \
                         that starts with - is written ./{dash}"
                    ))),
                    "{argv:?}"
                );
            }
        }
    }
    let args = Args::parse(["export", "p.aria", "o.mp4", "--at", "-1"]).expect("--at -1");
    assert_eq!(args.positional, ["p.aria", "o.mp4"]);
    assert_eq!(args.value("at"), Ok(Some("-1")));
    assert_eq!(
        Args::parse(["frame", "p.aria", "./-o"])
            .map(|args| args.positional)
            .ok(),
        Some(vec!["p.aria".to_owned(), "./-o".to_owned()])
    );
    assert_eq!(
        Args::parse(["frame", "-h"]).err(),
        Some(ArgsError::Help(Some(Verb::Frame)))
    );
}

/// `aria --version` prints the version and exits 0; it is a top-level word,
/// so after a verb it is that verb's unknown flag.
#[test]
fn version_is_the_first_word_only() {
    assert_eq!(Args::parse(["--version"]).err(), Some(ArgsError::Version));
    assert_eq!(
        Args::parse(["--version", "export"]).err(),
        Some(ArgsError::Version)
    );
    let args = Args::parse(["export", "--version"]).expect("parses as a flag");
    assert_eq!(
        args.reject_unknown(Verb::Export.flags()),
        Err(ArgsError::Usage("export does not take --version".into()))
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
    let start = section
        .find(&marker)
        .unwrap_or_else(|| panic!("missing subsection `### {heading}` in docs/command-line.md"));
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
    let verb_set: std::collections::BTreeSet<&str> = Verb::ALL.iter().map(|v| v.key()).collect();
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
            .take_while(|line| line.starts_with(&usage_marker) || line.starts_with("          "))
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

    // Prose reflows; the facts it states do not. Words are compared with
    // every run of whitespace collapsed, so a rewrap cannot fail the check.
    let analyze_sec = doc_section(COMMAND_LINE_DOC, "analyze")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        analyze_sec.contains(&format!("`{}`", crate::analyze::DEFAULT_LIMIT)),
        "`analyze` section must document DEFAULT_LIMIT"
    );
    assert!(
        analyze_sec.contains(&format!("`{}` fps", aria_detect::SHOT_SAMPLE_FPS))
            && analyze_sec.contains(&format!(
                "`{}` minutes (`{}` seconds)",
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
        .split_once("| Execution tier (`NewTier::ALL` in `crates/aria-cli/src/plugin/new.rs`): ")
        .expect("`--tier` row in `plugin new` table");
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

/// WHY: every usage error printed the whole 50-line usage block after the
/// reason, so a one-word typo buried its own message, and the block ran the
/// `analyze` entry into `plugin new` with no blank line between them. A usage
/// error now prints the reason, the entries of the command it names, and a
/// pointer to `--help`; a runtime failure prints the reason alone. `aria
/// frame --help` printed the whole block as well; `--help` after a command
/// prints that command's entries and the pointer. Every verb is enumerated
/// from `Verb::ALL`, so a verb added without a `USAGE` entry fails here. Not
/// caught: the wording of an entry's description.
#[test]
fn a_usage_error_prints_the_named_command_and_not_the_whole_block() {
    let entry_starts = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.len() - line.trim_start().len() == 4)
            .map(str::to_owned)
            .collect()
    };
    for verb in Verb::ALL {
        let key = verb.key();
        let entry = synopsis(verb);
        let starts = entry_starts(&entry);
        assert!(!starts.is_empty(), "`{key}` has no USAGE entry");
        for start in &starts {
            assert!(
                start.starts_with(&format!("    {key} ")),
                "`{key}` printed another command's entry: {start}"
            );
        }
        for flag in verb.flags() {
            assert!(
                entry.contains(&format!("--{flag}")),
                "`{key}` synopsis omits --{flag}"
            );
        }
        assert!(
            !entry.contains("COMMANDS") && !entry.contains("EXIT CODES"),
            "`{key}` printed section headings"
        );
        let failure = crate::fail::Failure::Usage("bad".into());
        assert_eq!(
            crate::report_text(&failure, Some(verb)),
            format!("aria: bad\n\n{entry}\nRun `aria --help` for every command.\n"),
            "`{key}`"
        );
        assert_eq!(
            crate::help_text(Some(verb)),
            format!("{entry}\nRun `aria --help` for every command.\n"),
            "`{key} --help`"
        );
    }
    assert_eq!(crate::help_text(None), USAGE);
    let plugin = synopsis(Verb::Plugin);
    for sub in crate::plugin::Subcommand::ALL {
        assert!(
            plugin.contains(&format!("    plugin {} ", sub.key())),
            "`plugin {}` missing from the plugin synopsis",
            sub.key()
        );
    }

    assert_eq!(
        crate::report_text(
            &crate::fail::Failure::Usage("unknown command \"exprot\"".into()),
            None
        ),
        "aria: unknown command \"exprot\"\n\nRun `aria --help` for every command.\n"
    );
    assert_eq!(
        crate::report_text(
            &crate::fail::Failure::Failed("no codec".into()),
            Some(Verb::Export)
        ),
        "aria: no codec\n"
    );

    // Every entry in COMMANDS starts after a blank line.
    let commands = USAGE
        .split_once("COMMANDS\n")
        .and_then(|(_, rest)| rest.split_once("\nEXIT CODES"))
        .map(|(body, _)| body)
        .expect("COMMANDS section");
    let lines: Vec<&str> = commands.lines().collect();
    for (index, line) in lines.iter().enumerate().skip(1) {
        if line.len() - line.trim_start().len() == 4 {
            assert_eq!(lines[index - 1], "", "no blank line before {line:?}");
        }
    }
    assert_eq!(
        entry_starts(commands).len(),
        Verb::ALL.len() - 1 + crate::plugin::Subcommand::ALL.len(),
        "one entry per verb and per plugin sub-command"
    );
}

/// WHY: `export p.aria out.webm` wrote MP4 into `out.webm`, and `frame p.aria
/// still.jpg` wrote a PNG named `.jpg`: each verb writes one format whatever
/// the output is named. Every verb in `Verb::ALL` that writes a file refuses
/// a name with no extension or with another verb's, and accepts its own in
/// either case, with any dots in the directory; a default output name is in
/// the verb's format. `Verb::writes` is an exhaustive match, so a new verb
/// does not compile until it states what it writes.
///
/// Not caught: `plugin pack --out`, whose archive name is a flag value.
#[test]
fn a_verb_refuses_an_output_named_for_another_format() {
    let writers: Vec<(Verb, usize, &str)> = Verb::ALL
        .into_iter()
        .filter_map(|verb| {
            verb.writes()
                .map(|(index, extension)| (verb, index, extension))
        })
        .collect();
    assert!(writers.len() >= 4, "only {writers:?} write a file");
    let line = |verb: Verb, index: usize, out: &str| {
        let mut words = vec![verb.key().to_string()];
        words.extend((0..index).map(|_| "p.aria".to_string()));
        words.push(out.to_string());
        Args::parse(words).expect("parse")
    };
    for &(verb, index, extension) in &writers {
        let others = writers
            .iter()
            .map(|(_, _, other)| *other)
            .filter(|other| *other != extension)
            .chain(["webm"]);
        for wrong in std::iter::once(String::new()).chain(others.map(|other| format!(".{other}"))) {
            let out = format!("cut{wrong}");
            let err = line(verb, index, &out)
                .reject_misnamed_output()
                .expect_err("an output named for another format was accepted");
            assert_eq!(
                err,
                ArgsError::Usage(format!(
                    "{} writes a .{extension} file; name the output \"cut.{extension}\" rather than \"{out}\"",
                    verb.key()
                )),
                "{} {out}",
                verb.key()
            );
        }
        for right in [
            format!("cut.{extension}"),
            format!("cut.{}", extension.to_uppercase()),
            format!("v1.2/cut.{extension}"),
        ] {
            assert_eq!(
                line(verb, index, &right).reject_misnamed_output(),
                Ok(()),
                "{} {right}",
                verb.key()
            );
        }
    }
    for (verb, default) in [
        (Verb::Frame, crate::frame::DEFAULT_OUT),
        (Verb::Demo, crate::demo::DEFAULT_OUT),
    ] {
        let (_, extension) = verb.writes().expect("the verb writes a file");
        assert_eq!(
            std::path::Path::new(default).extension(),
            Some(std::ffi::OsStr::new(extension)),
            "{} writes {default} by default",
            verb.key()
        );
    }
}

/// WHY: `aria frame p.aria nodir/still.png` rendered and then failed with
/// "i/o error: No such file or directory (os error 2)", naming no path, and
/// an output that is a directory or a symbolic link failed as an i/o error
/// with exit code 1. Every verb in `Verb::ALL` that writes a file refuses an
/// output that is not a file, or whose directory is missing or is a file, and
/// accepts a file, a new name in a directory that exists, or a bare name.
///
/// Not caught: a directory that exists but cannot be written; the write
/// states that directory.
#[test]
fn a_verb_refuses_an_output_it_cannot_write_as_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("plain");
    std::fs::write(&file, b"a file").expect("write a file");
    let mut not_files = vec![(dir.path().join("folder.png"), "a directory")];
    std::fs::create_dir(&not_files[0].0).expect("make a directory");
    #[cfg(unix)]
    {
        let link = dir.path().join("link.mp4");
        std::os::unix::fs::symlink(&file, &link).expect("link to a file");
        not_files.push((link, "a symbolic link"));
    }
    let mut writers = 0;
    for verb in Verb::ALL {
        let Some((index, extension)) = verb.writes() else {
            continue;
        };
        writers += 1;
        let line = |out: &std::path::Path| {
            let mut words = vec![verb.key().to_string()];
            words.extend((0..index).map(|_| "p.aria".to_string()));
            words.push(out.display().to_string());
            Args::parse(words).expect("parse")
        };
        let name = format!("cut.{extension}");
        for missing in [dir.path().join("nodir"), file.clone()] {
            let out = missing.join(&name);
            assert_eq!(
                line(&out).reject_unwritable_output(),
                Err(ArgsError::Usage(format!(
                    "there is no directory \"{}\" to write \"{}\" in",
                    missing.display(),
                    out.display()
                ))),
                "{} {}",
                verb.key(),
                out.display()
            );
        }
        for (out, what) in &not_files {
            assert_eq!(
                line(out).reject_unwritable_output(),
                Err(ArgsError::Usage(format!(
                    "\"{}\" is {what}; name a file to write",
                    out.display()
                )))
            );
        }
        for out in [dir.path().join(&name), name.clone().into(), file.clone()] {
            assert_eq!(
                line(&out).reject_unwritable_output(),
                Ok(()),
                "{} {}",
                verb.key(),
                out.display()
            );
        }
    }
    assert!(writers >= 4, "only {writers} verbs write a file");
}
