//! Every way a session ends saves the main window's placement.
//!
//! The class closed here: an ending that does not record where the window was.
//! The placement was saved only by the window manager's close request, so a
//! window moved, resized or maximized and then closed with Ctrl+Q, File > Quit
//! or the close control reopened where it had been before. Each ending below
//! runs the production path: the shell's draw after `AppState::request_quit`,
//! and the handler [`super::close_on_request`] installs, driven by the test
//! platform's close request. The census beside it holds every quit in the crate
//! to [`super::quit`], so a new way of quitting is covered by the run that adds
//! it.
//!
//! Not covered: the maximized flag, which the test platform's window does not
//! model (its bounds are always windowed), and a dirty session, whose ending
//! waits on the unsaved prompt `app::unsaved_tests` covers.

use std::path::Path;

use gpui::{Bounds, TestAppContext, point, px, size};

use crate::settings::{UiSettings, WindowPlacement};
use crate::test_support::control_sites::strip_comments;
use crate::test_support::sources::without_test_items;
use crate::test_support::{collect_rs, drawn_shell_at};

/// How a test ends the session.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// Ctrl+Q, File > Quit and the close control: a quit request the shell's
    /// next draw answers.
    Quit,
    /// The window manager's close request.
    CloseRequest,
}

impl Ending {
    const ALL: [Self; 2] = [Self::Quit, Self::CloseRequest];
}

#[gpui::test]
fn every_way_a_session_ends_saves_the_window_placement(cx: &mut TestAppContext) {
    for (index, ending) in Ending::ALL.into_iter().enumerate() {
        let x = 120.0 + 40.0 * index as f32;
        let at = Bounds::new(point(px(x), px(80.)), size(px(1280.), px(760.)));
        let (state, mut visual) = drawn_shell_at(cx, &format!("session-end-{index}"), at);
        let data_dir = visual.update(|_, app| state.read(app).data_dir.clone());
        assert!(
            UiSettings::load(&data_dir).window_placements.is_empty(),
            "{ending:?}: a placement was saved before the session ended"
        );
        match ending {
            Ending::Quit => {
                visual.update(|_, app| state.update(app, |state, _| state.request_quit()));
                assert!(
                    visual.update(|_, app| state.read(app).quit_requested()),
                    "{ending:?}: a clean session did not quit at once"
                );
                visual.update(|window, _| window.refresh());
                visual.run_until_parked();
            }
            Ending::CloseRequest => {
                visual.update(|window, app| super::close_on_request(window, &state, app));
                assert!(
                    visual.simulate_close(),
                    "{ending:?}: a clean session did not close at once"
                );
            }
        }
        let saved = UiSettings::load(&data_dir);
        let placements: Vec<WindowPlacement> = saved.window_placements.values().copied().collect();
        assert_eq!(
            placements,
            [WindowPlacement {
                x,
                y: 80.0,
                width: 1280.0,
                height: 760.0,
                maximized: false,
                display_index: 0,
            }],
            "{ending:?}: the saved placement is not the window's"
        );
        assert!(
            saved
                .last_display_arrangement
                .is_some_and(|last| saved.window_placements.contains_key(&last)),
            "{ending:?}: the arrangement the placement was saved for is not the last one"
        );
    }
}

/// **Every quit in the crate goes through [`super::quit`].**
///
/// A quit anywhere else ends the session without saving the placement. Read
/// from the sources with test items and comments blanked, so a new call turns
/// this red on the run that adds it; and fails when no call is found at all,
/// so a renamed owner does not leave a rule that matches nothing.
#[test]
fn every_quit_in_the_crate_goes_through_session_end() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 200,
        "found {} files under {src:?}, which cannot be this crate",
        files.len()
    );
    let mut owner = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("test_support/") || rel.ends_with("_tests.rs") {
            continue;
        }
        let raw = std::fs::read_to_string(file)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
        let code = strip_comments(&without_test_items(&raw));
        for (number, line) in code.lines().enumerate() {
            if !line.contains(".quit()") {
                continue;
            }
            if rel == "session_end.rs" {
                owner += 1;
            } else {
                offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert_eq!(owner, 1, "session_end.rs no longer quits the app once");
    assert!(
        offenders.is_empty(),
        "a quit outside session_end::quit skips the placement save:\n  {}",
        offenders.join("\n  ")
    );
}
