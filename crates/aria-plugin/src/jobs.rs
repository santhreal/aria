//! Plugin jobs: background work listed on the host's jobs board with
//! progress and cancel.
//!
//! A job runs on the host's [`JobManager`], the registry the board draws, on a
//! thread of its own. Its label starts with the plugin's name. A panic in the
//! work fails the plugin for the session, and unmounting or failing a plugin
//! cancels every job it started that has not reached its commit step. A job
//! started while its plugin mounts waits for the mount to finish, and does not
//! run when the mount fails.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use aria_jobs::{JobCompletion, JobContext, JobId, JobManager};
use parking_lot::{Condvar, Mutex};

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::listed::owner;

thread_local! {
    /// The plugin job running on this thread. A job that fails its own plugin
    /// ends failed: the cancel the failure sends every job skips it.
    static RUNNING: Cell<Option<JobId>> = const { Cell::new(None) };
}

/// One step of a job a WASM or native plugin runs in steps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobStep {
    /// Fraction done, `0.0..=1.0`, or `None` while it is unknown.
    pub progress: Option<f32>,
    /// The line the jobs board shows.
    pub detail: String,
    /// The job is finished, and `detail` is its last line.
    pub done: bool,
    /// The next step cannot be undone: Cancel is refused from here on.
    pub commit: bool,
}

/// The jobs each plugin started, and the mount in progress.
#[derive(Debug, Default)]
pub(crate) struct PluginJobs {
    started: Mutex<Vec<(Arc<str>, JobId)>>,
    mounting: Mutex<Option<(Arc<str>, Arc<Gate>)>>,
}

/// Opens when a mount finishes, successful or not.
#[derive(Debug, Default)]
struct Gate {
    open: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock();
        while !*open {
            self.opened.wait(&mut open);
        }
    }
}

impl PluginJobs {
    /// Hold the jobs `plugin` starts until [`release`](Self::release).
    pub(crate) fn hold(&self, plugin: &str) {
        *self.mounting.lock() = Some((Arc::from(plugin), Arc::default()));
    }

    /// Let the held jobs run. After a failed mount, cancel them first.
    pub(crate) fn release(&self) {
        if let Some((_, gate)) = self.mounting.lock().take() {
            *gate.open.lock() = true;
            gate.opened.notify_all();
        }
    }

    fn gate(&self, plugin: &str) -> Option<Arc<Gate>> {
        match &*self.mounting.lock() {
            Some((mounting, gate)) if **mounting == *plugin => Some(Arc::clone(gate)),
            _ => None,
        }
    }

    /// Cancel every job `plugin` started and forget them. A job past its
    /// commit step finishes.
    pub(crate) fn cancel_all(&self, plugin: &str, manager: Option<&JobManager>) {
        let running = RUNNING.get();
        let ids: Vec<JobId> = self
            .started
            .lock()
            .extract_if(.., |(owner, _)| **owner == *plugin)
            .map(|(_, id)| id)
            .filter(|id| Some(*id) != running)
            .collect();
        if let Some(manager) = manager {
            for id in ids {
                manager.cancel(id);
            }
        }
    }
}

impl PluginContext {
    /// The host's job registry, read without the calling plugin's service
    /// scope: starting a job needs no permission.
    pub(crate) fn job_manager(&self) -> Option<Arc<JobManager>> {
        self.scoped(None).try_get::<JobManager>()
    }

    /// Run `work` as a background job on the host's jobs board.
    ///
    /// `work` reports progress with [`JobContext::set_progress`], checks
    /// [`JobContext::cancelled`] between steps, and returns the line the board
    /// shows when it ends. Before an irreversible step it calls
    /// [`JobContext::try_begin_commit`]; past that point Cancel is refused.
    ///
    /// The board labels the job `<plugin name>: <label>`. A panic in `work`
    /// fails the plugin for the session and ends the job as failed.
    /// Unmounting or failing the plugin cancels the job. A job started during
    /// mount runs once the mount succeeds. Refused when the host registered no
    /// job registry.
    pub fn start_job(
        &self,
        label: &str,
        work: impl FnOnce(&JobContext) -> Result<String, String> + Send + 'static,
    ) -> Result<JobId, PluginError> {
        let Some(manager) = self.job_manager() else {
            return Err(PluginError::JobRefused(format!(
                "no job registry is mounted, so {label:?} cannot start"
            )));
        };
        let (plugin_id, plugin) = owner(self);
        let label = if plugin_id.is_empty() {
            label.to_owned()
        } else {
            format!("{plugin}: {label}")
        };
        let gate = self.job_registry().gate(&plugin_id);
        let mounting = gate.is_some();
        let ctx = self.downgrade();
        let failing = Arc::clone(&plugin_id);
        // Held across the spawn, so an unmount or a fault in the job cancels
        // the job only after it is listed.
        let mut started = self.job_registry().started.lock();
        let id = manager.spawn_with_completion(label, move |job| {
            if let Some(gate) = gate {
                gate.wait();
                if job.cancelled() {
                    return Err("the plugin did not mount".into());
                }
            }
            RUNNING.set(Some(job.id()));
            let outcome = catch_unwind(AssertUnwindSafe(|| work(job)));
            RUNNING.set(None);
            match outcome {
                Ok(result) => result.map(JobCompletion::new),
                Err(payload) => {
                    if !failing.is_empty()
                        && let Some(ctx) = ctx.upgrade()
                    {
                        let reason = format!("job panicked: {}", panic_message(payload.as_ref()));
                        ctx.fail(&failing, reason);
                    }
                    // The registry records the panic and ends the job failed.
                    resume_unwind(payload)
                }
            }
        });
        if plugin_id.is_empty() {
            return Ok(id);
        }
        started.retain(|(_, held)| {
            manager
                .get(*held)
                .is_some_and(|snap| !snap.state.is_terminal())
        });
        started.push((Arc::clone(&plugin_id), id));
        drop(started);
        // A plugin unmounted while this call ran leaves no job behind.
        if !mounting && !self.is_mounted(&plugin_id) {
            self.job_registry().cancel_all(&plugin_id, Some(&manager));
        }
        Ok(id)
    }

    /// Run a job in steps: `step` until a step is done, Cancel is accepted,
    /// or a step fails, then `end` with whether the job was cancelled.
    ///
    /// For plugins whose code the host calls one bounded call at a time, the
    /// WASM and native tiers. Progress and detail go to the board after each
    /// step; a step with `commit` closes Cancel before the next one runs.
    pub fn start_stepped_job(
        &self,
        label: &str,
        mut step: impl FnMut() -> Result<JobStep, String> + Send + 'static,
        end: impl FnOnce(bool) + Send + 'static,
    ) -> Result<JobId, PluginError> {
        self.start_job(label, move |board| {
            let mut committed = false;
            let result = loop {
                if !committed && board.cancelled() {
                    break Err(None);
                }
                let next = match step() {
                    Ok(next) => next,
                    Err(reason) => break Err(Some(reason)),
                };
                if next.done {
                    break Ok(next.detail);
                }
                match next.progress {
                    Some(fraction) => board.set_progress(fraction, next.detail),
                    None => board.set_detail(next.detail),
                }
                if next.commit && !committed {
                    if !board.try_begin_commit() {
                        break Err(None);
                    }
                    committed = true;
                }
            };
            end(matches!(result, Err(None)));
            result.map_err(|reason| reason.unwrap_or_else(|| "cancelled".to_owned()))
        })
    }
}
