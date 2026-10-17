//! Contracts for what the window posts to plugins.
//!
//! The class these close is "the editor changes something a plugin can observe
//! and the plugin is never told, or is told something the editor did not do":
//!
//! - A drawn frame records the view `EditorView` serves and posts each part
//!   that changed, with In and Out at the playhead's rate.
//! - A pool item the projection shows missing is posted once, and again only
//!   after it returns and goes missing a second time.
//! - Every registry transition reaches the bus as `JobProgress` or
//!   `JobFinished`.
//! - A render posts `RenderFinished` with the result the registry records for
//!   the same job, including a cancel accepted after the work returned.
//! - A context menu collects plugin rows in mount order, for the target it was
//!   opened on and over the session's project, and runs a clicked row off the
//!   window's thread.
//!
//! Not covered: that the shell calls `publish_view` on each frame and draws the
//! plugin rows. Both need a window.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aria_core::RationalTime;
use aria_jobs::{JobCompletion, JobId, JobState};
use aria_models::{ClipId, MediaId, Project, TrackId};
use aria_plugin::{
    ContextMenuRequested, EditorView, Event, EventBus, InOutChanged, JobFinished, JobOutcome,
    JobProgress, MediaOffline, MenuTarget, PlayheadMoved, Registration, RenderFinished, RenderKind,
    RenderResult, SelectionChanged,
};

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::ContextTarget;

const BOUND: Duration = Duration::from_secs(10);

/// Every `E` posted on `bus`, mapped by `seen`, in delivery order.
struct Recorder<T> {
    bus: Arc<EventBus>,
    seen: crossbeam_channel::Receiver<T>,
    _registration: Registration,
}

impl<T: Send + 'static> Recorder<T> {
    fn new<E: Event>(bus: &Arc<EventBus>, seen: fn(&E) -> T) -> Self {
        let (sent, received) = crossbeam_channel::unbounded();
        let registration = bus.subscribe::<E, _>(move |event| {
            let _ = sent.send(seen(event));
        });
        Self {
            bus: Arc::clone(bus),
            seen: received,
            _registration: registration,
        }
    }

    /// Everything delivered so far.
    fn take(&self) -> Vec<T> {
        assert!(
            self.bus.flush(BOUND),
            "delivery must finish within the bound"
        );
        self.seen.try_iter().collect()
    }

    /// The next delivery, within the bound.
    fn next(&self) -> T {
        self.seen
            .recv_timeout(BOUND)
            .expect("a notification within the bound")
    }
}

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .expect("a file name")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_drawn_frame_records_the_view_and_posts_each_changed_part() {
    let (mut state, clip) = with_clip();
    let bus = Arc::clone(state.ctx.event_bus());
    let selections = Recorder::new(&bus, |e: &SelectionChanged| e.clips.clone());
    let playheads = Recorder::new(&bus, |e: &PlayheadMoved| e.at);
    let marks = Recorder::new(&bus, |e: &InOutChanged| (e.mark_in, e.mark_out));
    let view = state
        .ctx
        .try_get::<EditorView>()
        .expect("the session serves EditorView");
    let rate = state.playhead.rate;

    state.publish_view();
    let first = view.current().expect("the first frame is recorded");
    assert_eq!(first.selection, [clip]);
    assert_eq!(first.playhead, state.playhead);
    assert!(
        selections.take().is_empty(),
        "the first frame posts nothing"
    );

    state.publish_view();
    assert!(
        playheads.take().is_empty(),
        "an unchanged frame posts nothing"
    );

    state.clear_selection();
    state.playhead = RationalTime::new(state.playhead.value + 12, rate);
    state.mark_in = Some(6);
    state.publish_view();
    assert_eq!(selections.take(), [Vec::<ClipId>::new()]);
    assert_eq!(playheads.take(), [state.playhead]);
    assert_eq!(
        marks.take(),
        [(Some(RationalTime::new(6, rate)), None)],
        "In and Out are posted at the playhead's rate"
    );
    let latest = view.current().expect("recorded");
    assert!(latest.selection.is_empty());
    assert_eq!(latest.mark_in, Some(RationalTime::new(6, rate)));
}

/// Stage a copy of the texture fixture named `name` in `dir`.
fn stage(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(texture(), &path).expect("stage a copy of the fixture");
    path
}

