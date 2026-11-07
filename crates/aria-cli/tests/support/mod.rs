//! The one place the `aria-cli` integration tests start the `aria` binary this
//! crate builds or signal it, and the scratch directory each case writes to.
//!
//! Each test file declares its own copy of this module and uses a subset of it.

#![allow(dead_code)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

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
    aria_with(&[], args)
}

/// Run the `aria` binary with `args` and the environment variables `vars`
/// set, and wait for it to exit.
pub(crate) fn aria_with<I, S>(vars: &[(&str, &OsStr)], args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_aria"))
        .envs(vars.iter().copied())
        .args(args)
        .output()
        .expect("run aria")
}

/// Start the `aria` binary with `args`, stdout discarded and stderr piped,
/// and return while it runs.
pub(crate) fn start<I, S>(args: I) -> Child
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_aria"))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start aria")
}

/// Send `child` the signal named `signal`, such as `TERM`, through `kill`.
#[cfg(unix)]
pub(crate) fn send_signal(child: &Child, signal: &str) {
    let sent = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("run kill");
    assert!(sent.success(), "kill -{signal} failed");
}
