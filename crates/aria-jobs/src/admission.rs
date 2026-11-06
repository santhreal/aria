//! Thread admission: at most `concurrency_limit` jobs run at once, and a job
//! spawned past the limit waits in [`JobState::Queued`] for the next free slot.
//!
//! A slot passes from a finishing job straight to the oldest waiting one, under
//! the same lock that decides whether a new job waits, so a job never waits
//! with no running job left to start it.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::{
    CancellationToken, Job, JobCompletion, JobContext, JobId, JobKind, JobManager, JobSnapshot,
    JobState, Shared, deliver_to_subscribers, panic_message, sanitize_detail, sanitize_label,
    thread_name,
};

/// The closure a job runs.
pub(crate) type Work = Box<dyn FnOnce(&JobContext) -> Result<JobCompletion, String> + Send>;

/// A job that has not started.
pub(crate) struct Waiting {
    pub(crate) id: JobId,
    pub(crate) cancellation: CancellationToken,
    pub(crate) thread_name: String,
    pub(crate) work: Work,
}

/// The jobs waiting for a slot, oldest first.
pub(crate) type Queue = VecDeque<Waiting>;

/// The detail of a job cancelled before it started.
pub(crate) const CANCELLED_WHILE_WAITING: &str = "cancelled before it started";

impl Shared {
    /// Start `job` now if a slot is free and nothing waits ahead of it;
    /// otherwise hold it until a running job leaves.
    pub(crate) fn admit(self: &Arc<Self>, job: Waiting) {
        {
            let mut queue = self.waiting.lock();
            let free = queue.is_empty()
                && self
                    .concurrency_limit
                    .is_none_or(|limit| self.active_threads.load(Ordering::Acquire) < limit);
            if !free {
                queue.push_back(job);
                return;
            }
            self.active_threads.fetch_add(1, Ordering::AcqRel);
        }
        self.run_on_slot(Some(job));
    }

    /// Spend the slot the caller holds on `first`, else on the oldest waiting
    /// job, else free it. A job that does not get a thread (cancelled while it
    /// waited, or the thread would not start) is finished here and the slot
    /// goes to the next one, in a loop rather than by recursion.
    fn run_on_slot(self: &Arc<Self>, first: Option<Waiting>) {
        let mut next = first;
        loop {
            let job = match next.take() {
                Some(job) => job,
                None => {
                    let mut queue = self.waiting.lock();
                    match queue.pop_front() {
                        Some(job) => job,
                        None => {
                            if self.active_threads.fetch_sub(1, Ordering::AcqRel) == 1 {
                                self.done_cv.notify_all();
                            }
                            return;
                        }
                    }
                }
            };
            if self.launch(job) {
                return;
            }
        }
    }

    /// Start `job` on its own thread, which then holds the slot. Returns
    /// `false` when no thread started and the slot is still the caller's.
    fn launch(self: &Arc<Self>, job: Waiting) -> bool {
        let Waiting {
            id,
            cancellation,
            thread_name,
            work,
        } = job;
        if cancellation.cancelled() {
            self.update(id, |job| {
                job.snap.state = JobState::Cancelled;
                job.snap.detail = CANCELLED_WHILE_WAITING.into();
                job.snap.finished = Some(Instant::now());
            });
            return false;
        }
        let context = JobContext {
            id,
            shared: Arc::clone(self),
            cancellation,
        };
        let shared = Arc::clone(self);
        let spawned = thread::Builder::new().name(thread_name).spawn(move || {
            run(&shared, &context, work);
            // The context holds the registry; it goes before the slot is
            // passed on, so a manager dropped meanwhile frees with its
            // last job.
            drop(context);
            shared.run_on_slot(None);
        });
        if let Err(err) = spawned {
            // Thread spawn failure (resource exhaustion) is the one way a job
            // dies without running; surface it like any other failure.
            let detail = sanitize_detail(&format!("failed to spawn thread: {err}"));
            self.update(id, |job| {
                job.snap.state = JobState::Failed;
                job.snap.detail = detail;
                job.snap.outputs.clear();
                job.snap.finished = Some(Instant::now());
            });
            return false;
        }
        true
    }
}