/// Import a fresh copy, so the pool is rebuilt and the missing flags re-read.
fn rebuild_pool(state: &mut AppState, dir: &Path, name: &str) {
    crate::projection::invalidate_media_statuses();
    state
        .import_media(stage(dir, name))
        .expect("the import that rebuilds the pool");
    state.sync_from_engine();
}

#[test]
fn a_missing_file_is_posted_once_and_again_after_it_returns() {
    let dir = crate::scratch_pool::scratch("media-offline");
    let (mut state, _) = with_clip();
    let bus = Arc::clone(state.ctx.event_bus());
    let offline = Recorder::new(&bus, |e: &MediaOffline| (e.media, name_of(&e.path)));
    let gone = stage(&dir, "gone.png");
    let card = state
        .import_media(gone.clone())
        .expect("import the staged file");
    assert!(offline.take().is_empty(), "a file on disk is not offline");

    std::fs::remove_file(&gone).expect("lose the file");
    rebuild_pool(&mut state, &dir, "second.png");
    assert_eq!(offline.take(), [(card, "gone.png".to_string())]);

    rebuild_pool(&mut state, &dir, "third.png");
    assert!(
        offline.take().is_empty(),
        "a file still missing is not posted twice"
    );

    std::fs::copy(texture(), &gone).expect("restore the file");
    rebuild_pool(&mut state, &dir, "fourth.png");
    assert!(
        offline.take().is_empty(),
        "a file that returned is not offline"
    );

    std::fs::remove_file(&gone).expect("lose the file again");
    rebuild_pool(&mut state, &dir, "fifth.png");
    assert_eq!(
        offline.take(),
        [(card, "gone.png".to_string())],
        "losing a file that returned is posted again"
    );
}

#[derive(Debug, PartialEq)]
enum Board {
    Progress(u64, Option<f32>, String),
    Finished(u64, String, JobOutcome, String),
}

#[test]
fn every_job_transition_reaches_the_bus() {
    let (mut state, _) = with_clip();
    let bus = Arc::clone(state.ctx.event_bus());
    let (sent, board) = crossbeam_channel::unbounded();
    let progress = {
        let sent = sent.clone();
        bus.subscribe::<JobProgress, _>(move |e| {
            let _ = sent.send(Board::Progress(e.job, e.progress, e.detail.clone()));
        })
    };
    let finished = bus.subscribe::<JobFinished, _>(move |e| {
        let _ = sent.send(Board::Finished(
            e.job,
            e.label.clone(),
            e.outcome,
            e.detail.clone(),
        ));
    });

    let (cancel_seen, cancel_wait) = crossbeam_channel::bounded::<()>(1);
    let cases: [(&str, JobOutcome, &str); 3] = [
        ("measuring", JobOutcome::Done, "done"),
        ("failing", JobOutcome::Failed, "disk full"),
        ("stopping", JobOutcome::Cancelled, "stopped"),
    ];
    for (label, outcome, detail) in cases {
        let ready = cancel_seen.clone();
        let id = state.spawn_job(label, move |context| {
            context.set_progress(0.5, "half way");
            match outcome {
                JobOutcome::Done => Ok(JobCompletion::new("done")),
                JobOutcome::Failed => Err("disk full".to_string()),
                JobOutcome::Cancelled => {
                    ready.send(()).expect("announce the wait");
                    let deadline = Instant::now() + BOUND;
                    while !context.cancelled() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err("stopped".to_string())
                }
            }
        });
        if outcome == JobOutcome::Cancelled {
            cancel_wait.recv_timeout(BOUND).expect("the job is running");
            assert!(state.jobs.cancel(id), "a running job accepts a cancel");
        }
        let end = Board::Finished(id.raw(), label.to_string(), outcome, detail.to_string());
        let ours = |board: &Board| match board {
            Board::Progress(job, ..) | Board::Finished(job, ..) => *job == id.raw(),
        };
        let mut seen = Vec::new();
        loop {
            let next = board
                .recv_timeout(BOUND)
                .expect("the job ends within the bound");
            let ended = next == end;
            if ours(&next) {
                seen.push(next);
            }
            if ended {
                break;
            }
        }
        assert!(
            seen.contains(&Board::Progress(
                id.raw(),
                Some(0.5),
                "half way".to_string()
            )),
            "{label}: the progress report never reached the bus: {seen:?}"
        );
        assert!(bus.flush(BOUND), "delivery must finish within the bound");
        let late: Vec<Board> = board.try_iter().filter(|note| ours(note)).collect();
        assert!(
            late.is_empty(),
            "{label}: posted after it finished: {late:?}"
        );
    }
    drop((progress, finished));
}

