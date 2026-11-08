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
#[test]
#[ignore = "child process of closed_decode_session_skips_library_destructor_on_exit"]
fn closed_session_exit_child() {
    #[allow(unsafe_code)]
    // SAFETY: registers a stand-in destructor before a decode session closes.
    let registered = unsafe { libc::atexit(never_returns) };
    assert_eq!(registered, 0, "atexit did not register the handler");
    let guard = crate::SessionGuard::new(std::path::Path::new("closed.mp4"));
    assert_eq!(crate::open_decode_sessions(), 1);
    drop(guard);
    assert_eq!(crate::open_decode_sessions(), 0);
    print!("{UNFLUSHED}");
    std::process::exit(i32::from(CODE));
}

#[test]
fn closed_decode_session_skips_library_destructor_on_exit() {
    let ran = crate::test_child::run("exit::tests::closed_session_exit_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the child ran the destructor after closing its decode session and did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert_eq!(status.code(), Some(i32::from(CODE)));
    assert!(ran.stdout.contains(UNFLUSHED));
    assert!(ran.took < BOUND);
}

extern "C" fn finish_from_destructor() {
    #[allow(unsafe_code)]
    // SAFETY: ends the child with `CODE` to prove this handler ran when a
    // decode session remained open at exit.
    unsafe {
        libc::_exit(i32::from(CODE));
    }
}

#[test]
#[ignore = "child process of open_decode_session_does_not_skip_exit_handlers"]
fn leaked_session_exit_child() {
    #[allow(unsafe_code)]
    // SAFETY: registers a stand-in handler before a decode session closes.
    let registered = unsafe { libc::atexit(finish_from_destructor) };
    assert_eq!(registered, 0, "atexit did not register the handler");
    let closed = crate::SessionGuard::new(std::path::Path::new("closed.mp4"));
    drop(closed);
    let leaked = crate::SessionGuard::new(std::path::Path::new("leaked.mp4"));
    std::mem::forget(leaked);
    assert_eq!(crate::open_decode_sessions(), 1);
    std::process::exit(0);
}

#[test]
fn open_decode_session_does_not_skip_exit_handlers() {
    let ran = crate::test_child::run("exit::tests::leaked_session_exit_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the child did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert_eq!(
        status.code(),
        Some(i32::from(CODE)),
        "an unclosed decode session bypassed the exit handler instead of leaving it active\n{}",
        ran.transcript()
    );
}

/// WHY: both exits through `_exit`, [`super::exit_process`] and the hook a
/// closed decode session arms, ended the process before the coverage
/// runtime's own exit handler ran, so a process that decoded video left an
/// empty profile and every line it ran read as never run. Each child below
/// ends through one of the two, and the profile it was told to write must
/// hold data. Compiled only under `cargo llvm-cov` (`cfg(coverage)`); a plain
/// test run has no profile to check. It does not check what the profile
/// counts, only that it was written.
#[cfg(coverage)]
#[test]
fn every_exit_through_exit_writes_the_coverage_profile() {
    for child in [
        "exit::tests::profile_session_exit_child",
        "exit::tests::profile_exit_process_child",
    ] {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let profile = dir.path().join("child.profraw");
        let ran =
            crate::test_child::run_in(child, BOUND, &[("LLVM_PROFILE_FILE", profile.as_os_str())]);
        assert_eq!(
            ran.status.and_then(|status| status.code()),
            Some(i32::from(CODE)),
            "{child} did not end with its code\n{}",
            ran.transcript()
        );
        let written = std::fs::metadata(&profile).map_or(0, |meta| meta.len());
        assert!(
            written > 0,
            "{child} ended through _exit and left an empty coverage profile ({written} bytes)"
        );
    }
}

#[cfg(coverage)]
#[test]
#[ignore = "child process of every_exit_through_exit_writes_the_coverage_profile"]
fn profile_session_exit_child() {
    let guard = crate::SessionGuard::new(std::path::Path::new("profiled.mp4"));
    drop(guard);
    assert_eq!(crate::open_decode_sessions(), 0);
    std::process::exit(i32::from(CODE));
}

#[cfg(coverage)]
#[test]
#[ignore = "child process of every_exit_through_exit_writes_the_coverage_profile"]
fn profile_exit_process_child() {
    super::exit_process(CODE);
}
