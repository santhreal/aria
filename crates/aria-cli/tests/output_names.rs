//! `aria` refuses an output named for a format other than the one the
//! command writes, before it reads anything or writes a byte.
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
