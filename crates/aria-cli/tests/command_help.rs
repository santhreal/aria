//! `aria <command> --help` prints that command's entries, not the whole
//! usage block.
//!
//! WHY: `aria frame --help` printed every command's entry, the same block as
//! `aria --help`. `args_tests` holds the text for every verb; this holds that
//! the binary prints it on stdout with exit 0 and nothing on stderr. Does not
//! cover the wording of an entry.

#[allow(
    dead_code,
    reason = "help writes no file, so no case here needs scratch"
)]
mod support;

use support::aria;

#[test]
fn help_after_a_command_prints_that_command_alone() {
    let output = aria(["frame", "p.aria", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert!(stdout.starts_with("    frame "), "{stdout}");
    assert!(
        stdout.ends_with("\nRun `aria --help` for every command.\n"),
        "{stdout}"
    );
    assert!(!stdout.contains("    export "), "{stdout}");

    let output = aria(["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("    frame ") && stdout.contains("    export "));
}
