//! Sequential render queue for background exports.
//!
//! Exports are heavyweight render passes that stream frames to disk. Running
//! several concurrently would saturate disk I/O, race on hardware encoders, and
//! exhaust video memory. [`ExportQueue`] ensures that enqueued export passes run
//! strictly one at a time under [`JobManager`], while each entry remains
//! individually inspectable, reportable, and cancellable.
//!
//! Lifecycle rules:
//!
//! - **Ordered execution.** Jobs execute strictly in the order they were enqueued.
//! - **One at a time.** Exactly one export pass runs at any given moment; subsequent
//!   passes wait in `Queued` state.
//! - **Cancellation.** Cancelling a queued entry immediately removes it from the
//!   queue. Cancelling the running entry cooperative-stops it, marks it `Cancelled`,
//!   and advances the queue to start the next queued entry.
//! - **Honest failure reporting.** A failed export records its error message and
//!   marks itself `Failed` without stalling the queue: subsequent entries proceed.
//! - **Panic containment.** A panicked export runner is caught safely and marked
//!   `Failed`, unblocking subsequent queue entries.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aria_core::count::counted;
use aria_jobs::{JobCompletion, JobId, JobManager};
use aria_models::Project;
use aria_plugin::{EventBus, ExportFinished, ExportStarted, PluginContext, RenderResult};
use aria_render::ExportSettings;

use super::jobs::{CommitGuard, file_label};

/// State of an entry inside the render queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportEntryState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl ExportEntryState {
    /// True when the entry has finished running (Done, Failed, or Cancelled).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// Pluggable runner closure type for export passes.
pub type ExportRunner = Arc<
    dyn Fn(&aria_jobs::JobContext, &Project, &Path, ExportSettings) -> Result<JobCompletion, String>
        + Send
        + Sync
        + 'static,
>;

/// Default production runner that renders using GPU or headless renderer.
pub fn default_export_runner(gpu: Option<aria_compositor::GpuContext>) -> ExportRunner {
    Arc::new(move |context, project, dest, settings| {
        let mut renderer =
            super::render_device::renderer_on(gpu.as_ref().map(|g| g.share()), "export")?;
        let mut guard = CommitGuard::new(context, "the exported file");
        let mut observer = |done: u64, total: u64| {
            let fraction = if total == 0 {
                0.0
            } else {
                done as f32 / total as f32
            };
            context.set_progress(fraction, format!("frame {done}/{total}"));
            if done == total {
                return guard.begin().is_ok();
            }
            !context.cancelled()
        };
        let frames = aria_render::export_to_file_observed(
            &mut renderer,
            project,
            dest,
            settings,
            &mut observer,
        )
        .map_err(|err| err.to_string())?;
        debug_assert!(
            guard.committed(),
            "the export published without the boundary"
        );
        JobCompletion::new(counted(frames, "frame", "frames").to_string())
            .with_output("path", dest.display().to_string())
            .map_err(String::from)
    })
}

fn run_plugin_export(
    context: &aria_jobs::JobContext,
    project: &Project,
    dest: &Path,
    settings: ExportSettings,
    gpu: Option<&aria_compositor::GpuContext>,
    plugins: &PluginContext,
    encoder_id: &str,
) -> Result<JobCompletion, String> {
    if plugins.encoder(encoder_id).is_none() {
        return Err(format!(
            "no encoder is registered as {encoder_id}; enable or reinstall the plugin that provides it"
        ));
    }
    let mut renderer = super::render_device::renderer_on(gpu.map(|g| g.share()), "export")?;
    let mut guard = CommitGuard::new(context, "the exported file");
    let mut observer = |done: u64, total: u64| {
        let fraction = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };
        context.set_progress(fraction, format!("frame {done}/{total}"));
        if done == total {
            return guard.begin().is_ok();
        }
        !context.cancelled()
    };
    let mut opener = |stage_path: &Path, config: aria_core::EncoderConfig| {
        plugins.open_encoder(encoder_id, stage_path, &config)
    };
    let frames = aria_render::export_to_file_with_encoder_observed(
        &mut renderer,
        project,
        dest,
        settings,
        &mut opener,
        &mut observer,
    )
    .map_err(|err| err.to_string())?
    .frames;
    debug_assert!(
        guard.committed(),
        "the export published without the boundary"
    );
    JobCompletion::new(counted(frames, "frame", "frames").to_string())
        .with_output("path", dest.display().to_string())
        .map_err(String::from)
}