/// Run one job's work on its thread and publish its terminal state.
fn run(shared: &Shared, context: &JobContext, work: Work) {
    let id = context.id;
    shared.update(id, |job| job.snap.state = JobState::Running);
    let result = catch_unwind(AssertUnwindSafe(|| work(context)));
    let (base_state, detail, outputs, panicked) = match result {
        // Panic wins over cancellation: a job that blows up on the way out is
        // a bug to surface, not a clean stop.
        Err(payload) => (
            JobState::Failed,
            format!("panicked: {}", panic_message(&*payload)),
            Vec::new(),
            true,
        ),
        Ok(Ok(completion)) => (JobState::Done, completion.detail, completion.outputs, false),
        Ok(Err(reason)) => (JobState::Failed, reason, Vec::new(), false),
    };
    // Normalize caller-sized text before taking the registry lock. The
    // bounded result is still published atomically with terminal state and
    // structured outputs.
    let detail = sanitize_detail(&detail);
    shared.update(id, |job| {
        // Decided under the same registry lock as `cancel`: if cancellation
        // wins the race before this terminal write, `cancel()` returned true
        // and the terminal state must agree. If this write wins, cancel sees
        // terminal and returns false. Panic remains a failure either way.
        let state = if !panicked && job.cancel.load(Ordering::Relaxed) {
            JobState::Cancelled
        } else {
            base_state
        };
        job.snap.state = state;
        job.snap.detail = detail;
        if state == JobState::Done {
            job.snap.outputs = outputs;
            // A progress-reporting job that completed is 100% done;
            // indeterminate jobs stay indeterminate.
            job.snap.progress = job.snap.progress.map(|_| 1.0);
        } else {
            job.snap.outputs.clear();
        }
        job.snap.finished = Some(Instant::now());
    });
}
impl JobManager {
    /// Spawn a typed job whose [`JobKind`] is stored as data on its snapshot.
    pub fn spawn_typed(
        &self,
        kind: JobKind,
        label: impl Into<String>,
        work: impl FnOnce(&JobContext) -> Result<String, String> + Send + 'static,
    ) -> JobId {
        self.spawn_with_target(kind, None, label, move |context| {
            work(context).map(JobCompletion::new)
        })
    }

    /// Spawn a typed job with structured completion outputs.
    pub fn spawn_with_completion_typed(
        &self,
        kind: JobKind,
        label: impl Into<String>,
        work: impl FnOnce(&JobContext) -> Result<JobCompletion, String> + Send + 'static,
    ) -> JobId {
        self.spawn_with_target(kind, None, label, work)
    }

    /// Spawn a typed job with an optional deduplication `target`.
    ///
    /// Any non-terminal job of the same `kind` and `target` (defaulting to the
    /// sanitized `label`) is cancelled before the new job is admitted, so two
    /// runs of the same job on one clip or target never stay active together.
    pub fn spawn_with_target(
        &self,
        kind: JobKind,
        target: Option<String>,
        label: impl Into<String>,
        work: impl FnOnce(&JobContext) -> Result<JobCompletion, String> + Send + 'static,
    ) -> JobId {
        let label = sanitize_label(&label.into());
        let dedup_target = Some(target.unwrap_or_else(|| label.clone()));
        let thread_name = thread_name(&label);
        let cancel = Arc::new(AtomicBool::new(false));

        let (id, snapshot, subscribers) = {
            let mut inner = self.shared.inner.lock();
            let crate::Inner {
                jobs,
                status_reported,
                ..
            } = &mut *inner;
            for existing in jobs.iter_mut() {
                if existing.snap.state.is_terminal() {
                    status_reported.insert(existing.snap.id);
                    continue;
                }
                if existing.cancellation_open
                    && existing.snap.kind == kind
                    && existing.target == dedup_target
                {
                    existing.cancel.store(true, Ordering::Relaxed);
                    existing.snap.cancel_requested = true;
                }
            }
            let id = JobId(self.shared.next_id.fetch_add(1, Ordering::Relaxed));
            let snap = JobSnapshot {
                id,
                kind,
                label,
                state: JobState::Queued,
                cancellable: true,
                progress: None,
                detail: String::new(),
                outputs: Vec::new(),
                cancel_requested: false,
                started: Instant::now(),
                finished: None,
            };
            inner.jobs.push(Job {
                snap: snap.clone(),
                target: dedup_target,
                cancel: Arc::clone(&cancel),
                cancellation_open: true,
            });
            (id, snap, inner.subscribers.clone())
        };
        deliver_to_subscribers(&snapshot, &subscribers);

        self.shared.admit(Waiting {
            id,
            cancellation: CancellationToken::new(cancel),
            thread_name,
            work: Box::new(work),
        });
        id
    }

