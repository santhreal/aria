//! Contracts for the sequential render queue.
//!
//! The defect class closed here is "exports block the editor, run unbounded in
//! parallel when multiple are requested, race on encoders, and cannot be queued
//! or cancelled individually".
//!
//! What is asserted:
//!
//! - Ordered sequential execution: jobs run strictly one at a time in enqueue order.
//! - Per-entry state transitions: Queued -> Running -> Done / Failed / Cancelled.
//! - Queued cancellation: cancelling a queued entry removes it immediately without
//!   affecting the running job.
//! - Running cancellation: cancelling the running entry stops it, marks it Cancelled,
//!   and advances to start the next queued entry.
//! - Honest failure handling: a failing export records its error message and does
//!   not stall subsequent jobs in the queue.
//! - Bounded termination: the queue reaches idle within a measured deadline.
//! - Scripted doubles: tests run without requiring a physical GPU encoder.
//! - Notifications: each entry that starts posts `ExportStarted`, then
//!   `ExportFinished` with how it ended, before the next entry starts; an entry
//!   cancelled while queued posts neither.
//! - Unmounted plugin encoder refusal: a queued export targeting a plugin encoder
//!   that unmounts before running fails naming the format id and never falls back
//!   to the platform encoder.
//!
//! What this does not catch: actual frame rendering or codec muxing on the GPU,
//! which is covered by `aria-render`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aria_jobs::{JobCompletion, JobManager};
use aria_models::{Project, Rational};
use aria_plugin::{EventBus, ExportFinished, ExportStarted, Registration, RenderResult};
use aria_render::ExportSettings;

use super::export_queue::{ExportEntryState, ExportQueue, ExportRunner};
use super::jobs::file_label;
use super::premiere_tests::with_clip;

const BOUND: Duration = Duration::from_secs(10);

/// A notification the queue posted, by file name.
#[derive(Debug, Clone, PartialEq)]
enum Posted {
    Started(String),
    Finished(String, RenderResult),
}

fn started(name: &str) -> Posted {
    Posted::Started(name.to_string())
}

fn finished(name: &str, result: RenderResult) -> Posted {
    Posted::Finished(name.to_string(), result)
}

/// A queue over `runner`, and what it posts.
struct Watched {
    queue: ExportQueue,
    bus: Arc<EventBus>,
    posted: crossbeam_channel::Receiver<Posted>,
    _registrations: [Registration; 2],
}

fn watch(manager: JobManager, runner: ExportRunner) -> Watched {
    let bus = Arc::new(EventBus::new());
    let (sent, posted) = crossbeam_channel::unbounded();
    let on_start = {
        let sent = sent.clone();
        bus.subscribe::<ExportStarted, _>(move |e| {
            let _ = sent.send(Posted::Started(file_label(&e.path)));
        })
    };
    let on_finish = bus.subscribe::<ExportFinished, _>(move |e| {
        let _ = sent.send(Posted::Finished(file_label(&e.path), e.result.clone()));
    });
    Watched {
        queue: ExportQueue::new(manager, runner, Arc::clone(&bus)),
        bus,
        posted,
        _registrations: [on_start, on_finish],
    }
}

impl Watched {
    /// The next `count` notifications, each arriving within the bound.
    fn next(&self, count: usize) -> Vec<Posted> {
        (0..count)
            .map(|_| {
                self.posted
                    .recv_timeout(BOUND)
                    .expect("a notification within the bound")
            })
            .collect()
    }

    /// Whether nothing else was posted.
    fn quiet(&self) -> bool {
        assert!(
            self.bus.flush(BOUND),
            "delivery must finish within the bound"
        );
        self.posted.try_recv().is_err()
    }
}

/// A project and the settings its own export path derives.
///
/// Neither type has a `Default`, and deliberately: an export is described
/// relative to a real sequence (its rate, its canvas), so a queue test that
/// invented settings would be queueing a job the shipped dialog cannot produce.
fn queued_project() -> (Arc<Project>, ExportSettings) {
    let project = Project::new("queue", Rational::FPS_24);
    let settings = ExportSettings::for_project(&project);
    (Arc::new(project), settings)
}