/// One entry in the export queue.
#[derive(Debug, Clone)]
pub struct ExportQueueEntry {
    pub id: u64,
    pub label: String,
    pub dest: PathBuf,
    pub settings: ExportSettings,
    pub project: Arc<Project>,
    pub plugin_encoder: Option<String>,
    pub state: ExportEntryState,
    pub progress: Option<f32>,
    pub error: Option<String>,
    pub job_id: Option<JobId>,
    pub cancel_requested: bool,
    pub created: Instant,
    pub finished: Option<Instant>,
}

/// Point-in-time snapshot of one export queue entry for UI inspection.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportQueueSnapshot {
    pub id: u64,
    pub label: String,
    pub dest: PathBuf,
    pub state: ExportEntryState,
    pub status: &'static str,
    pub progress: Option<f32>,
    pub error: Option<String>,
    pub cancellable: bool,
    pub terminal: bool,
    pub elapsed: f32,
}

struct QueueInner {
    next_id: u64,
    entries: Vec<ExportQueueEntry>,
    running_id: Option<u64>,
}

struct SharedQueue {
    job_manager: JobManager,
    runner: Mutex<ExportRunner>,
    inner: Mutex<QueueInner>,
    plugins: Option<PluginContext>,
    gpu: Option<aria_compositor::GpuContext>,
    /// Where `ExportStarted` and `ExportFinished` are posted.
    events: Arc<EventBus>,
    /// Held across a state transition and its notification, so an entry's
    /// `ExportFinished` is posted before the next entry's `ExportStarted`.
    /// Taken before `inner`.
    announcing: Mutex<()>,
}

impl SharedQueue {
    // Takes the `Arc` because finishing one entry starts the next, and the
    // pump hands a clone of the queue to the job it spawns.
    fn complete_entry(
        self: &Arc<Self>,
        entry_id: u64,
        state: ExportEntryState,
        error: Option<String>,
    ) {
        let announcing = self.announcing.lock().expect("announce lock");
        let finished = {
            let mut inner = self.inner.lock().expect("queue lock");
            let finished = inner
                .entries
                .iter_mut()
                .find(|e| e.id == entry_id)
                .map(|entry| {
                    let result = match state {
                        ExportEntryState::Done => RenderResult::Written,
                        ExportEntryState::Cancelled => RenderResult::Cancelled,
                        ExportEntryState::Failed
                        | ExportEntryState::Queued
                        | ExportEntryState::Running => {
                            RenderResult::Failed(error.clone().unwrap_or_default())
                        }
                    };
                    entry.state = state;
                    entry.error = error;
                    entry.finished = Some(Instant::now());
                    if state == ExportEntryState::Done {
                        entry.progress = Some(1.0);
                    }
                    ExportFinished {
                        path: entry.dest.clone(),
                        result,
                    }
                });
            if inner.running_id == Some(entry_id) {
                inner.running_id = None;
            }
            finished
        };
        if let Some(finished) = finished {
            self.events.post(finished);
        }
        drop(announcing);
        self.pump_internal();
    }

    fn pump_internal(self: &Arc<Self>) {
        let announcing = self.announcing.lock().expect("announce lock");
        let to_spawn = {
            let mut inner = self.inner.lock().expect("queue lock");
            if let Some(running_id) = inner.running_id {
                let still_running = inner
                    .entries
                    .iter()
                    .any(|e| e.id == running_id && !e.state.is_terminal());
                if still_running {
                    return;
                }
                inner.running_id = None;
            }

            let Some(entry) = inner
                .entries
                .iter_mut()
                .find(|e| e.state == ExportEntryState::Queued)
            else {
                return;
            };

            entry.state = ExportEntryState::Running;
            let entry_id = entry.id;
            let project = Arc::clone(&entry.project);
            let dest = entry.dest.to_path_buf();
            let settings = entry.settings;
            let plugin_encoder = entry.plugin_encoder.clone();
            let label = entry.label.to_string();
            let cancel_early = entry.cancel_requested;
            // After the entry's own fields: `entry` borrows `inner`, so the
            // running marker is written once that borrow is done with.
            inner.running_id = Some(entry_id);

            (
                entry_id,
                label,
                dest,
                settings,
                project,
                plugin_encoder,
                cancel_early,
            )
        };

        let (entry_id, label, dest, settings, project, plugin_encoder, cancel_early) = to_spawn;
        self.events.post(ExportStarted { path: dest.clone() });
        drop(announcing);
        let runner = {
            let r = self.runner.lock().expect("runner lock");
            Arc::clone(&*r)
        };
        let plugins = self.plugins.clone();
        let gpu = self.gpu.as_ref().map(|g| g.share());

        let shared_for_job = Arc::clone(self);
        let job_id = self
            .job_manager
            .spawn_with_completion(label, move |context| {
                if cancel_early {
                    let _ = context.cancelled();
                }
                let mut guard = JobCompletionGuard {
                    shared: shared_for_job,
                    entry_id,
                    context,
                    completed: false,
                };
                let result = match plugin_encoder.as_deref() {
                    Some(encoder_id) => match plugins.as_ref() {
                        Some(ctx) => run_plugin_export(
                            context,
                            &project,
                            &dest,
                            settings,
                            gpu.as_ref(),
                            ctx,
                            encoder_id,
                        ),
                        None => Err(format!(
                            "no encoder is registered as {encoder_id}; enable or reinstall the plugin that provides it"
                        )),
                    },
                    None => runner(context, &project, &dest, settings),
                };
                guard.finish(&result);
                result
            });

        let mut inner = self.inner.lock().expect("queue lock");
        if let Some(entry) = inner.entries.iter_mut().find(|e| e.id == entry_id) {
            entry.job_id = Some(job_id);
            if entry.cancel_requested {
                self.job_manager.cancel(job_id);
            }
        }
    }
}

