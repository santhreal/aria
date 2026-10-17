//! Contracts for the jobs board.
//!
//! The class these close is "a long operation runs with no way to see it or stop it,
//! and the panel invents its own view of the registry". So what is asserted is the
//! projection and the refusals, not the threading, which `aria-jobs` already tests:
//!
//! - Status is derived from the pair (state, cancel_requested). A cancel that has been
//!   accepted but whose thread has not exited must read differently from Running, or a
//!   job the operator already stopped looks stuck and gets stopped again.
//! - A terminal row stops offering Cancel, and asking anyway is reported. A Cancel that
//!   silently does nothing is the worse surface.
//! - Newest first. The row an operator just created must not be below sixty finished
//!   ones.
//! - Export refuses an empty timeline by name instead of spawning a job that fails a
//!   minute later.
//!
//! The board's registry entries (Window menu, palette) are enumerated from the same
//! tables the chrome renders, so removing the entry fails here.

use std::time::{Duration, Instant};

use aria_jobs::{JobCompletion, JobId, JobManager, JobOutput, JobSnapshot, JobState};
use gpui::AppContext;

use super::AppState;
use super::jobs::{JobKind, UiJob, file_label};
use super::premiere_tests::with_clip;
use aria_plugin::PluginContext;

fn snapshot(state: JobState, cancel_requested: bool) -> JobSnapshot {
    JobSnapshot {
        id: JobId::from_raw(3),
        label: "Exporting draft.mp4".into(),
        state,
        cancellable: !state.is_terminal(),
        progress: Some(0.5),
        detail: "frame 12/24".into(),
        outputs: Vec::new(),
        cancel_requested,
        started: Instant::now(),
        finished: state.is_terminal().then(Instant::now),
    }
}

/// Wait for a job to reach a terminal state without spinning: the registry is the
/// authority, and a test that asserted on a Running snapshot would be a flake.
fn settle(manager: &JobManager, id: JobId) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = manager.get(id).expect("job is registered");
        if snapshot.state.is_terminal() {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "job never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn status_distinguishes_every_state_and_a_pending_cancel() {
    for (state, requested, expected) in [
        (JobState::Queued, false, "Queued"),
        (JobState::Running, false, "Running"),
        (JobState::Queued, true, "Cancelling…"),
        (JobState::Running, true, "Cancelling…"),
        (JobState::Done, false, "Done"),
        (JobState::Failed, false, "Failed"),
        (JobState::Cancelled, true, "Cancelled"),
        // A panic during cancellation lands as Failed with the request still set: the
        // terminal state wins, because "Cancelling…" on a finished row never clears.
        (JobState::Failed, true, "Failed"),
        (JobState::Done, true, "Done"),
    ] {
        let job = UiJob::from_snapshot(&snapshot(state, requested));
        assert_eq!(
            job.status, expected,
            "{state:?} cancel_requested={requested}"
        );
        assert_eq!(job.terminal, state.is_terminal());
    }
}

#[test]
fn indeterminate_progress_survives_the_projection() {
    let mut raw = snapshot(JobState::Running, false);
    raw.progress = None;
    // Collapsing None to 0.0 would render a determinate bar stuck at the left edge,
    // which reads as a broken job rather than an unmeasurable one.
    assert_eq!(UiJob::from_snapshot(&raw).progress, None);
}