#[test]
fn two_enqueued_jobs_run_in_order_and_both_report_done() {
    let manager = JobManager::new();
    let order = Arc::new(Mutex::new(Vec::new()));
    let order_clone = Arc::clone(&order);

    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let name = file_label(dest);
        {
            let mut log = order_clone.lock().expect("lock");
            log.push(format!("start:{name}"));
        }
        for frame in 1..=10 {
            if context.cancelled() {
                return Err("cancelled".to_string());
            }
            context.set_progress(frame as f32 / 10.0, format!("frame {frame}/10"));
            std::thread::sleep(Duration::from_millis(2));
        }
        {
            let mut log = order_clone.lock().expect("lock");
            log.push(format!("end:{name}"));
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("10 frames")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let watched = watch(manager, runner);
    let queue = &watched.queue;
    let (project, settings) = queued_project();

    let id1 = queue.enqueue(PathBuf::from("first.mp4"), settings, Arc::clone(&project));
    let id2 = queue.enqueue(PathBuf::from("second.mp4"), settings, Arc::clone(&project));

    let settled = queue.settle_all(Duration::from_secs(10));
    assert!(settled, "queue did not settle in time");

    let entries = queue.snapshot();
    let e1 = entries.iter().find(|e| e.id == id1).expect("entry 1");
    let e2 = entries.iter().find(|e| e.id == id2).expect("entry 2");

    assert_eq!(e1.state, ExportEntryState::Done);
    assert_eq!(e2.state, ExportEntryState::Done);
    assert_eq!(e1.progress, Some(1.0));
    assert_eq!(e2.progress, Some(1.0));
    assert!(e1.terminal);
    assert!(e2.terminal);

    let log = order.lock().expect("lock").to_vec();
    assert_eq!(
        log,
        vec![
            "start:first.mp4",
            "end:first.mp4",
            "start:second.mp4",
            "end:second.mp4"
        ]
    );
    assert_eq!(
        watched.next(4),
        [
            started("first.mp4"),
            finished("first.mp4", RenderResult::Written),
            started("second.mp4"),
            finished("second.mp4", RenderResult::Written),
        ]
    );
}

#[test]
fn cancelling_a_queued_entry_leaves_the_running_one_alone() {
    let manager = JobManager::new();
    let running = Arc::new(AtomicBool::new(false));
    let running_clone = Arc::clone(&running);
    let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);

    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        running_clone.store(true, Ordering::SeqCst);
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
        if context.cancelled() {
            return Err("cancelled".to_string());
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let watched = watch(manager, runner);
    let queue = &watched.queue;
    let (project, settings) = queued_project();

    let id1 = queue.enqueue(PathBuf::from("running.mp4"), settings, Arc::clone(&project));
    let id2 = queue.enqueue(PathBuf::from("queued.mp4"), settings, Arc::clone(&project));

    // Wait until job 1 is running
    let deadline = Instant::now() + Duration::from_secs(5);
    while !running.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "job 1 never started");
        std::thread::sleep(Duration::from_millis(5));
    }

    // Cancel queued entry id2 - should remove it from the queue
    assert!(queue.cancel(id2), "queued job was not cancelled");

    // Release job 1 to complete normally
    let _ = release_tx.send(());
    assert!(queue.settle_all(Duration::from_secs(10)));

    let entries = queue.snapshot();
    let e1 = entries.iter().find(|e| e.id == id1).expect("entry 1");
    assert_eq!(e1.state, ExportEntryState::Done);
    // id2 was removed from the queue
    assert!(
        entries.iter().all(|e| e.id != id2),
        "cancelled queued entry was not removed from queue"
    );
    assert_eq!(
        watched.next(2),
        [
            started("running.mp4"),
            finished("running.mp4", RenderResult::Written),
        ]
    );
    assert!(
        watched.quiet(),
        "an entry cancelled while queued posts nothing"
    );
}

