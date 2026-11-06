//! How an editor session ends, and the window placement each ending saves.
//!
//! A session ends one of two ways. Ctrl+Q, File > Quit and the window's close
//! control set [`AppState::quit_requested`] once the unsaved prompt is answered,
//! and the shell's next draw calls [`quit`]. The window manager's close request
//! runs the handler [`close_on_request`] installs. Each saves the window's
//! placement before the session ends, so the next launch reopens the window at
//! the size, position and maximized state it closed with (`docs/spec/layout.md`).

use std::time::Duration;

use gpui::{App, Entity, Window, WindowBounds};

use crate::app::AppState;
use crate::settings::WindowPlacement;

/// How long a session end waits for its job threads after cancelling them.
///
/// A job thread still drawing when the process exits races the GPU driver's
/// exit handlers, which free the device under it: the process ends in SIGSEGV
/// instead of exit 0.
pub(crate) const JOB_SHUTDOWN: Duration = Duration::from_secs(5);

/// Stop background workers and close every video decode session held by `state`.
pub(crate) fn close_decode_sessions(state: &mut AppState) {
    state.playing = false;
    state.audio.publish_stop();
    state.preview_worker.stop();
    state.shutdown_jobs(JOB_SHUTDOWN);
    state.filmstrips = crate::filmstrips::FilmstripStore::default();
    state.engine().lock().new_session();
}

/// A session dropped without [`quit`] stops its preview worker and settles its
/// jobs the same way, so no thread of the session outlives it.
impl Drop for AppState {
    fn drop(&mut self) {
        self.shutdown_jobs(JOB_SHUTDOWN);
    }
}

/// Save the window's placement, close decode sessions, then quit the app.
pub(crate) fn quit(window: &Window, state: &Entity<AppState>, cx: &mut App) {
    state.update(cx, |state, cx| {
        save_placement(window, state, cx);
        close_decode_sessions(state);
    });
    cx.quit();
}

/// Route the window manager's close request through the unsaved prompt. A clean
/// session closes at once and saves the placement as it goes; a dirty one stays
/// open until the prompt is answered, and the answer ends it through [`quit`].
/// Only a window that stays open is redrawn, to draw the prompt: the shell's
/// draw of a closing window would end the session a second time.
pub(crate) fn close_on_request(window: &mut Window, state: &Entity<AppState>, cx: &mut App) {
    let closing = state.clone();
    window.on_window_should_close(cx, move |window, cx| {
        let closes = closing.update(cx, |state, _| {
            if !state.quit_requested() {
                state.request_quit();
            }
            state.quit_requested()
        });
        if closes {
            quit(window, &closing, cx);
        } else {
            closing.update(cx, |_, cx| cx.notify());
        }
        closes
    });
}

/// Record the placement of `window` for the display arrangement it is on, and
/// write it to the session's data directory for the next launch.
fn save_placement(window: &Window, state: &mut AppState, cx: &App) {
    let bounds = window.window_bounds();
    let inner = bounds.get_bounds();
    let maximized = matches!(
        bounds,
        WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_)
    );
    let displays: Vec<[f32; 4]> = cx
        .displays()
        .into_iter()
        .map(|display| {
            let b = display.bounds();
            [
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ]
        })
        .collect();
    let center_x = f32::from(inner.origin.x) + f32::from(inner.size.width) * 0.5;
    let center_y = f32::from(inner.origin.y) + f32::from(inner.size.height) * 0.5;
    let display_index = displays
        .iter()
        .position(|&[x, y, w, h]| {
            center_x >= x && center_x <= x + w && center_y >= y && center_y <= y + h
        })
        .unwrap_or(0);
    state.ui_settings.record_window_placement(
        &displays,
        WindowPlacement {
            x: f32::from(inner.origin.x),
            y: f32::from(inner.origin.y),
            width: f32::from(inner.size.width),
            height: f32::from(inner.size.height),
            maximized,
            display_index,
        },
    );
    // The session is ending, so a status line would never be read: the failure
    // goes to the log with the path to fix.
    if let Err(err) = state.write_settings_now() {
        tracing::warn!(
            dir = %state.data_dir.display(),
            "window placement not saved for the next launch: {err}"
        );
    }
}

#[cfg(test)]
#[path = "session_end_tests.rs"]
mod tests;
