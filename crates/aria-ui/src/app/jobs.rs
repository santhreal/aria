//! Background jobs: the projection the panel renders, and the export producer.
//!
//! `aria-jobs` owns the lifecycle; nothing here reimplements a thread, a flag, or a
//! progress channel. What this module adds is:
//!
//! - a projection ([`UiJob`]) so the panel never touches an `Instant` or a `JobId`,
//! - the export producer, which runs on its own renderer over a cloned project so a
//!   two-minute export never holds the engine lock the preview needs,
//! - a cancel path that goes through the registry rather than a private flag.
//!
//! Progress arrives by subscription, not by a timer: [`AppState::watch_jobs`] installs
//! one subscriber that wakes the entity per transition, so an idle app does no work.

use std::path::PathBuf;

use aria_jobs::{JobId, JobSnapshot, JobState};
use gpui::Context;

use super::AppState;
use super::export_request::ExportRequest;
use gpui::SharedString;

/// The one-shot publication boundary every job that writes shares.
///
/// [`aria_jobs::JobContext::try_begin_commit`] is the ordering point against
/// `cancel`, and it is deliberately idempotent: a second call from the same context
/// returns `true` again. That is right for the registry and wrong for a caller, which
/// wants the other half of the rule — one publication per job. This guard adds it, so a
/// job that grows a second write path is refused by name instead of writing twice.
///
/// Take it immediately before the irreversible step (a database batch, an atomic file
/// publish), never at the top of the job: everything before the boundary is what
/// cancellation is allowed to throw away.
pub(crate) struct CommitGuard<'a> {
    context: &'a aria_jobs::JobContext,
    what: &'static str,
    taken: bool,
}

impl<'a> CommitGuard<'a> {
    pub(crate) fn new(context: &'a aria_jobs::JobContext, what: &'static str) -> Self {
        Self {
            context,
            what,
            taken: false,
        }
    }

    /// Win the boundary, or say why publication must be skipped.
    pub(crate) fn begin(&mut self) -> Result<(), String> {
        if self.taken {
            return Err(format!("{} was already committed", self.what));
        }
        if !self.context.try_begin_commit() {
            return Err(format!("cancelled before {} was written", self.what));
        }
        self.taken = true;
        Ok(())
    }

    /// Whether this job has published.
    pub(crate) fn committed(&self) -> bool {
        self.taken
    }
}

/// Typed category of a background job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Transcription,
    Download,
    Analysis,
    Loudness,
    Export,
    Generation,
    Other,
}

/// One job as the panel needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct UiJob {
    pub id: u64,
    pub label: SharedString,
    pub kind: JobKind,
    /// "Queued", "Running", "Cancelling…", "Done", "Failed", "Cancelled".
    pub status: &'static str,
    /// `None` for indeterminate work, so the panel can tell a job that reports
    /// nothing from one that reports zero.
    pub progress: Option<f32>,
    pub detail: SharedString,
    /// Whether Cancel would still be accepted. False past the commit boundary, so the
    /// panel hides the button instead of offering one the registry refuses.
    pub cancellable: bool,
    pub terminal: bool,
    /// Machine-readable results, present only on a job that finished successfully.
    pub outputs: Vec<(String, String)>,
    /// Seconds since the job was registered, or its total run time once finished.
    pub elapsed: f32,
}

impl UiJob {
    /// Whether a surface should offer to stop this job.
    ///
    /// The rule is the job's, not each surface's: a panel row, the board, and the
    /// export sheet all show the same work, and one of them deciding differently
    /// is an operator watching a running job with no way to stop it. False past
    /// the commit boundary and on anything terminal, so the control is never
    /// offered for a request the registry would refuse.
    pub fn stoppable(&self) -> bool {
        self.cancellable && !self.terminal
    }

