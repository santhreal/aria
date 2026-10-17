//! Process spawning for `aria plugin`.
//!
//! This is the only module in `aria-cli` that spawns child processes: `cargo`
//! for building a plugin crate (`aria plugin dev` and `aria plugin pack`), and
//! the editor binary for `aria plugin dev`.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Output of a `cargo build` invocation.
#[derive(Debug, Clone)]
pub(crate) struct CargoOutput {
    /// Whether `cargo` exited with status 0.
    pub(crate) success: bool,
    /// Process exit code, when terminated normally.
    pub(crate) code: Option<i32>,
    /// JSON messages emitted on standard output.
    pub(crate) stdout: String,
    /// Human-readable compiler diagnostics emitted on standard error.
    pub(crate) stderr: String,
}

fn cargo_binary() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

/// Run `cargo build --lib --manifest-path <manifest_path> --message-format=json-render-diagnostics`.
///
/// Compiler diagnostics on stderr are forwarded to this process's stderr as well
/// as retained in [`CargoOutput::stderr`].
pub(crate) fn cargo_build(
    manifest_path: &Path,
    release: bool,
    target: Option<&str>,
) -> Result<CargoOutput, String> {
    let mut cmd = Command::new(cargo_binary());
    cmd.arg("build")
        .arg("--lib")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--message-format=json-render-diagnostics");
    if release {
        cmd.arg("--release");
    }
    if let Some(target) = target {
        cmd.arg("--target").arg(target);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|err| format!("failed to run cargo for {}: {err}", manifest_path.display()))?;
    if !output.stderr.is_empty() {
        let _ = std::io::stderr().write_all(&output.stderr);
    }
    Ok(CargoOutput {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Spawn the editor binary with `ARIA_PLUGIN_DEV` pointing at `plugin_dir` and
/// optional `ARIA_DATA_DIR`.
pub(crate) fn spawn_editor(
    editor: &Path,
    plugin_dir: &Path,
    data_dir: Option<&Path>,
) -> Result<Child, String> {
    let mut cmd = Command::new(editor);
    cmd.env("ARIA_PLUGIN_DEV", plugin_dir);
    if let Some(data_dir) = data_dir {
        cmd.env("ARIA_DATA_DIR", data_dir);
    }
    cmd.spawn()
        .map_err(|err| format!("{}: {err}", editor.display()))
}