struct JobCompletionGuard<'a> {
    shared: Arc<SharedQueue>,
    entry_id: u64,
    context: &'a aria_jobs::JobContext,
    completed: bool,
}

impl<'a> JobCompletionGuard<'a> {
    fn finish(&mut self, result: &Result<JobCompletion, String>) {
        if self.completed {
            return;
        }
        self.completed = true;
        let cancelled = self.context.cancelled();
        let (state, error) = match result {
            _ if cancelled => (ExportEntryState::Cancelled, None),
            Ok(_) => (ExportEntryState::Done, None),
            Err(err) => (ExportEntryState::Failed, Some(err.to_string())),
        };
        self.shared.complete_entry(self.entry_id, state, error);
    }
}

impl<'a> Drop for JobCompletionGuard<'a> {
    fn drop(&mut self) {
        if !self.completed {
            self.completed = true;
            let cancelled = self.context.cancelled();
            let state = if cancelled {
                ExportEntryState::Cancelled
            } else {
                ExportEntryState::Failed
            };
            let error = Some("export runner panicked or exited early".to_string());
            self.shared.complete_entry(self.entry_id, state, error);
        }
    }
}

/// Sequential export queue.
pub struct ExportQueue {
    shared: Arc<SharedQueue>,
}

impl ExportQueue {
    /// Create a new queue backed by `job_manager` and `runner`, posting
    /// `ExportStarted` and `ExportFinished` on `events`.
    pub fn new(job_manager: JobManager, runner: ExportRunner, events: Arc<EventBus>) -> Self {
        Self {
            shared: Arc::new(SharedQueue {
                job_manager,
                runner: Mutex::new(runner),
                plugins: None,
                gpu: None,
                inner: Mutex::new(QueueInner {
                    next_id: 1,
                    entries: Vec::new(),
                    running_id: None,
                }),
                events,
                announcing: Mutex::new(()),
            }),
        }
    }

    /// Create a new queue using the default production GPU renderer.
    pub fn new_with_device(
        job_manager: JobManager,
        gpu: Option<aria_compositor::GpuContext>,
        events: Arc<EventBus>,
    ) -> Self {
        Self::new_with_device_and_plugins(job_manager, gpu, None, events)
    }

    /// Create a new queue using the default production GPU renderer and a
    /// [`PluginContext`] for resolving plugin-contributed encoders.
    pub fn new_with_device_and_plugins(
        job_manager: JobManager,
        gpu: Option<aria_compositor::GpuContext>,
        plugins: Option<PluginContext>,
        events: Arc<EventBus>,
    ) -> Self {
        let runner = default_export_runner(gpu.as_ref().map(|g| g.share()));
        Self {
            shared: Arc::new(SharedQueue {
                job_manager,
                runner: Mutex::new(runner),
                plugins,
                gpu,
                inner: Mutex::new(QueueInner {
                    next_id: 1,
                    entries: Vec::new(),
                    running_id: None,
                }),
                events,
                announcing: Mutex::new(()),
            }),
        }
    }

    /// Replace the export runner (useful for tests and scripted doubles).
    pub fn set_runner(&self, runner: ExportRunner) {
        let mut r = self.shared.runner.lock().expect("runner lock");
        *r = runner;
    }