    pub(crate) fn from_snapshot(snapshot: &JobSnapshot) -> Self {
        // "Cancelling…" is a distinct status: a cancel that has been accepted but whose
        // thread has not exited yet looks stuck otherwise.
        let status = match (snapshot.state, snapshot.cancel_requested) {
            (JobState::Cancelled, _) => "Cancelled",
            (JobState::Failed, _) => "Failed",
            (JobState::Done, _) => "Done",
            (_, true) => "Cancelling…",
            (JobState::Running, false) => "Running",
            (JobState::Queued, false) => "Queued",
        };
        let elapsed = match snapshot.finished {
            Some(end) => end.duration_since(snapshot.started).as_secs_f32(),
            None => snapshot.started.elapsed().as_secs_f32(),
        };
        let kind = if snapshot.label.starts_with("Transcribing ") {
            JobKind::Transcription
        } else if snapshot.label.starts_with("Downloading ") {
            JobKind::Download
        } else if snapshot.label.starts_with("Analyzing ") {
            JobKind::Analysis
        } else if snapshot.label.starts_with("Measuring ") {
            JobKind::Loudness
        } else if snapshot.label.starts_with("Exporting ") {
            JobKind::Export
        } else if snapshot.label.starts_with("Generating ") {
            JobKind::Generation
        } else {
            JobKind::Other
        };
        Self {
            id: snapshot.id.raw(),
            label: snapshot.label.clone().into(),
            kind,
            status,
            progress: snapshot.progress,
            detail: snapshot.detail.clone().into(),
            cancellable: snapshot.cancellable,
            terminal: snapshot.state.is_terminal(),
            outputs: snapshot
                .outputs
                .iter()
                .map(|output| (output.name().to_string(), output.value().to_string()))
                .collect(),
            elapsed,
        }
    }
}

impl AppState {
    /// Register one background job, and project the board it just changed.
    ///
    /// The one place work is handed to the registry from this side. The
    /// subscriber's wake refreshes too, but it lands a scheduler hop later and
    /// the operator has already pressed the button: the row exists in the same
    /// update that started the work.
    pub(crate) fn spawn_job(
        &mut self,
        label: impl Into<String>,
        work: impl FnOnce(&aria_jobs::JobContext) -> Result<aria_jobs::JobCompletion, String>
        + Send
        + 'static,
    ) -> JobId {
        let id = self.jobs.spawn_with_completion(label, work);
        self.refresh_jobs();
        id
    }

    /// Rebuild the board's projection from the registry and the export queue.
    ///
    /// Called wherever the list can change and nowhere else: the subscriber's
    /// wake ([`Self::watch_jobs`]) covers every state and progress transition,
    /// because the registry publishes to its subscribers on registration too,
    /// and the two places that change the export queue from this side —
    /// enqueueing and cancelling — refresh directly, since a queued entry has
    /// no registry job until it starts.
    ///
    /// A panel used to build this itself, every frame: `snapshot` takes the
    /// registry's mutex and clones every job's label, detail and outputs, and a
    /// frame with the pending strip up did that four times while the job threads
    /// writing progress contended for the same lock.
    pub(crate) fn refresh_jobs(&mut self) {
        let mut rows = Vec::new();
        for entry in self.export_queue.snapshot() {
            if entry.state == crate::app::export_queue::ExportEntryState::Queued {
                rows.push(UiJob {
                    id: entry.id,
                    label: entry.label.to_string().into(),
                    kind: JobKind::Export,
                    status: "Queued",
                    progress: None,
                    detail: "Waiting in render queue".to_string().into(),
                    cancellable: true,
                    terminal: false,
                    outputs: Vec::new(),
                    elapsed: entry.elapsed,
                });
            }
        }
        rows.extend(self.jobs.snapshot().iter().map(UiJob::from_snapshot));
        self.job_view = rows.into();
    }

    /// How many jobs are still running or queued, for the status line.
    ///
    /// Counted off the same projection the rows are drawn from, so the number
    /// and the list can never disagree about what is running.
    pub fn running_job_count(&self) -> usize {
        self.job_view.iter().filter(|job| !job.terminal).count()
    }

    /// Ask the registry or export queue to cancel a job.
    ///
    /// A refusal is reported rather than swallowed: past the commit boundary the work
    /// is finishing whatever the button says, and a Cancel that silently does nothing
    /// is the worse surface.
    pub fn cancel_job(&mut self, id: u64) {
        if self.export_queue.cancel(id) {
            self.refresh_jobs();
            self.status_message = Some("Cancelling…".into());
            return;
        }
        if self.jobs.cancel(JobId::from_raw(id)) {
            self.status_message = Some("Cancelling…".into());
        } else {
            self.status_message = Some("Too late to cancel".into());
        }
    }