    /// Current project epoch; incremented whenever a project is closed or replaced.
    pub fn project_epoch(&self) -> u64 {
        self.shared.project_epoch.load(Ordering::Acquire)
    }

    /// Advance the project epoch and cancel every queued or running job from the
    /// previous project.
    pub fn advance_project_epoch(&self) -> u64 {
        let next = self.shared.project_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        {
            let mut inner = self.shared.inner.lock();
            inner.owned_statuses.clear();
            let ids: Vec<JobId> = inner.jobs.iter().map(|j| j.snap.id).collect();
            inner.status_reported.extend(ids);
        }
        self.cancel_all();
        next
    }
    /// Request cancellation for every queued or running job, immediately
    /// draining waiting jobs to [`JobState::Cancelled`].
    pub fn cancel_all(&self) {
        {
            let mut inner = self.shared.inner.lock();
            for job in &mut inner.jobs {
                if !job.snap.state.is_terminal() && job.cancellation_open {
                    job.cancel.store(true, Ordering::Relaxed);
                    job.snap.cancel_requested = true;
                }
            }
        }
        let drained: Vec<Waiting> = {
            let mut queue = self.shared.waiting.lock();
            queue.drain(..).collect()
        };
        for waiting in drained {
            self.shared.update(waiting.id, |job| {
                job.snap.state = JobState::Cancelled;
                job.snap.detail = CANCELLED_WHILE_WAITING.into();
                job.snap.finished = Some(Instant::now());
            });
        }
        if self.shared.active_threads.load(Ordering::Acquire) == 0 {
            self.shared.done_cv.notify_all();
        }
    }

    /// Wait up to `timeout` for every queued and running job to finish.
    pub fn settle_all(&self, timeout: Duration) -> bool {
        let mut queue = self.shared.waiting.lock();
        let wait = self.shared.done_cv.wait_while_for(
            &mut queue,
            |q| !q.is_empty() || self.shared.active_threads.load(Ordering::Acquire) > 0,
            timeout,
        );
        !wait.timed_out()
    }

    /// Cancel every queued and running job and wait up to `timeout` for worker
    /// threads to exit.
    pub fn shutdown(&self, timeout: Duration) -> bool {
        self.cancel_all();
        self.settle_all(timeout)
    }

    /// Record that `line` was written to the status strip on behalf of a job.
    pub fn record_status_line(&self, line: impl Into<String>) {
        let line = line.into();
        if !line.is_empty() {
            self.shared.inner.lock().owned_statuses.insert(line);
        }
    }

    /// Whether `current` is absent or is a status strip line owned by a job.
    pub fn owns_status_line(&self, current: Option<&str>) -> bool {
        let Some(line) = current else {
            return true;
        };
        if line == "Cancelling…" {
            return true;
        }
        self.shared.inner.lock().owned_statuses.contains(line)
    }

    /// Mark `id` as having had its terminal status-strip outcome evaluated.
    /// Returns `true` the first time `id` is marked.
    pub fn mark_terminal_status_reported(&self, id: JobId) -> bool {
        self.shared.inner.lock().status_reported.insert(id)
    }
}
