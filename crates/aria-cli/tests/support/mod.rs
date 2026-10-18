//! The one place the `aria-cli` integration tests start the `aria` binary this
//! crate builds, and the scratch directory each case writes to.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An empty directory for one case, at a fixed name under the target's temp
/// directory, so every run reuses it.
pub(crate) fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("aria-cli-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

/// Run the `aria` binary with `args` and wait for it to exit.
pub(crate) fn aria<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_aria"))
        .args(args)
        .output()
        .expect("run aria")
}
