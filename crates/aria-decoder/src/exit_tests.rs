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

use std::time::Duration;

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
    let ran = crate::test_child::run("exit::tests::exit_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the child ran an exit handler and did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert_eq!(
        status.code(),
        Some(i32::from(CODE)),
        "the child ended with another status\n{}",
        ran.transcript()
    );
    assert!(
        ran.stdout.contains(UNFLUSHED),
        "the unterminated stdout line was lost at exit\n{}",
        ran.transcript()
    );
    assert!(ran.took < BOUND);
}
