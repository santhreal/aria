//! Handing a path to another application on the operator's desktop.
//!
//! One owner, **off until the shipped editor turns it on**. [`open_folder`] is
//! the only place in the crate that spawns a process, and it spawns nothing
//! until [`allow_desktop_launch`] has been called — which happens once, inside
//! [`crate::run_app_with`], the function that opens the window.
//!
//! The direction matters. A guard a test opts out of opens a window the first
//! time a new test forgets, and that is exactly what happened: the menu reach
//! test runs every menu item, "Open data dir" reveals a folder, and a suite on
//! the operator's machine opened a file manager on their desktop. A capability
//! the *application* switches on cannot be forgotten by a test, because a test
//! never runs the application.
//!
//! A disabled process still answers the caller honestly: the request is
//! recorded as the last one asked for, so a test asserts what would have been
//! opened, and [`open_folder`] returns `Ok` because refusing would make the
//! shipped status line depend on whether launching is enabled.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

/// Whether this process may start another application.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The folder the last reveal asked for. One slot, not a log: a shipped session
/// reveals as often as the operator clicks, and a growing list would be a leak
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
    *LAST.lock().unwrap_or_else(PoisonError::into_inner) = Some(folder.to_path_buf());
    if !desktop_launch_allowed() {
        return Ok(());
    }
    let opener = file_manager();
    std::process::Command::new(opener)
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open {opener}: {err}"))
}

/// The folder the last reveal asked for, if any.
pub fn last_folder_asked_for() -> Option<PathBuf> {
    LAST.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_deref()
        .map(Path::to_path_buf)
}

/// Forget the last request, so a test can assert that the next call is the one
/// it drove rather than a leftover from an earlier case.
pub fn forget_last_folder() {
    *LAST.lock().unwrap_or_else(PoisonError::into_inner) = None;
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

    /// The program is the platform's, named once. A caller picking its own would
    /// be a second owner, which is how the two spawn sites drifted apart.
    #[test]
    fn one_program_shows_a_folder_on_this_platform() {
        let expected = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "explorer"
        } else {
            "xdg-open"
        };
        assert_eq!(file_manager(), expected);
    }
}
