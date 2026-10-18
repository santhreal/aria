//! [`super::exit_process`] ends the process with no exit handler run.
//!
//! WHY: `libnvcuvid`'s library destructor joins a decode worker thread that
//! was already joined, so a process that decoded on NVDEC and ended through
//! `exit` hung or aborted after writing its output (the Desktop `playback`
//! bench). The class is "code another library registered to run at exit".
//! The child below registers an exit handler that never returns, the stand-in
//! for that second join, and the test asserts the child ends within a bound,
//! with its code, and with its unterminated stdout line delivered. It does not
//! exercise `libnvcuvid` itself; the Desktop `playback` bench does.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The exit code the child ends with.
const CODE: u8 = 37;

/// Printed with no newline, so it stays in stdout's line buffer until a flush.
const UNFLUSHED: &str = "written before exit, no newline";

/// How long the child may take to start, run, and end.
const BOUND: Duration = Duration::from_secs(30);

/// An exit handler that never returns, as the second join of a joined thread
/// does when its descriptor went to a thread that is still running.
extern "C" fn never_returns() {
    loop {
        std::thread::park();
    }
}

#[test]
#[ignore = "child process of exit_process_runs_no_exit_handler"]
fn exit_child() {
    #[allow(unsafe_code)]
    // SAFETY: registers a handler with the C exit path; `never_returns` takes
    // no arguments and touches no state.
    let registered = unsafe { libc::atexit(never_returns) };
    assert_eq!(registered, 0, "atexit did not register the handler");
    print!("{UNFLUSHED}");
    super::exit_process(CODE);
}

#[test]
fn exit_process_runs_no_exit_handler() {
    let exe = std::env::current_exe().expect("the test binary has a path");
    let started = Instant::now();
    let mut child = Command::new(exe)
        .args([
            "--exact",
            "exit::tests::exit_child",
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
        if started.elapsed() > BOUND {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
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
    let status = status.unwrap_or_else(|| {
        panic!(
            "the child ran an exit handler and did not end within {BOUND:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        )
    });
    assert_eq!(
        status.code(),
        Some(i32::from(CODE)),
        "the child ended with another status\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(UNFLUSHED),
        "the unterminated stdout line was lost at exit\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(started.elapsed() < BOUND);
}
