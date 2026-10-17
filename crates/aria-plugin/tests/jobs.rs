//! Plugin jobs: a job a plugin starts runs on the host's registry under the
//! plugin's name, reports progress, ends when cancelled, is cancelled when its
//! plugin unmounts, and fails its plugin when it panics.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aria_plugin::{
    JobId, JobManager, JobSnapshot, JobState, JobStep, Plugin, PluginContext, PluginError,
    PluginManifest,
};
use parking_lot::Mutex;

/// What the job a [`Starter`] starts does.
#[derive(Clone, Copy)]
enum Work {
    /// Reports progress until cancelled.
    UntilCancelled,
    /// Panics with "boom".
    Panics,
    /// Commits, then waits until released, so a cancel arrives after commit.
    CommitsThenWaits,
}

/// Starts one job of `work` while mounting and records its id.
struct Starter {
    manifest: PluginManifest,
    work: Work,
    started: Arc<Mutex<Option<JobId>>>,
    release: Arc<Mutex<bool>>,
}

impl Starter {
    fn new(id: &'static str, work: Work) -> Self {
        Self {
            manifest: PluginManifest::new(id, "Starter", "0.1.0", &[], &[]),
            work,
            started: Arc::default(),
            release: Arc::default(),
        }
    }

    fn job(&self) -> JobId {
        self.started.lock().expect("the plugin started a job")
    }
}

impl Plugin for Starter {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let work = self.work;
        let release = Arc::clone(&self.release);
        let id = ctx.start_job("crunch", move |job| match work {
            Work::UntilCancelled => {
                let mut step = 0u32;
                while !job.cancelled() {
                    step = step.wrapping_add(1);
                    job.set_progress(0.25, format!("step {step}"));
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err("stopped".into())
            }
            Work::Panics => panic!("boom"),
            Work::CommitsThenWaits => {
                assert!(job.try_begin_commit(), "nothing cancelled before commit");
                while !*release.lock() {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok("written".into())
            }
        })?;
        *self.started.lock() = Some(id);
        Ok(())
    }
}

/// The host side: a context with a job registry registered as a service.
fn host() -> (PluginContext, Arc<JobManager>, aria_plugin::Registration) {
    let ctx = PluginContext::new();
    let manager = Arc::new(JobManager::new());
    let held = ctx.register_arc(Arc::clone(&manager));
    (ctx, manager, held)
}

/// The job's snapshot once `done` holds for it, within five seconds.
fn wait_for(manager: &JobManager, id: JobId, done: impl Fn(&JobSnapshot) -> bool) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snap = manager.get(id).expect("the registry keeps the job");
        if done(&snap) {
            return snap;
        }
        assert!(
            Instant::now() < deadline,
            "job {id} never reached the state: {snap:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_plugin_job_is_labelled_reports_progress_and_ends_on_cancel() {
    let (ctx, manager, _held) = host();
    let plugin = Starter::new("test.jobs.cancel", Work::UntilCancelled);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();

    let running = wait_for(&manager, id, |snap| snap.progress.is_some());
    assert_eq!(running.label, "Starter: crunch");
    assert_eq!(running.state, JobState::Running);
    assert_eq!(running.progress, Some(0.25));
    assert!(running.cancellable);

    assert!(manager.cancel(id), "the board's Cancel is accepted");
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Cancelled);
    assert!(ctx.is_mounted("test.jobs.cancel"), "a cancel is no fault");
}

#[test]
fn unmounting_a_plugin_cancels_its_running_jobs() {
    let (ctx, manager, _held) = host();
    let plugin = Starter::new("test.jobs.unmount", Work::UntilCancelled);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();
    wait_for(&manager, id, |snap| snap.state == JobState::Running);

    ctx.unmount("test.jobs.unmount").expect("unmounts");
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Cancelled);
}