/// How a scripted render ends.
#[derive(Debug, Clone, Copy)]
enum Ending {
    Writes,
    Fails,
    IsCancelled,
    IsCancelledAfterReturning,
    Panics,
}

fn render_job(
    state: &mut AppState,
    ending: Ending,
    kind: RenderKind,
    path: &Path,
) -> (JobId, RenderResult) {
    let bus = Arc::clone(state.ctx.event_bus());
    let renders = Recorder::new(&bus, |e: &RenderFinished| {
        (e.kind, e.path.clone(), e.result.clone())
    });
    let (running, running_wait) = crossbeam_channel::bounded::<()>(1);
    let (release, release_wait) = crossbeam_channel::bounded::<()>(1);
    let id = state.spawn_render(
        "rendering",
        kind,
        path.to_path_buf(),
        move |context| match ending {
            Ending::Writes => Ok(JobCompletion::new("written")),
            Ending::Fails => Err("No space left on device".to_string()),
            Ending::IsCancelled => {
                running.send(()).expect("announce the wait");
                let deadline = Instant::now() + BOUND;
                while !context.cancelled() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err("cancelled".to_string())
            }
            Ending::IsCancelledAfterReturning => {
                running.send(()).expect("announce the wait");
                release_wait.recv_timeout(BOUND).expect("released");
                Ok(JobCompletion::new("written"))
            }
            Ending::Panics => panic!("codec exploded"),
        },
    );
    if matches!(
        ending,
        Ending::IsCancelled | Ending::IsCancelledAfterReturning
    ) {
        running_wait
            .recv_timeout(BOUND)
            .expect("the render is running");
        assert!(state.jobs.cancel(id), "a running render accepts a cancel");
        let _ = release.send(());
    }
    let (posted_kind, posted_path, result) = renders.next();
    assert_eq!(posted_kind, kind);
    assert_eq!(posted_path, path);
    (id, result)
}

