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

#[cfg(all(test, target_os = "linux"))]
#[path = "exit_tests.rs"]
mod tests;
