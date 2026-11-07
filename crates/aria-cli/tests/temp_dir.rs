//! A command that unpacks into a scratch directory states the temporary
//! directory when it cannot make one there.
//!
//! WHY: `aria plugin check x.ariaplugin` with `TMPDIR` set to a missing
//! directory failed with "x.ariaplugin: no such file or directory" for an
//! archive that exists. The failure states the temporary directory and its
//! own error, and exits 1. Does not cover a temporary directory that exists
//! and is not writable: that failure states tempfile's error, which appends
//! the scratch name it tried.

mod support;

use std::ffi::OsStr;

use support::{aria_with, scratch};

#[test]
fn a_missing_temporary_directory_is_stated_by_its_own_path() {
    let dir = scratch("temp-dir");
    let archive = dir.join("color.ariaplugin");
    std::fs::write(&archive, b"").expect("write the archive");
    let tmp = dir.join("absent");
    let output = aria_with(
        &[("TMPDIR", tmp.as_os_str())],
        [
            OsStr::new("plugin"),
            OsStr::new("check"),
            archive.as_os_str(),
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert_eq!(
        stderr,
        format!("aria: {}: no such file or directory\n", tmp.display())
    );
}
