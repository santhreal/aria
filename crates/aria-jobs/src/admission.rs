//! Thread admission: at most `concurrency_limit` jobs run at once, and a job
//! spawned past the limit waits in [`JobState::Queued`] for the next free slot.
//!
//! A slot passes from a finishing job straight to the oldest waiting one, under
//! the same lock that decides whether a new job waits, so a job never waits
//! with no running job left to start it.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Instant;

use crate::{
    CancellationToken, JobCompletion, JobContext, JobId, JobState, Shared, panic_message,
    sanitize_detail,
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
                            self.active_threads.fetch_sub(1, Ordering::AcqRel);
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
