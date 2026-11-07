//! Process spawning for `aria plugin`.
//!
//! This is the only module in `aria-cli` that spawns child processes: `cargo`
//! for building a plugin crate (`aria plugin dev` and `aria plugin pack`), and
//! the editor binary for `aria plugin dev`.

use std::ffi::OsString;
use std::io::{Read, Write};
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
/// Compiler diagnostics and progress on stderr are copied to this process's
/// stderr as cargo writes them, and retained in [`CargoOutput::stderr`].
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
    run_teeing(&mut cmd, &mut std::io::stderr()).map_err(|err| {
        format!(
            "failed to run cargo for {}: {}",
            manifest_path.display(),
            aria_models::io_reason(&err)
        )
    })
}

/// Run `cmd` to its exit, collecting its stdout, and copying each chunk of
/// its stderr to `progress` as it arrives as well as keeping it.
fn run_teeing(
    cmd: &mut Command,
    progress: &mut (dyn Write + Send),
) -> std::io::Result<CargoOutput> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (Some(mut out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("the child's output was not piped"));
    };
    let (stdout, stderr) = std::thread::scope(|scope| {
        let teed = scope.spawn(move || tee(err, progress));
        let mut stdout = Vec::new();
        let read = out.read_to_end(&mut stdout).map(|_| stdout);
        let stderr = teed
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("the stderr copy panicked")));
        (read, stderr)
    });
    let status = child.wait()?;
    Ok(CargoOutput {
        success: status.success(),
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout?).into_owned(),
        stderr: String::from_utf8_lossy(&stderr?).into_owned(),
    })
}

/// Copy `from` to `to` chunk by chunk until it ends, returning every byte
/// read. A failed write to `to` is ignored: the bytes are still kept.
fn tee(mut from: impl Read, to: &mut dyn Write) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = match from.read(&mut chunk) {
            Ok(0) => return Ok(kept),
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        let _ = to.write_all(&chunk[..read]).and_then(|()| to.flush());
        kept.extend_from_slice(&chunk[..read]);
    }
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
        .map_err(|err| format!("{}: {}", editor.display(), aria_models::io_reason(&err)))
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;

    /// Records what it is given, and creates `seen` once it holds the first
    /// line the child writes.
    struct Progress {
        seen: PathBuf,
        got: Vec<u8>,
    }

    impl Write for Progress {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.got.extend_from_slice(bytes);
            if self.got.starts_with(b"Compiling demo\n") {
                std::fs::write(&self.seen, b"")?;
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// WHY: cargo's stderr reached the terminal only after cargo exited, so
    /// `aria plugin pack` and `aria plugin dev` printed nothing through a whole
    /// build, minutes on a cold cache. A line the child writes reaches the
    /// progress writer while the child still runs: the child writes a line,
    /// waits up to ten seconds for the writer to create a file, and exits 0
    /// only if it did. Stdout and stderr are still returned whole. Not caught:
    /// a terminal that buffers stderr itself.
    #[test]
    fn a_line_the_child_writes_is_forwarded_while_it_still_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let seen = dir.path().join("seen");
        let script = "echo Compiling demo >&2; i=0; \
            while [ ! -e \"$1\" ] && [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; \
            [ -e \"$1\" ] && echo '{}' && echo Finished >&2";
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script).arg("sh").arg(&seen);
        let mut progress = Progress {
            seen,
            got: Vec::new(),
        };
        let output = super::run_teeing(&mut cmd, &mut progress).expect("sh runs");
        assert!(
            output.success,
            "the child saw no forwarded line before it timed out: {output:?}"
        );
        assert_eq!(output.stdout, "{}\n");
        assert_eq!(output.stderr, "Compiling demo\nFinished\n");
        assert_eq!(progress.got, b"Compiling demo\nFinished\n");
    }
}