/// Whether a job offers a stop is the job's rule, and every surface reads it.
///
/// The class closed here is a surface offering a control the registry would
/// refuse — and its mirror, a running job with no way to stop it. Every state
/// the registry can publish is enumerated, including the one that matters most:
/// a terminal snapshot that still claims to be cancellable, which is what a
/// bug in the boundary looks like from here.
#[test]
fn a_job_offers_a_stop_exactly_while_the_registry_would_accept_one() {
    for state in [
        JobState::Queued,
        JobState::Running,
        JobState::Done,
        JobState::Failed,
        JobState::Cancelled,
    ] {
        for requested in [false, true] {
            let job = UiJob::from_snapshot(&snapshot(state, requested));
            assert_eq!(
                job.stoppable(),
                !state.is_terminal(),
                "{state:?} cancel_requested={requested}"
            );
        }

        // Past the commit boundary the registry refuses, whatever the state.
        let mut committed = snapshot(state, false);
        committed.cancellable = false;
        assert!(
            !UiJob::from_snapshot(&committed).stoppable(),
            "{state:?} offered a stop past the commit boundary"
        );

        // A terminal snapshot that still claims cancellability must not put the
        // control on a finished row.
        let mut stale = snapshot(state, false);
        stale.cancellable = true;
        assert_eq!(
            UiJob::from_snapshot(&stale).stoppable(),
            !state.is_terminal(),
            "{state:?} trusted a stale cancellable flag"
        );
    }
}

#[test]
fn outputs_are_projected_as_plain_pairs() {
    let mut raw = snapshot(JobState::Done, false);
    raw.outputs = vec![JobOutput::new("path", "/tmp/out.mp4").expect("valid output")];
    let job = UiJob::from_snapshot(&raw);
    assert_eq!(
        job.outputs,
        vec![("path".to_string(), "/tmp/out.mp4".to_string())]
    );
}

#[test]
fn rows_are_newest_first_and_running_is_counted() {
    let (mut state, _clip) = with_clip();
    let manager = state.jobs.clone();
    let first = manager.spawn("first", |_| Ok(String::from("done")));
    settle(&manager, first);
    let second = manager.spawn("second", |_| Ok(String::from("done")));
    settle(&manager, second);

    // What the subscriber's wake does when a job publishes.
    state.refresh_jobs();
    let rows = state.job_view.clone();
    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "first"]
    );
    assert_eq!(state.running_job_count(), 0);
    assert!(rows.iter().all(|row| !row.cancellable));
}

#[test]
fn cancelling_a_finished_job_is_reported_not_swallowed() {
    let (mut state, _clip) = with_clip();
    let manager = state.jobs.clone();
    let id = manager.spawn("already done", |_| Ok(String::from("done")));
    settle(&manager, id);

    state.cancel_job(id.raw());
    assert_eq!(state.status_message.as_deref(), Some("Too late to cancel"));

    // An id the registry never issued is the same refusal, not a panic.
    state.status_message = None;
    state.cancel_job(9_999_999);
    assert_eq!(state.status_message.as_deref(), Some("Too late to cancel"));
}

/// An export sheet refuses in place rather than spawning a job that dies on its
/// first frame, and it refuses in the render path's own words: the rule has one
/// owner, so the words are read from it rather than retyped here.
#[test]
fn export_refuses_an_empty_timeline_before_spawning() {
    let dir = crate::scratch_pool::scratch("jobs");
    let out = dir.join("empty-export.mp4");
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.new_project().expect("new project");
    let before = state.job_view.len();

    let err = state
        .export_job(out)
        .expect_err("an empty timeline cannot export");
    assert_eq!(
        err,
        aria_render::RenderError::Empty(aria_render::EmptyExport::Sequence).to_string(),
        "the refusal must be the one the render would raise"
    );
    // The refusal must not leave a job row behind: a Failed export nobody asked for
    // reads as a broken renderer.
    assert_eq!(state.job_view.len(), before);
    assert_eq!(state.status_message, None);
}

#[test]
fn a_job_label_names_the_file_not_the_path() {
    assert_eq!(
        file_label(std::path::Path::new("/a/b/draft.mp4")),
        "draft.mp4"
    );
    assert_eq!(file_label(std::path::Path::new("draft.mp4")), "draft.mp4");
    // A path with no filename still has to label something.
    assert_eq!(file_label(std::path::Path::new("/")), "/");
}