#[test]
fn cancelling_the_running_entry_marks_it_cancelled_and_starts_the_next() {
    let manager = JobManager::new();
    let (started_tx, started_rx) = crossbeam_channel::bounded::<String>(2);

    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let name = file_label(dest);
        let _ = started_tx.send(name.to_string());
        if name == "first.mp4" {
            while !context.cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err("cancelled by operator".to_string());
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let watched = watch(manager, runner);
    let queue = &watched.queue;
    let (project, settings) = queued_project();

    let id1 = queue.enqueue(PathBuf::from("first.mp4"), settings, Arc::clone(&project));
    let id2 = queue.enqueue(PathBuf::from("second.mp4"), settings, Arc::clone(&project));

    let first_started = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("job 1 start");
    assert_eq!(first_started, "first.mp4");

    // Cancel running job 1
    assert!(queue.cancel(id1), "running job cancel was refused");

    // Job 2 should start automatically after job 1 cancels
    let second_started = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("job 2 start");
    assert_eq!(second_started, "second.mp4");

    assert!(queue.settle_all(Duration::from_secs(10)));

    let entries = queue.snapshot();
    let e1 = entries.iter().find(|e| e.id == id1).expect("entry 1");
    let e2 = entries.iter().find(|e| e.id == id2).expect("entry 2");

    assert_eq!(e1.state, ExportEntryState::Cancelled);
    assert_eq!(e2.state, ExportEntryState::Done);
    assert_eq!(
        watched.next(4),
        [
            started("first.mp4"),
            finished("first.mp4", RenderResult::Cancelled),
            started("second.mp4"),
            finished("second.mp4", RenderResult::Written),
        ]
    );
}

#[test]
fn a_failed_export_reports_its_message_and_does_not_stall_the_queue() {
    let manager = JobManager::new();

    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let name = file_label(dest);
        if name == "failing.mp4" {
            return Err("No space left on device".to_string());
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let watched = watch(manager, runner);
    let queue = &watched.queue;
    let (project, settings) = queued_project();

    let id1 = queue.enqueue(PathBuf::from("failing.mp4"), settings, Arc::clone(&project));
    let id2 = queue.enqueue(
        PathBuf::from("succeeding.mp4"),
        settings,
        Arc::clone(&project),
    );

    assert!(queue.settle_all(Duration::from_secs(10)));

    let entries = queue.snapshot();
    let e1 = entries.iter().find(|e| e.id == id1).expect("entry 1");
    let e2 = entries.iter().find(|e| e.id == id2).expect("entry 2");

    assert_eq!(e1.state, ExportEntryState::Failed);
    assert_eq!(e1.error.as_deref(), Some("No space left on device"));
    assert_eq!(e2.state, ExportEntryState::Done);
    assert_eq!(e2.error, None);
    assert_eq!(
        watched.next(4),
        [
            started("failing.mp4"),
            finished(
                "failing.mp4",
                RenderResult::Failed("No space left on device".to_string())
            ),
            started("succeeding.mp4"),
            finished("succeeding.mp4", RenderResult::Written),
        ]
    );
}

#[test]
fn the_queue_reports_bounded_termination() {
    let manager = JobManager::new();
    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let queue = ExportQueue::new(manager, runner, Arc::new(EventBus::new()));
    let (project, settings) = queued_project();

    for i in 0..5 {
        queue.enqueue(
            PathBuf::from(format!("batch_{i}.mp4")),
            settings,
            Arc::clone(&project),
        );
    }

    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    let settled = queue.settle_all(timeout);
    assert!(settled, "queue did not terminate within bound");
    assert!(start.elapsed() < timeout, "queue exceeded timeout bound");
    assert!(queue.is_idle());

    let entries = queue.snapshot();
    assert_eq!(entries.len(), 5);
    for entry in &entries {
        assert!(entry.terminal);
        assert_eq!(entry.state, ExportEntryState::Done);
    }
}

#[test]
fn a_panicking_runner_fails_honestly_and_advances_queue() {
    let manager = JobManager::new();

    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let name = file_label(dest);
        if name == "panicking.mp4" {
            panic!("unexpected encoder crash");
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });

    let watched = watch(manager, runner);
    let queue = &watched.queue;
    let (project, settings) = queued_project();

    let id1 = queue.enqueue(
        PathBuf::from("panicking.mp4"),
        settings,
        Arc::clone(&project),
    );
    let id2 = queue.enqueue(
        PathBuf::from("recovered.mp4"),
        settings,
        Arc::clone(&project),
    );

    assert!(queue.settle_all(Duration::from_secs(10)));

    let entries = queue.snapshot();
    let e1 = entries.iter().find(|e| e.id == id1).expect("entry 1");
    let e2 = entries.iter().find(|e| e.id == id2).expect("entry 2");

    assert_eq!(e1.state, ExportEntryState::Failed);
    assert_eq!(e2.state, ExportEntryState::Done);
    assert_eq!(
        watched.next(4),
        [
            started("panicking.mp4"),
            finished(
                "panicking.mp4",
                RenderResult::Failed("export runner panicked or exited early".to_string())
            ),
            started("recovered.mp4"),
            finished("recovered.mp4", RenderResult::Written),
        ]
    );
}

#[test]
fn app_state_reaches_export_queue_and_cancels_entries() {
    let (mut state, _clip) = with_clip();
    // The first entry holds until the second is cancelled, so the second is
    // still queued when the cancel reaches it.
    let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);
    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let _ = release_rx.recv_timeout(Duration::from_secs(10));
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });
    state.export_queue.set_runner(runner);

    let dir = crate::scratch_pool::scratch("export_queue_reach");
    let out1 = dir.join("first.mp4");
    let out2 = dir.join("second.mp4");

    let id1 = state.export_job(out1).expect("enqueue 1");
    let id2 = state.export_job(out2).expect("enqueue 2");

    assert!(state.status_message.is_some());
    // Enqueueing projects the board, so the rows exist without a wake.
    let rows = state.job_view.clone();
    assert!(rows.iter().any(|r| r.id == id1 || r.id == id2));

    // Cancel queued entry 2
    state.cancel_job(id2);
    release_tx
        .send(())
        .expect("the first entry waits for its release");
    assert!(state.export_queue.settle_all(Duration::from_secs(10)));

    let snapshot = state.export_queue.snapshot();
    assert!(
        snapshot
            .iter()
            .any(|e| e.id == id1 && e.state == ExportEntryState::Done)
    );
    assert!(snapshot.iter().all(|e| e.id != id2));
}

