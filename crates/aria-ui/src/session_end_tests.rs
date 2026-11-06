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

#[gpui::test]
fn every_way_a_session_ends_closes_decode_sessions(cx: &mut TestAppContext) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4");
    for (index, ending) in Ending::ALL.into_iter().enumerate() {
        let at = Bounds::new(point(px(120.), px(80.)), size(px(1280.), px(760.)));
        let (state, mut visual) = drawn_shell_at(cx, &format!("session-end-decode-{index}"), at);
        let video = visual.update(|_, app| {
            let dir = state.read(app).data_dir.clone();
            let copy = dir.join(format!("end-{index}.mp4"));
            std::fs::copy(&fixture, &copy).expect("copy video fixture");
            state.update(app, |state, cx| {
                state.new_project().expect("new project");
                let media = state.import_media(copy.clone()).expect("import video");
                state.project_path = Some(dir.join(format!("end-{index}.aria")));
                state.save_project().expect("save clean session");
                state
                    .engine()
                    .lock()
                    .media_frame_fit(media, 0, 160, 90)
                    .expect("decode first frame");
                let engine = std::sync::Arc::clone(state.engine());
                crate::handoff::on_a_thread(
                    cx.background_executor(),
                    "panels::sidebar",
                    move || {
                        let _ = engine.lock().media_frame_fit(media, 1, 160, 90);
                    },
                )
                .detach();
            });
            copy
        });
        assert_eq!(
            aria_decoder::open_decode_sessions_for(&video),
            1,
            "{ending:?}: video decode session was not open before ending the session"
        );
        match ending {
            Ending::Quit => {
                visual.update(|_, app| state.update(app, |state, _| state.request_quit()));
                visual.update(|window, _| window.refresh());
                visual.run_until_parked();
            }
            Ending::CloseRequest => {
                visual.update(|window, app| super::close_on_request(window, &state, app));
                assert!(visual.simulate_close());
            }
        }
        assert_eq!(
            aria_decoder::open_decode_sessions_for(&video),
            0,
            "{ending:?}: video decode session stayed open after ending the session"
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

/// A dropped session cancels its jobs and waits for their threads: a job
/// thread still drawing when the process exits races the GPU driver's exit
/// handlers and ends the process in SIGSEGV. The job here finishes 300 ms of
/// work after the cancel, as a device bring-up or a renderer's drop does, and
/// has finished when `drop` returns. Not caught: a job that outlasts
/// [`super::JOB_SHUTDOWN`].
#[test]
fn dropping_app_state_cancels_and_settles_in_flight_jobs() {
    let (state, _) = crate::app::premiere_tests::with_clip();
    let jobs = state.jobs.clone();
    let epoch_before = jobs.project_epoch();
    let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = std::sync::Arc::clone(&finished);
    let job_id = state.jobs.spawn("background-job", move |ctx| {
        while !ctx.cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        done.store(true, std::sync::atomic::Ordering::Release);
        Err("cancelled".into())
    });
    drop(state);
    assert!(
        jobs.project_epoch() > epoch_before,
        "dropping AppState must advance the project epoch"
    );
    assert!(
        finished.load(std::sync::atomic::Ordering::Acquire),
        "the job was still running when the dropped session returned"
    );
    assert!(
        jobs.settle_all(std::time::Duration::ZERO),
        "a job thread outlived the dropped session"
    );
    let snapshot = jobs
        .get(job_id)
        .expect("spawned job stays in registry history");
    assert_eq!(snapshot.state, aria_jobs::JobState::Cancelled);
}