    /// Enqueue an export pass using the platform encoder. Returns the unique entry ID.
    pub fn enqueue(&self, dest: PathBuf, settings: ExportSettings, project: Arc<Project>) -> u64 {
        self.enqueue_with_encoder(dest, settings, project, None)
    }

    /// Enqueue an export pass with an optional plugin encoder id. Returns the unique entry ID.
    pub fn enqueue_with_encoder(
        &self,
        dest: PathBuf,
        settings: ExportSettings,
        project: Arc<Project>,
        plugin_encoder: Option<String>,
    ) -> u64 {
        let id = {
            let mut inner = self.shared.inner.lock().expect("queue lock");
            let id = inner.next_id;
            inner.next_id += 1;
            let label = format!("Exporting {}", file_label(&dest));
            let entry = ExportQueueEntry {
                id,
                label,
                dest,
                settings,
                project,
                plugin_encoder,
                state: ExportEntryState::Queued,
                progress: None,
                error: None,
                job_id: None,
                cancel_requested: false,
                created: Instant::now(),
                finished: None,
            };
            inner.entries.push(entry);
            id
        };
        self.shared.pump_internal();
        id
    }

    /// Cancel a queue entry by ID.
    ///
    /// - If `Queued`: removes the entry from the queue and returns `true`.
    /// - If `Running`: cancels the running job in `JobManager`, marks cancel requested, and returns `true`.
    /// - If terminal or unknown: returns `false`.
    pub fn cancel(&self, id: u64) -> bool {
        let job_to_cancel: Option<JobId>;
        {
            let mut inner = self.shared.inner.lock().expect("queue lock");
            let Some(pos) = inner.entries.iter().position(|e| e.id == id) else {
                return false;
            };
            let entry = &mut inner.entries[pos];
            match entry.state {
                ExportEntryState::Queued => {
                    inner.entries.remove(pos);
                    return true;
                }
                ExportEntryState::Running => {
                    entry.cancel_requested = true;
                    job_to_cancel = entry.job_id;
                }
                ExportEntryState::Done | ExportEntryState::Failed | ExportEntryState::Cancelled => {
                    return false;
                }
            }
        }

        if let Some(job_id) = job_to_cancel {
            self.shared.job_manager.cancel(job_id);
            true
        } else {
            true
        }
    }

    /// Point-in-time view of all queue entries, in queue order.
    pub fn snapshot(&self) -> Vec<ExportQueueSnapshot> {
        let inner = self.shared.inner.lock().expect("queue lock");
        inner
            .entries
            .iter()
            .map(|entry| {
                let (progress, status) = match entry.state {
                    ExportEntryState::Queued => (None, "Queued"),
                    ExportEntryState::Running => {
                        let p = entry
                            .job_id
                            .and_then(|jid| self.shared.job_manager.get(jid))
                            .and_then(|snap| snap.progress)
                            .or(entry.progress);
                        (p, "Running")
                    }
                    ExportEntryState::Done => (Some(1.0), "Done"),
                    ExportEntryState::Failed => (entry.progress, "Failed"),
                    ExportEntryState::Cancelled => (entry.progress, "Cancelled"),
                };
                let elapsed = match entry.finished {
                    Some(end) => end.duration_since(entry.created).as_secs_f32(),
                    None => entry.created.elapsed().as_secs_f32(),
                };
                ExportQueueSnapshot {
                    id: entry.id,
                    label: entry.label.to_string(),
                    dest: entry.dest.to_path_buf(),
                    state: entry.state,
                    status,
                    progress,
                    error: entry.error.as_ref().map(|e| e.to_string()),
                    cancellable: matches!(
                        entry.state,
                        ExportEntryState::Queued | ExportEntryState::Running
                    ),
                    terminal: entry.state.is_terminal(),
                    elapsed,
                }
            })
            .collect()
    }

    /// True when the queue has no active or pending jobs.
    pub fn is_idle(&self) -> bool {
        let inner = self.shared.inner.lock().expect("queue lock");
        !inner.entries.iter().any(|e| !e.state.is_terminal())
    }

    /// Number of jobs currently queued or running.
    pub fn active_count(&self) -> usize {
        let inner = self.shared.inner.lock().expect("queue lock");
        inner
            .entries
            .iter()
            .filter(|e| !e.state.is_terminal())
            .count()
    }

    /// Wait until all currently enqueued and running jobs reach terminal state, up to `timeout`.
    /// Returns `true` if all settled, `false` if timed out.
    pub fn settle_all(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_idle() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
