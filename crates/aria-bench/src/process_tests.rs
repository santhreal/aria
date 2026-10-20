//! Private `Xvfb` display selection and shutdown.
//!
//! WHY: `xdpyinfo` reports nothing on a display whose lock file was left by
//! another user's `Xvfb` that is no longer running, yet `Xvfb` exits on that
//! display because it cannot remove the lock. `start_private_xvfb` once tried
//! only the first display `xdpyinfo` found empty, so `startup` fell back to
//! the in-process measurement on such a host. The guard also stopped the
//! server with `SIGKILL`, which leaves such a lock behind. This test
//! holds a display with a lock naming PID 1, which `Xvfb` reports as an
//! active server, checks the server starts on another display that answers,
//! and checks dropping the guard removes that server's lock within 1 s.
//!
//! Not covered: hosts without `Xvfb` or `xdpyinfo` skip the test, the
//! lock-file fallback used when `xdpyinfo` cannot run, and the `SIGKILL`
//! fallback for a server that ignores `SIGTERM`.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{display_unanswered, start_private_xvfb};

/// A lock file this test created, removed on drop so a failing assertion
/// leaves no display held.
struct HeldDisplay {
    num: u32,
    lock: PathBuf,
}

impl Drop for HeldDisplay {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock);
    }
}

/// Whether a file named `program` is in a `PATH` directory, found without
/// starting it.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// Create the X lock file of the first unanswered display in `:91..=:99`
/// that has none, naming PID 1 in the X lock format.
fn hold_first_free_display() -> Option<HeldDisplay> {
    (91..=99)
        .filter(|&num| display_unanswered(num))
        .find_map(|num| {
            let lock = PathBuf::from(format!("/tmp/.X{num}-lock"));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
                .ok()?;
            let held = HeldDisplay { num, lock };
            file.write_all(format!("{:>10}\n", 1).as_bytes()).ok()?;
            Some(held)
        })
}

#[test]
fn a_display_xvfb_rejects_is_skipped_and_a_stopped_server_frees_its_display() {
    if !on_path("Xvfb") || !on_path("xdpyinfo") {
        eprintln!("skipping private Xvfb test: Xvfb or xdpyinfo is not installed");
        return;
    }
    let held = hold_first_free_display().expect("a free display in :91..=:99 to hold");
    let held_display = format!(":{}", held.num);

    let xvfb = start_private_xvfb()
        .unwrap_or_else(|| panic!("Xvfb must start on a display after the held {held_display}"));

    assert_ne!(
        xvfb.display, held_display,
        "the held display has an active server lock"
    );
    let started: u32 = xvfb.display[1..]
        .parse()
        .expect("the guard's display is `:<number>`");
    assert!(
        !display_unanswered(started),
        "the started server on {} must accept connections",
        xvfb.display
    );

    let started_lock = format!("/tmp/.X{started}-lock");
    assert!(
        Path::new(&started_lock).exists(),
        "a running Xvfb holds {started_lock}"
    );
    let stopping = Instant::now();
    drop(xvfb);
    let stopped_in = stopping.elapsed();
    assert!(
        stopped_in < Duration::from_secs(1),
        "Xvfb exits on SIGTERM in milliseconds, so the guard must return once it \
         exits rather than at its 2 s fallback; stopping took {stopped_in:?}"
    );
    assert!(
        !Path::new(&started_lock).exists(),
        "a stopped Xvfb must remove {started_lock}, or the display stays held"
    );
}
