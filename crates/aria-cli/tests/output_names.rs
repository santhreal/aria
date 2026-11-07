//! `aria` refuses an output named for a format other than the one the
//! command writes, or in a directory that does not exist, before it reads
//! anything or writes a byte.
//!
//! WHY: `aria export p.aria out.webm` wrote MP4 into `out.webm`. The census
//! in `args_tests` holds each verb's table; this holds that the binary runs
//! the check: a refusal exits 2, names the file to write instead, and leaves
//! no file. Does not cover `plugin pack --out`.

mod support;

use support::{aria, scratch};

#[test]
fn a_misnamed_output_exits_2_and_writes_nothing() {
    let dir = scratch("output-names");
    let project = dir.join("absent.aria");
    for (verb, out, named) in [
        ("export", "cut.webm", "cut.mp4"),
        ("frame", "still.jpg", "still.png"),
    ] {
        let out = dir.join(out);
        let output = aria([
            std::ffi::OsStr::new(verb),
            project.as_os_str(),
            out.as_os_str(),
        ]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{verb}: {stderr}");
        let named = dir.join(named);
        assert!(
            stderr.contains(&format!("name the output \"{}\"", named.display())),
            "{verb}: {stderr}"
        );
        assert!(!out.exists(), "{verb} wrote {}", out.display());
    }
}

/// WHY: `aria new nodir/p.aria song.wav` printed a `placed` line for each
/// file and then failed with a bare "No such file or directory (os error 2)".
/// The binary refuses an output in a missing directory before it reads an
/// input: exit 2, the directory named, nothing printed to stdout.
#[test]
fn an_output_in_a_missing_directory_exits_2_before_any_work() {
    let dir = scratch("output-dir");
    let missing = dir.join("nodir");
    let sound = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-sine-48k-2s.wav");
    let project = dir.join("absent.aria");
    let new_out = missing.join("p.aria");
    let frame_out = missing.join("still.png");
    for (out, words) in [
        (&new_out, ["new", path(&new_out), path(&sound)]),
        (&frame_out, ["frame", path(&project), path(&frame_out)]),
    ] {
        let output = aria(words);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{words:?}: {stderr}");
        assert!(
            stderr.starts_with(&format!(
                "aria: there is no directory \"{}\" to write \"{}\" in\n",
                missing.display(),
                out.display()
            )),
            "{words:?}: {stderr}"
        );
        assert!(output.stdout.is_empty(), "{words:?} printed to stdout");
    }
}

fn path(path: &std::path::Path) -> &str {
    path.to_str().expect("a UTF-8 scratch path")
}