#[test]
fn reveal_refuses_a_path_it_cannot_open() {
    let (mut state, _clip) = with_clip();
    // The import that built the fixture already wrote a status line; a refusal must not
    // overwrite it with a success-shaped message.
    let before = state.status_message.clone();

    // A bare filename names no directory. Falling back to the working directory would
    // open somewhere the artifact is not.
    state.reveal_path("draft.mp4");
    let err = state.last_error.take().expect("a bare name is refused");
    assert!(err.contains("no folder to open"), "{err}");
    assert_eq!(state.status_message, before);

    // A path whose directory has since been deleted is named, not silently ignored.
    state.reveal_path("/nonexistent-aria-dir-9f2/out.mp4");
    let err = state
        .last_error
        .take()
        .expect("a missing folder is refused");
    assert!(err.contains("the folder is gone"), "{err}");
    assert_eq!(state.status_message, before);
}

/// The status line projects the time left from the rate so far, and only
/// when there is a rate to project.
///
/// The class closed here is an estimate drawn from nothing: a queued job, a
/// job that reports no fraction, one that has reported almost none of it, a
/// finished one, and one being stopped all show the time spent and no more.
/// A fraction at or past the end has nothing left to estimate either.
#[test]
fn the_status_line_estimates_time_left_only_for_measured_running_work() {
    let job = |status: &'static str, progress: Option<f32>, elapsed: f32, terminal: bool| UiJob {
        id: 1,
        label: "Exporting draft.mp4".into(),
        kind: JobKind::Export,
        status,
        progress,
        detail: "".into(),
        cancellable: !terminal,
        terminal,
        outputs: Vec::new(),
        elapsed,
    };
    // 12.4 s for a quarter: three more quarters at that rate is 37.2 s,
    // rounded up so the estimate never promises sooner than the rate says.
    let running = job("Running", Some(0.25), 12.4, false);
    assert_eq!(running.status_line(), "Running · 12.4s · 38s left");
    assert_eq!(
        job("Running", Some(0.5), 3.0, false).status_line(),
        "Running · 3.0s · 3s left"
    );
    for (status, progress, terminal) in [
        ("Queued", Some(0.25), false),
        ("Running", None, false),
        ("Running", Some(0.0), false),
        ("Running", Some(0.005), false),
        ("Running", Some(1.0), false),
        ("Cancelling…", Some(0.25), false),
        ("Done", Some(0.25), true),
        ("Failed", Some(0.25), true),
    ] {
        let line = job(status, progress, 12.4, terminal).status_line();
        assert_eq!(line, format!("{status} · 12.4s"), "{status} {progress:?}");
    }
    assert_eq!(
        job("Running", Some(0.5), 0.0, false).status_line(),
        "Running · 0.0s",
        "no time spent is no rate"
    );
}

/// Window > Jobs docks the board beside the editor rather than over it, so a
/// render can be watched while the edit goes on, and a second press closes it.
/// The check mark reads the dock, so it never claims a panel that is closed.
#[test]
fn window_jobs_toggles_the_docked_board() {
    let (state, _clip) = with_clip();
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    let press = || {
        cx.update(|app| {
            entity.update(app, |state, cx| {
                let row = crate::chrome::menu_items::items_of(crate::types::AppMenu::Window, state)
                    .into_iter()
                    .find(|item| item.id == "win-jobs")
                    .expect("Window > Jobs");
                (row.run)("win-jobs", state, cx);
                let checked = row.checked.expect("a panel row is checkable");
                (
                    state.dock_tree.is_open(crate::dock::PANEL_JOBS),
                    checked("win-jobs", state),
                    state.dialog,
                )
            })
        })
    };
    assert_eq!(
        press(),
        (true, true, super::DialogKind::None),
        "the first press"
    );
    assert_eq!(
        press(),
        (false, false, super::DialogKind::None),
        "the second press"
    );
}

#[test]
fn the_board_is_reachable_from_the_window_menu_and_the_palette() {
    let (state, _clip) = with_clip();
    assert!(
        crate::chrome::labels_for(crate::types::AppMenu::Window, &state)
            .iter()
            .any(|label| label.contains("Jobs")),
        "Window menu lost the jobs board"
    );
    assert!(
        crate::commands::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.id == "pal-jobs"),
        "palette lost the jobs board"
    );
}

