//! Handing a path to another application on the desktop.
//!
//! One owner, **off until the shipped editor turns it on**. [`open_folder`] is
//! the only place in the crate that spawns a process, and it spawns nothing
//! until [`allow_desktop_launch`] has been called — which happens once, inside
//! [`crate::run_app_with`], the function that opens the window.
//!
//! The direction matters. A guard a test opts out of opens a window the first
//! time a new test forgets, and that is exactly what happened: the menu reach
//! test runs every menu item, "Open data dir" reveals a folder, and a suite on
//! a workstation opened a file manager on its desktop. A capability
//! the *application* switches on cannot be forgotten by a test, because a test
//! never runs the application.
//!
//! A disabled process still answers the caller honestly: the request is
//! recorded as the last one asked for, so a test asserts what would have been
//! opened, and [`open_folder`] returns `Ok` because refusing would make the
//! shipped status line depend on whether launching is enabled.

use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this process may start another application.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The folder the last reveal asked for. One slot, not a log: a shipped session
/// reveals once per click, and a growing list would be a leak
/// for the sake of a test.
static LAST: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Let this process open folders in the desktop's file manager.
///
/// Called by the headed editor at startup and by nothing else.
pub fn allow_desktop_launch() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether a reveal reaches the desktop.
pub fn desktop_launch_allowed() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The program that shows a folder on this platform.
pub fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    }
}

/// Show `folder` in the platform file manager.
///
/// The error is the spawn's, phrased with the program that failed, so a machine
/// without a file manager says so instead of reporting a missing folder.
pub fn open_folder(folder: &Path) -> Result<(), String> {
    *LAST.lock() = Some(folder.to_path_buf());
    if !desktop_launch_allowed() {
        return Ok(());
    }
    let opener = file_manager();
    spawn_reaped(std::process::Command::new(opener).arg(folder))
        .map(|_| ())
        .map_err(|err| format!("could not open {opener}: {err}"))
}

/// Start `command` and wait for it on a thread of its own, so a program that
/// exits leaves no zombie in the process table for the rest of the session.
/// Returns the child's process id.
///
/// A wait thread that cannot start costs one zombie, not the launch: the
/// program is already running, so the call still succeeds.
fn spawn_reaped(command: &mut std::process::Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    let _ = std::thread::Builder::new()
        .name("aria-launch-wait".to_owned())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(pid)
}

/// The folder the last reveal asked for, if any.
pub fn last_folder_asked_for() -> Option<PathBuf> {
    LAST.lock().as_deref().map(Path::to_path_buf)
}

/// Forget the last request, so a test can assert that the next call is the one
/// it drove rather than a leftover from an earlier case.
pub fn forget_last_folder() {
    *LAST.lock() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default is the whole point: a process that never asks is a process
    /// that cannot open a window, whatever its tests do.
    #[test]
    fn launching_is_off_until_the_application_asks() {
        assert!(
            !desktop_launch_allowed(),
            "a test process must not be allowed to start another application"
        );
    }

    /// A disabled reveal is still a reveal the caller can see: the folder is
    /// recorded, and the call succeeds so the status line reads the same.
    #[test]
    fn a_disabled_reveal_records_the_folder_it_would_open() {
        forget_last_folder();
        let folder = crate::scratch_pool::scratch("launch-record");
        assert_eq!(open_folder(&folder), Ok(()));
        assert_eq!(last_folder_asked_for().as_deref(), Some(folder.as_path()));
    }

    /// A program the editor starts is waited for once it exits. Before, the
    /// file manager was spawned and never waited for, so every reveal left a
    /// zombie in the process table until the editor quit. `true` stands in for
    /// the file manager, which exits as soon as it hands the folder over. Not
    /// caught: a program that never exits, which is not a zombie.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_launched_program_leaves_no_zombie_once_it_exits() {
        let pid = spawn_reaped(&mut std::process::Command::new("true")).expect("start true");
        let stat = format!("/proc/{pid}/stat");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while let Ok(entry) = std::fs::read_to_string(&stat) {
            assert!(
                std::time::Instant::now() < deadline,
                "the launched program was never waited for: {entry}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