/// The registry's terminal snapshot for `id`, waited for within the bound.
fn settled(state: &AppState, id: JobId) -> (JobState, String) {
    let deadline = Instant::now() + BOUND;
    loop {
        let snapshot = state.jobs.get(id).expect("the job is registered");
        if snapshot.state.is_terminal() {
            return (snapshot.state, snapshot.detail);
        }
        assert!(Instant::now() < deadline, "the render never ended");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_render_posts_the_result_the_registry_records() {
    let (mut state, clip) = with_clip();
    let cases = [
        (Ending::Writes, RenderResult::Written, JobState::Done),
        (
            Ending::Fails,
            RenderResult::Failed("No space left on device".to_string()),
            JobState::Failed,
        ),
        (
            Ending::IsCancelled,
            RenderResult::Cancelled,
            JobState::Cancelled,
        ),
        (
            Ending::IsCancelledAfterReturning,
            RenderResult::Cancelled,
            JobState::Cancelled,
        ),
        (
            Ending::Panics,
            RenderResult::Failed("panicked: codec exploded".to_string()),
            JobState::Failed,
        ),
    ];
    for (index, (ending, expected, registry)) in cases.into_iter().enumerate() {
        let kind = if index % 2 == 0 {
            RenderKind::Clip(clip)
        } else {
            RenderKind::ImageSequence
        };
        let path = PathBuf::from(format!("render-{index}"));
        let (id, posted) = render_job(&mut state, ending, kind, &path);
        assert_eq!(posted, expected, "{ending:?}");
        let (state_of, detail) = settled(&state, id);
        assert_eq!(state_of, registry, "{ending:?}: the registry disagrees");
        if let RenderResult::Failed(reason) = &posted {
            assert_eq!(&detail, reason, "{ending:?}: the reasons differ");
        }
    }
}

/// Every context menu target and the target a plugin reads for it.
fn every_target() -> Vec<(ContextTarget, MenuTarget)> {
    vec![
        (
            ContextTarget::Clip { clip: 4, track: 2 },
            MenuTarget::Clip {
                clip: ClipId::from_raw(4),
                track: TrackId::from_raw(2),
            },
        ),
        (
            ContextTarget::Track { track: 3 },
            MenuTarget::Track {
                track: TrackId::from_raw(3),
            },
        ),
        (
            ContextTarget::Transition { clip: 5 },
            MenuTarget::Transition {
                clip: ClipId::from_raw(5),
            },
        ),
        (ContextTarget::Preview, MenuTarget::Viewer),
        (
            ContextTarget::PoolItem { media: 7 },
            MenuTarget::Media {
                media: MediaId::from_raw(7),
            },
        ),
        (
            ContextTarget::Recent {
                path: PathBuf::from("cut.aria"),
            },
            MenuTarget::Recent {
                path: PathBuf::from("cut.aria"),
            },
        ),
        (
            ContextTarget::Effect { clip: 4, index: 1 },
            MenuTarget::Effect {
                clip: ClipId::from_raw(4),
                index: 1,
            },
        ),
        (
            ContextTarget::ClipBadge { clip: 4, track: 2 },
            MenuTarget::ClipBadge {
                clip: ClipId::from_raw(4),
                track: TrackId::from_raw(2),
            },
        ),
    ]
}

fn row_labels(state: &AppState) -> Vec<String> {
    state
        .context_plugin_rows
        .iter()
        .map(|row| row.label().to_owned())
        .collect()
}

#[test]
fn a_menu_collects_plugin_rows_in_mount_order_for_its_target() {
    let (mut state, _) = with_clip();
    let bus = Arc::clone(state.ctx.event_bus());

    state.open_context(ContextTarget::Preview, 10.0, 10.0);
    assert!(row_labels(&state).is_empty(), "no handler adds no rows");

    type Seen = Arc<Mutex<Vec<(MenuTarget, Arc<Project>)>>>;
    let seen: Seen = Arc::default();
    let first = {
        let seen = Arc::clone(&seen);
        bus.waterfall::<ContextMenuRequested, _>(move |request| {
            seen.lock()
                .expect("seen")
                .push((request.target.clone(), Arc::clone(&request.project)));
            request.add("First", || {});
        })
    };
    let second = bus.waterfall::<ContextMenuRequested, _>(|request| {
        request.add("Second", || {});
    });

    for (target, expected) in every_target() {
        state.open_context(target.clone(), 10.0, 10.0);
        assert_eq!(row_labels(&state), ["First", "Second"], "{target:?}");
        let (read, project) = seen.lock().expect("seen").pop().expect("the handler ran");
        assert_eq!(read, expected);
        assert!(
            Arc::ptr_eq(&project, &state.host.snapshot()),
            "{target:?}: the handler read a project other than the session's"
        );
    }

    state.close_context();
    assert_eq!(
        row_labels(&state),
        ["First", "Second"],
        "a closing menu keeps its rows for the fade out"
    );
    drop((first, second));
    state.open_context(ContextTarget::Preview, 10.0, 10.0);
    assert!(
        row_labels(&state).is_empty(),
        "dropped handlers still added rows"
    );
}

#[test]
fn a_panicking_menu_handler_keeps_the_rows_added_before_it() {
    let (mut state, _) = with_clip();
    let bus = Arc::clone(state.ctx.event_bus());
    let _kept = bus.waterfall::<ContextMenuRequested, _>(|request| {
        request.add("Kept", || {});
    });
    let _panics = bus.waterfall::<ContextMenuRequested, _>(|_| panic!("menu handler"));
    let _lost = bus.waterfall::<ContextMenuRequested, _>(|request| {
        request.add("Lost", || {});
    });
    state.open_context(ContextTarget::Preview, 10.0, 10.0);
    assert_eq!(row_labels(&state), ["Kept"]);
    assert_eq!(state.context_menu, Some(ContextTarget::Preview));
}

#[test]
fn a_clicked_plugin_row_runs_off_the_window_thread() {
    let (mut state, _) = with_clip();
    let (sent, ran) = crossbeam_channel::bounded(1);
    state.run_plugin_row(Arc::new(move || {
        let _ = sent.send(std::thread::current().id());
    }));
    let thread = ran
        .recv_timeout(BOUND)
        .expect("the row ran within the bound");
    assert_ne!(thread, std::thread::current().id());
    assert_eq!(state.last_error, None);
}