/// A job publishes once: the guard wins the boundary, and a second attempt is refused
/// by name rather than writing twice.
///
/// The class closed here is "the registry's boundary is idempotent, so a caller that
/// gained a second write path publishes twice and nothing says so". `try_begin_commit`
/// returning `true` again is correct for the registry; the caller-side rule is what this
/// asserts.
#[test]
fn a_writing_job_commits_once_and_refuses_a_second_commit() {
    let manager = JobManager::new();
    let (tx, rx) = crossbeam_channel::bounded::<Result<(), String>>(2);
    let id = manager.spawn("writing", move |context| {
        let mut guard = super::jobs::CommitGuard::new(context, "the batch");
        tx.send(guard.begin()).expect("report the first attempt");
        assert!(guard.committed(), "the first commit did not take");
        tx.send(guard.begin()).expect("report the second attempt");
        // The registry's own boundary is idempotent, which is why the guard has to
        // carry the once-only rule: delete the guard's `taken` flag and this is what a
        // second publication would see.
        assert!(
            context.try_begin_commit(),
            "the registry stopped being idempotent; the guard is now the only rule"
        );
        Ok(String::from("written"))
    });
    let first = rx.recv_timeout(Duration::from_secs(10)).expect("first");
    assert_eq!(first, Ok(()), "the first commit lost the boundary");
    let second = rx.recv_timeout(Duration::from_secs(10)).expect("second");
    assert_eq!(
        second,
        Err("the batch was already committed".to_string()),
        "a second commit was allowed"
    );
    assert_eq!(settle(&manager, id).state, JobState::Done);
}

/// A cancel accepted before the boundary refuses publication, and the refusal names what
/// was not written. Past the boundary a cancel no longer lands, so the same job cannot
/// both publish and read as cancelled.
#[test]
fn a_cancel_before_the_boundary_stops_the_write_and_after_it_does_not() {
    let manager = JobManager::new();
    let (report, outcome) = crossbeam_channel::bounded::<Result<(), String>>(1);
    let (ready, at_boundary) = crossbeam_channel::bounded::<()>(1);
    let (release, wait) = crossbeam_channel::bounded::<()>(1);
    let id = manager.spawn("writing", move |context| {
        ready.send(()).expect("announce the boundary");
        wait.recv().expect("wait for the cancel");
        let mut guard = super::jobs::CommitGuard::new(context, "the batch");
        let result = guard.begin();
        report.send(result.clone()).expect("report");
        result.map(|()| String::from("written"))
    });
    at_boundary
        .recv_timeout(Duration::from_secs(10))
        .expect("the job started");
    assert!(manager.cancel(id), "a cancellable job refused the cancel");
    release.send(()).expect("let the job try to commit");
    let refused = outcome
        .recv_timeout(Duration::from_secs(10))
        .expect("result");
    assert_eq!(
        refused,
        Err("cancelled before the batch was written".to_string()),
        "the write went ahead after a cancel"
    );
    assert_eq!(settle(&manager, id).state, JobState::Cancelled);

    // The other side of the boundary: once it is won, cancel is rejected.
    let (ready, at_boundary) = crossbeam_channel::bounded::<()>(1);
    let (release, wait) = crossbeam_channel::bounded::<()>(1);
    let committed = manager.spawn("writing", move |context| {
        let mut guard = super::jobs::CommitGuard::new(context, "the batch");
        guard.begin().expect("win the boundary");
        ready.send(()).expect("announce");
        wait.recv().expect("wait");
        Ok(String::from("written"))
    });
    at_boundary
        .recv_timeout(Duration::from_secs(10))
        .expect("the job committed");
    assert!(
        !manager.cancel(committed),
        "a cancel was accepted past the commit boundary"
    );
    release.send(()).expect("let the job finish");
    assert_eq!(settle(&manager, committed).state, JobState::Done);
}