    /// Install the one job subscriber for this session.
    ///
    /// The subscriber runs on the job's own thread and must not touch the entity, so it
    /// only sends a wake. A task on the window's thread awaits that wake and notifies
    /// the entity per transition — no polling timer, no thread waiting, and nothing
    /// runs while jobs are idle.
    ///
    /// A wake carries whether the job reached a terminal state, because a job that
    /// finished may have changed what the engine holds — a written proxy is
    /// registered as a substitution by the job itself, and a projection built
    /// before that is a projection of a decode path that is no longer in force.
    /// Only the terminal wake re-reads the engine: progress arrives per
    /// transition, and rebuilding the store on every progress tick would make a
    /// two-minute export cost a projection per frame. The board's own
    /// projection is rebuilt on every wake, which is what a panel reads.
    pub fn watch_jobs(&mut self, cx: &mut Context<Self>) {
        if self.jobs_watched {
            return;
        }
        self.jobs_watched = true;
        let (wake, wakes) = crate::handoff::wakes::<bool>();
        self.jobs.subscribe(move |snapshot| {
            wake.wake(snapshot.state.is_terminal());
        });
        cx.spawn(async move |this, cx| {
            loop {
                // A job runs on its own thread, so the wake is found by the
                // scheduler rather than delivered by that thread. `None` is every
                // writer gone: the manager outlives the app, so that is shutdown.
                let Some(finished) = wakes.next_from_a_thread(cx.background_executor()).await
                else {
                    return;
                };
                let updated = this.update(cx, |state, cx| {
                    if finished {
                        state.sync_from_engine();
                    }
                    state.refresh_jobs();
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Export as a job: determinate progress, cancellable, off the UI thread.
    ///
    /// The job renders through its *own* headless renderer over a cloned project. The
    /// session's renderer is the preview's, and an export that borrowed it would hold
    /// the engine lock for the whole pass — the preview would freeze for minutes, which
    /// is exactly what a progress bar is supposed to make unnecessary.
    pub fn export_job(&mut self, path: PathBuf) -> Result<u64, String> {
        let request = self.export_request(path);
        self.enqueue_export(request)
    }

    /// Queue the export a request describes.
    ///
    /// The request is what the sheet asked for; this is where the machine gets a
    /// say. One resolution against this host — a missing encoder, a codec or a
    /// bitrate it will not honour, and a span with no frames in it — is refused
    /// here, by name, before a job exists. Spawning first would report the same
    /// thing a minute into a render.
    pub fn enqueue_export(&mut self, request: ExportRequest) -> Result<u64, String> {
        let project = {
            let engine = self.engine.lock();
            engine.project().clone()
        };
        let settings = aria_render::ExportSettings::resolved(&project, request.format)
            .map_err(|err| err.to_string())?;
        let entry_id =
            self.export_queue
                .enqueue(request.destination, settings, std::sync::Arc::new(project));
        self.refresh_jobs();
        let (w, h) = request.size;
        self.status_message = Some(format!(
            "Export enqueued · {w} × {h} · {} · {}",
            if settings.audio {
                "with audio"
            } else {
                "video only"
            },
            request.range
        ));
        Ok(entry_id)
    }

    /// Point-in-time view of all render queue entries.
    pub fn export_queue_snapshot(&self) -> Vec<crate::app::export_queue::ExportQueueSnapshot> {
        self.export_queue.snapshot()
    }

    /// Show a finished job's artifact in the platform file manager.
    ///
    /// The directory is opened rather than the file: opening the file would launch
    /// whatever plays an `.mp4`, which is not what "reveal" means, and a codec the
    /// system cannot play would look like a failed export.
    pub fn reveal_path(&mut self, value: &str) {
        let path = std::path::Path::new(value);
        let target = if path.is_dir() {
            path
        } else {
            match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                // A bare filename has no directory to open, and guessing the working
                // directory would reveal the wrong place.
                _ => {
                    self.report_error(format!("{value}: no folder to open"));
                    return;
                }
            }
        };
        if !target.exists() {
            self.report_error(format!("{}: the folder is gone", target.display()));
            return;
        }
        match crate::app::launch::open_folder(target) {
            Ok(()) => self.status_message = Some(format!("Opened {}", target.display())),
            Err(err) => self.report_error(err),
        }
    }
}

/// A path's filename, or the whole path when it has none.
pub(crate) fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}
