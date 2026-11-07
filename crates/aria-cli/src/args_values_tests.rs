//! Flag values: the parsed value of each spelling, and the refusal of a
//! value that is no number, has a zero side, or is past `u32`, stated by the
//! reason. Not covered: the flags a verb reads, which `args_tests` holds.

use super::*;

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
    let args = Args::parse(["frame", "a.aria", "--at", "soon"]).unwrap();
    let err = args.seconds("at").expect_err("\"soon\" parsed as seconds");
    assert!(matches!(err, ArgsError::Usage(message) if message.contains("--at")));

    // Each refusal states why: a zero side, a side past u32, or no number.
    let refused = |raw: &str, why: &str| {
        Err(ArgsError::Usage(format!(
            "--size wants {why}, got \"{raw}\""
        )))
    };
    let zero = "a width and a height above zero";
    let max = "a whole number up to 4294967295";
    for (raw, want) in [
        ("0x480", refused("0x480", zero)),
        ("640x0", refused("640x0", zero)),
        ("0X0", refused("0X0", zero)),
        ("4294967296x2", refused("4294967296x2", max)),
        ("2x4294967296", refused("2x4294967296", max)),
        ("1080", refused("1080", "WIDTHxHEIGHT")),
        ("ax2", refused("ax2", "WIDTHxHEIGHT")),
        ("-2x2", refused("-2x2", "WIDTHxHEIGHT")),
        ("4294967295x1", Ok(Some((u32::MAX, 1)))),
    ] {
        let args = Args::parse(["export", "a.aria", "b.mp4", "--size", raw]).unwrap();
        assert_eq!(args.size("size"), want, "{raw}");
    }
    let refused = |raw: &str, why: &str| {
        Err(ArgsError::Usage(format!(
            "--bitrate wants {why}, got \"{raw}\""
        )))
    };
    for (raw, want) in [
        ("4294967296", refused("4294967296", max)),
        ("1.5", refused("1.5", "a whole number")),
        ("-1", refused("-1", "a whole number")),
        ("4294967295", Ok(Some(u32::MAX))),
    ] {
        let args = Args::parse(["export", "a.aria", "b.mp4", "--bitrate", raw]).unwrap();
        assert_eq!(args.number("bitrate"), want, "{raw}");
    }
}
