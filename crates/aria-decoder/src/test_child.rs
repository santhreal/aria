//! Running one ignored test of this test binary in a child process, for a
//! test whose claim is about how a process ends.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// What a child run produced.
pub(crate) struct Ran {
    /// How the child ended, or `None` when it ran past the bound and was
    /// killed.
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    /// From the spawn to the end or the kill.
    pub(crate) took: Duration,
}

impl Ran {
    /// The child's stdout and stderr, for an assertion message.
    pub(crate) fn transcript(&self) -> String {
        format!("stdout:\n{}\nstderr:\n{}", self.stdout, self.stderr)
    }
}

/// Run the ignored test at `path` (such as `exit::tests::exit_child`) in a
/// child of this test binary with its output uncaptured, and kill the child
/// once it runs longer than `bound`.
pub(crate) fn run(path: &str, bound: Duration) -> Ran {
    let exe = std::env::current_exe().expect("the test binary has a path");
    let started = Instant::now();
    let mut child = Command::new(exe)
        .args([
            "--exact",
            path,
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the test binary starts again as the child");
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            break Some(status);
        }
        if started.elapsed() > bound {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let took = started.elapsed();
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_string(&mut stdout)
            .expect("child stdout reads");
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .expect("child stderr reads");
    }
    Ran {
        status,
        stdout,
        stderr,
        took,
    }
}
