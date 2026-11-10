//! `aria new` that fails prints no placement.
//!
//! WHY: `aria new` printed a `placed` line as it placed each file, so a file
//! later in the list that did not import, or a save that failed, ended the
//! command with lines stating placements no project held. Every file is now
//! probed before the first is placed, and the `placed` lines are printed once
//! the save succeeds. A failed save stated the OS reason with no path, as
//! "could not inspect the project destination: file name too long", and now
//! names the output first. Cases: a file that does not import after one that
//! does, and an output whose save fails after every file is placed; each
//! failure starts with the path it is about. Not caught: a placement that
//! fails after its probe succeeded, which no file here causes.

mod support;

use std::ffi::OsStr;
use std::path::Path;

use support::{aria, scratch};

#[test]
fn a_failed_aria_new_prints_no_placement_and_writes_no_project() {
    let dir = scratch("new-failure");
    let still = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png")
        .canonicalize()
        .expect("the still fixture");
    let notes = dir.join("notes.dat");
    std::fs::write(&notes, b"these are notes, not media").expect("write the notes");
    let notes = notes.canonicalize().expect("the notes");
    // A name past the 255 bytes a file name holds: the directory exists, so
    // the command line passes and the save itself fails, for any user.
    let unsaved = dir.join(format!("{}.aria", "n".repeat(300)));

    let unimported = dir.join("unimported.aria");
    let cases = [
        (
            "a file that does not import",
            &unimported,
            &notes,
            Some(&notes),
        ),
        ("a save that fails", &unsaved, &still, Some(&unsaved)),
    ];
    for (case, out, last, named) in cases {
        let ran = aria([
            OsStr::new("new"),
            out.as_os_str(),
            still.as_os_str(),
            last.as_os_str(),
        ]);
        let stderr = String::from_utf8_lossy(&ran.stderr);
        assert_eq!(ran.status.code(), Some(1), "{case}: {stderr}");
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            "",
            "{case}: stdout states placements no project holds"
        );
        if let Some(named) = named {
            assert!(
                stderr.starts_with(&format!("aria: {}: ", named.display())),
                "{case}: {stderr}"
            );
        }
        assert!(
            std::fs::symlink_metadata(out).is_err(),
            "{case}: a project was written"
        );
    }
}