#[test]
fn a_queued_plugin_encoder_that_unmounts_before_running_fails_naming_the_format_without_fallback() {
    use aria_core::{EncodeError, EncoderConfig, VideoEncoder, VideoFrame};
    use aria_plugin::{Encoder, EncoderSpec};
    use std::path::Path;

    struct DummyEncoder;
    impl VideoEncoder for DummyEncoder {
        fn push(&mut self, _frame: &VideoFrame) -> Result<(), EncodeError> {
            Ok(())
        }
        fn finish(&mut self) -> Result<(), EncodeError> {
            Ok(())
        }
        fn choice(&self) -> aria_core::EncoderChoice {
            aria_core::EncoderChoice::only("dummy", None)
        }
    }
    impl Encoder for DummyEncoder {
        fn open(
            &self,
            _path: &Path,
            _config: &EncoderConfig,
        ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
            Ok(Box::new(DummyEncoder))
        }
    }

    let (mut state, _clip) = with_clip();
    let reg = state
        .ctx
        .register_encoder(
            EncoderSpec {
                id: "prores.422".into(),
                name: "Apple ProRes 422".into(),
                extension: "mov".into(),
                audio: true,
            },
            DummyEncoder,
        )
        .expect("register prores.422");

    let (started_tx, started_rx) = crossbeam_channel::bounded::<()>(1);
    let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(1);
    let runner_called_for_second = Arc::new(AtomicBool::new(false));
    let runner_called_clone = Arc::clone(&runner_called_for_second);
    let runner: ExportRunner = Arc::new(move |context, _project, dest, _settings| {
        let name = file_label(dest);
        if name == "first.mp4" {
            let _ = started_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        } else {
            runner_called_clone.store(true, Ordering::SeqCst);
        }
        let _ = context.try_begin_commit();
        JobCompletion::new("done")
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    });
    state.export_queue.set_runner(runner);

    let dir = crate::scratch_pool::scratch("export_queue_plugin_unmount");
    let id1 = state
        .export_job(dir.join("first.mp4"))
        .expect("enqueue first");
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first job started");

    // Enqueue second job using the plugin encoder while it is still mounted.
    state.select_export_plugin_encoder(Some("prores.422".into()));
    let id2 = state
        .export_job(dir.join("second.mov"))
        .expect("enqueue second with prores.422");

    // Unmount the plugin encoder while job 2 is still Queued.
    drop(reg);

    // Let job 1 finish so job 2 is pumped.
    let _ = release_tx.send(());
    assert!(state.export_queue.settle_all(Duration::from_secs(10)));

    let snapshot = state.export_queue.snapshot();
    let e1 = snapshot.iter().find(|e| e.id == id1).expect("entry 1");
    let e2 = snapshot.iter().find(|e| e.id == id2).expect("entry 2");
    assert_eq!(e1.state, ExportEntryState::Done);
    assert_eq!(e2.state, ExportEntryState::Failed);
    let err = e2.error.as_deref().expect("error recorded");
    assert!(
        err.contains("prores.422"),
        "failed export must name the unmounted format id: {err}"
    );
    assert!(
        !runner_called_for_second.load(Ordering::SeqCst),
        "unmounted plugin encoder must fail without falling back to the default runner"
    );
}