/// **One module hands work to the registry, and the board is projected there.**
///
/// The board used to be built inside a draw: `snapshot` takes the registry's
/// mutex and clones every job's label, detail and outputs, and a frame with the
/// pending strip up did that four times while the job threads writing progress
/// contended for the same lock. It is a projection now, refreshed where the list
/// changes — and that only holds if every producer goes through the one owner
/// that refreshes it. A job registered directly is a job the operator does not
/// see until something else happens to publish, so a new producer that reaches
/// past `AppState::spawn_job` is red here rather than mysteriously slow to
/// appear.
#[test]
fn only_one_module_registers_a_job() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    crate::test_support::collect_rs(&src, &mut files);
    let mut reaches = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(&src)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        // `jobs.rs` is the owner; the export queue is a producer of its own,
        // holding the one running pass and publishing it, and it is what
        // `enqueue_export` refreshes around. Tests drive the registry directly
        // on purpose: that is what a producer thread does.
        if name == "app/jobs.rs" || name == "app/export_queue.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") || code.starts_with("///") {
                continue;
            }
            if code.contains("spawn_with_completion(") || code.contains("jobs.spawn(") {
                reaches.push(format!("{name}:{}: {code}", index + 1));
            }
        }
    }
    assert!(
        reaches.is_empty(),
        "{} producer(s) register a job without projecting the board — go through \
         `AppState::spawn_job`:\n  {}",
        reaches.len(),
        reaches.join("\n  ")
    );
}

/// **A progress report reaches the board the panel draws.**
///
/// The board is a projection now, so "the panel reads the registry every frame"
/// is no longer what keeps a progress bar moving: the subscriber's wake has to
/// rebuild it. This drives the real path — a job on its own thread reporting
/// half way through, the wake landing on the window's thread — and reads what a
/// draw would read. Without the refresh in that wake the row stays at the
/// progress it was registered with, which is a bar that never moves.
#[gpui::test]
async fn a_progress_report_reaches_the_board(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (state, _clip) = with_clip();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| entity.update(app, |state: &mut AppState, cx| state.watch_jobs(cx)));

    let (ready, reported) = crossbeam_channel::bounded::<()>(1);
    let (release, wait) = crossbeam_channel::bounded::<()>(1);
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, _| {
            state.spawn_job("measuring", move |context| {
                context.set_progress(0.5, "half way");
                ready.send(()).expect("announce the report");
                wait.recv().expect("hold the job open");
                Ok(JobCompletion::new("done"))
            });
        })
    });
    // The row exists in the update that started the work: the operator pressed a
    // button, and the wake is a scheduler hop away.
    assert!(
        cx.update(|app| {
            entity
                .read(app)
                .job_view
                .iter()
                .any(|row| row.label == "measuring")
        }),
        "starting the work left the board empty"
    );
    reported
        .recv_timeout(Duration::from_secs(10))
        .expect("the job reported its progress");
    cx.run_until_parked();

    let row = cx.update(|app| {
        entity
            .read(app)
            .job_view
            .iter()
            .find(|row| row.label == "measuring")
            .map(|row| (row.progress, row.detail.clone()))
    });
    assert_eq!(row, Some((Some(0.5), "half way".into())));
    release.send(()).expect("let the job finish");
}

/// A running job is drawn at the centre of the status bar, and a click on it
/// brings the Jobs panel forward with the board drawn in it.
#[gpui::test]
fn a_running_job_in_the_status_bar_opens_the_jobs_panel(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "status-job");
    let (release, wait) = crossbeam_channel::bounded::<()>(1);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.spawn_job("Exporting held.mp4", move |_| {
                let _ = wait.recv();
                Ok(JobCompletion::new("done"))
            });
            cx.notify();
        })
    });
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("jobs-panel").is_none(),
        "the Jobs panel starts closed"
    );
    let job = visual
        .debug_bounds("status-job")
        .expect("the running job is not drawn in the status bar");
    visual.simulate_click(job.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).dock_tree.is_active(crate::dock::PANEL_JOBS)),
        "the click did not bring the Jobs panel forward"
    );
    assert!(
        visual.debug_bounds("jobs-panel").is_some(),
        "the Jobs panel is not drawn"
    );
    release.send(()).expect("let the job finish");
}
