//! Ending a process that decoded video.

use std::io::Write;

/// Flush stdout and stderr, then end the process with `code`.
///
/// On Linux the process ends through `_exit`: no `atexit` handler and no
/// shared library destructor runs after this call. NVIDIA's `libnvcuvid`
/// (driver 580) joins, from its library destructor, the worker thread of the
/// last decode session it opened, a thread that session's teardown already
/// joined. glibc gives a joined thread's descriptor to the next thread it
/// starts, so the second join waits on an unrelated thread or on a released
/// descriptor, and the process hangs or aborts after its work is done. A
/// device warm-up thread still inside a plugin load is also ended where it
/// stands, with no destructor run under it.
///
/// Nothing on any stack is dropped after this call: drop every value whose
/// drop writes something (an output file, a log writer) before calling it.
pub fn exit_process(code: u8) -> ! {
    // A closed stdout leaves nothing to flush to; the exit code still stands.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    write_coverage_profile();
    #[cfg(target_os = "linux")]
    #[allow(unsafe_code)]
    // SAFETY: `_exit` takes any status and has no preconditions; it does not
    // return and runs no code of this process after the call.
    unsafe {
        libc::_exit(i32::from(code))
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(i32::from(code))
}

/// Write this process's coverage profile, in a build instrumented by
/// `cargo llvm-cov`.
///
/// The profile runtime writes from an `atexit` handler registered before
/// `main`, so it runs after every handler registered later, and `_exit` from
/// one of those ends the process first. Without this call a process that
/// decoded video wrote an empty profile, and every line it ran read as never
/// run.
#[cfg(coverage)]
fn write_coverage_profile() {
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn __llvm_profile_write_file() -> std::ffi::c_int;
    }
    #[allow(unsafe_code)]
    // SAFETY: the profile runtime is linked into every instrumented binary;
    // the call reads the counters and writes the file named by
    // `LLVM_PROFILE_FILE`, and takes no arguments.
    unsafe {
        let _ = __llvm_profile_write_file();
    }
}

#[cfg(not(coverage))]
fn write_coverage_profile() {}

#[cfg(target_os = "linux")]
static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Register an `on_exit` hook once all open decode sessions have closed so a
/// normal process exit flushes standard streams and ends through `_exit` when
/// `open_decode_sessions() == 0`, bypassing `libnvcuvid`'s duplicate thread
/// join in `_dl_fini`.
#[cfg(target_os = "linux")]
pub(crate) fn arm_after_decode_close() {
    if ARMED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn on_exit(
            function: extern "C" fn(libc::c_int, *mut libc::c_void),
            arg: *mut libc::c_void,
        ) -> libc::c_int;
    }
    #[allow(unsafe_code)]
    // SAFETY: `finish_if_all_sessions_closed` has C ABI, touches only atomic
    // counters and stdio flush before `_exit`, and does not unwind.
    unsafe {
        let _ = on_exit(finish_if_all_sessions_closed, std::ptr::null_mut());
    }
}

#[cfg(target_os = "linux")]
extern "C" fn finish_if_all_sessions_closed(status: libc::c_int, _arg: *mut libc::c_void) {
    if crate::open_decode_sessions() == 0 {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        write_coverage_profile();
        #[allow(unsafe_code)]
        // SAFETY: every decode session is closed; `_exit` preserves
        // `status` and skips `libnvcuvid`'s `.fini_array` destructor.
        unsafe {
            libc::_exit(status);
        }
    }
}
#[cfg(all(test, target_os = "linux"))]
#[path = "exit_tests.rs"]
mod tests;