#[test]
fn a_panicking_job_fails_its_plugin_and_ends_failed() {
    let (ctx, manager, _held) = host();
    let plugin = Starter::new("test.jobs.panic", Work::Panics);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();

    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Failed);
    let deadline = Instant::now() + Duration::from_secs(5);
    while ctx.is_mounted("test.jobs.panic") {
        assert!(
            Instant::now() < deadline,
            "the panic never failed the plugin"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let failure = ctx
        .failures()
        .into_iter()
        .find(|failure| failure.id == "test.jobs.panic")
        .expect("the failure is recorded");
    assert_eq!(failure.reason, "job panicked: boom");
    assert_eq!(ended.detail, "panicked: boom");
}

#[test]
fn a_committed_job_finishes_after_its_plugin_unmounts() {
    let (ctx, manager, _held) = host();
    let plugin = Starter::new("test.jobs.commit", Work::CommitsThenWaits);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();
    wait_for(&manager, id, |snap| !snap.cancellable);

    ctx.unmount("test.jobs.commit").expect("unmounts");
    *plugin.release.lock() = true;
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Done);
}

/// Starts a job that records it ran, then fails its own mount.
struct FailsMount {
    manifest: PluginManifest,
    ran: Arc<AtomicBool>,
    started: Arc<Mutex<Option<JobId>>>,
}

impl Plugin for FailsMount {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let ran = Arc::clone(&self.ran);
        let id = ctx.start_job("never", move |_| {
            ran.store(true, Ordering::SeqCst);
            Ok("ran".into())
        })?;
        *self.started.lock() = Some(id);
        Err(PluginError::MountFailed(
            self.manifest.id.clone(),
            "declines".into(),
        ))
    }
}

#[test]
fn a_job_started_by_a_failed_mount_never_runs() {
    let (ctx, manager, _held) = host();
    let plugin = FailsMount {
        manifest: PluginManifest::new("test.jobs.failed", "Failed", "0.1.0", &[], &[]),
        ran: Arc::default(),
        started: Arc::default(),
    };
    ctx.mount(&plugin).expect_err("the plugin declines");
    let id = plugin.started.lock().expect("the job was started");
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Cancelled);
    assert!(
        !plugin.ran.load(Ordering::SeqCst),
        "the job ran for a plugin that never mounted"
    );
}

#[test]
fn a_job_without_a_registry_is_refused_and_fails_the_mount() {
    let ctx = PluginContext::new();
    let plugin = Starter::new("test.jobs.none", Work::UntilCancelled);
    let err = ctx.mount(&plugin).expect_err("no registry, no job");
    assert!(
        err.to_string()
            .contains("no job registry is mounted, so \"crunch\" cannot start"),
        "{err}"
    );
}

#[test]
fn the_hosts_own_job_keeps_its_label() {
    let (ctx, manager, _held) = host();
    let id = ctx
        .start_job("Rebuilding caches", |_| Ok("rebuilt".into()))
        .expect("the host starts a job");
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.label, "Rebuilding caches");
    assert_eq!(ended.state, JobState::Done);
}

/// How the steps of a [`Stepper`] job behave.
#[derive(Clone, Copy)]
enum Steps {
    /// Three steps of a third each, then done.
    Three,
    /// Steps until cancelled.
    Endless,
    /// The first step commits; later steps wait until released.
    CommitFirst,
    /// The first step fails its own plugin, as a WASM trap does.
    FailsPlugin,
}

/// Starts one stepped job while mounting and records its id and its end.
struct Stepper {
    manifest: PluginManifest,
    steps: Steps,
    started: Arc<Mutex<Option<JobId>>>,
    ended: Arc<Mutex<Option<bool>>>,
    release: Arc<AtomicBool>,
}

impl Stepper {
    fn new(id: &'static str, steps: Steps) -> Self {
        Self {
            manifest: PluginManifest::new(id, "Stepper", "0.1.0", &[], &[]),
            steps,
            started: Arc::default(),
            ended: Arc::default(),
            release: Arc::default(),
        }
    }

    fn job(&self) -> JobId {
        self.started.lock().expect("the plugin started a job")
    }
}

impl Plugin for Stepper {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let steps = self.steps;
        let release = Arc::clone(&self.release);
        let ended = Arc::clone(&self.ended);
        let failing = ctx.downgrade();
        let id = self.manifest.id.to_string();
        let mut taken = 0u32;
        let job = ctx.start_stepped_job(
            "render",
            move || {
                taken += 1;
                std::thread::sleep(Duration::from_millis(2));
                match steps {
                    Steps::Three => Ok(JobStep {
                        progress: Some(taken as f32 / 3.0),
                        detail: if taken == 3 {
                            "rendered 3".into()
                        } else {
                            format!("step {taken}")
                        },
                        done: taken == 3,
                        commit: false,
                    }),
                    Steps::Endless => Ok(JobStep {
                        progress: None,
                        detail: format!("step {taken}"),
                        ..JobStep::default()
                    }),
                    Steps::CommitFirst => {
                        while taken > 1 && !release.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Ok(JobStep {
                            progress: Some(0.5),
                            detail: "writing".into(),
                            done: taken > 1,
                            commit: taken == 1,
                        })
                    }
                    Steps::FailsPlugin => {
                        if let Some(ctx) = failing.upgrade() {
                            ctx.fail(&id, "trapped");
                        }
                        Err("trapped".into())
                    }
                }
            },
            move |cancelled| *ended.lock() = Some(cancelled),
        )?;
        *self.started.lock() = Some(job);
        Ok(())
    }
}

#[test]
fn a_stepped_job_reports_each_step_and_ends_with_the_last() {
    let (ctx, manager, _held) = host();
    let plugin = Stepper::new("test.steps.three", Steps::Three);
    ctx.mount(&plugin).expect("mounts");
    let ended = wait_for(&manager, plugin.job(), |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Done);
    assert_eq!(ended.detail, "rendered 3");
    assert_eq!(ended.label, "Stepper: render");
    assert_eq!(*plugin.ended.lock(), Some(false));
}

#[test]
fn cancelling_a_stepped_job_stops_its_steps_and_ends_it_cancelled() {
    let (ctx, manager, _held) = host();
    let plugin = Stepper::new("test.steps.endless", Steps::Endless);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();
    let running = wait_for(&manager, id, |snap| snap.detail.starts_with("step "));
    assert_eq!(
        running.progress, None,
        "a step without a fraction stays indeterminate"
    );
    assert!(manager.cancel(id));
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Cancelled);
    assert_eq!(*plugin.ended.lock(), Some(true));
}

#[test]
fn a_commit_step_refuses_cancel_and_the_job_finishes() {
    let (ctx, manager, _held) = host();
    let plugin = Stepper::new("test.steps.commit", Steps::CommitFirst);
    ctx.mount(&plugin).expect("mounts");
    let id = plugin.job();
    wait_for(&manager, id, |snap| !snap.cancellable);
    assert!(!manager.cancel(id), "Cancel after a commit step is refused");
    plugin.release.store(true, Ordering::SeqCst);
    let ended = wait_for(&manager, id, |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Done);
    assert_eq!(*plugin.ended.lock(), Some(false));
}

#[test]
fn a_step_that_fails_its_plugin_ends_the_job_failed_not_cancelled() {
    let (ctx, manager, _held) = host();
    let plugin = Stepper::new("test.steps.fails", Steps::FailsPlugin);
    ctx.mount(&plugin).expect("mounts");
    let ended = wait_for(&manager, plugin.job(), |snap| snap.state.is_terminal());
    assert_eq!(ended.state, JobState::Failed);
    assert_eq!(ended.detail, "trapped");
    assert!(!ctx.is_mounted("test.steps.fails"));
    assert_eq!(*plugin.ended.lock(), Some(false));
}
