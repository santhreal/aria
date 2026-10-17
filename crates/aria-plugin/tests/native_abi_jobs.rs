//! Native C ABI jobs. An `aria-sdk` plugin starts a job with `Host::start_job`;
//! the host runs `Plugin::job_step` on the job thread, with progress, detail,
//! and commit crossing `AriaJobStep`, and calls `Plugin::job_end` with the
//! cancelled flag. Cancel ends a job before its commit step and is refused
//! after it, unmounting cancels running jobs, a panic in a step fails the
//! plugin, and a plugin table without the job entries cannot start a job.
//!
//! Not covered: a plugin built as a separate `cdylib`; the in-process table
//! has the same layout `aria_sdk::export!` builds.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin::{JobManager, JobSnapshot, JobState, PluginContext};
use aria_plugin_abi::{ARIA_API_MAJOR, ARIA_PLUGIN_V1_OVERLAYS_SIZE};
use aria_sdk::{Host, JobStep, Plugin as SdkPlugin, PluginRuntime};
use parking_lot::Mutex;
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

/// The commands the guest registers; each starts a job of that kind.
const KINDS: [&str; 4] = ["count", "endless", "commit", "explode"];

/// What one guest observed, in statics so each test owns its own.
struct Log {
    /// The kind of each job the guest started, by job id.
    started: Mutex<Vec<(u64, &'static str)>>,
    /// Steps taken by each job.
    steps: Mutex<Vec<(u64, u32)>>,
    /// Every `job_end` call: (job, cancelled).
    ended: Mutex<Vec<(u64, bool)>>,
    /// Lets a committed job finish.
    release: AtomicBool,
}

impl Log {
    const fn new() -> Self {
        Self {
            started: Mutex::new(Vec::new()),
            steps: Mutex::new(Vec::new()),
            ended: Mutex::new(Vec::new()),
            release: AtomicBool::new(false),
        }
    }

    fn kind(&self, job: u64) -> &'static str {
        self.started
            .lock()
            .iter()
            .find(|(id, _)| *id == job)
            .map(|(_, kind)| *kind)
            .expect("job_step runs only for a job the guest started")
    }

    fn step(&self, job: u64) -> u32 {
        let mut steps = self.steps.lock();
        if let Some((_, taken)) = steps.iter_mut().find(|(id, _)| *id == job) {
            *taken += 1;
            return *taken;
        }
        steps.push((job, 1));
        1
    }
}

struct Guest {
    log: &'static Log,
}

impl SdkPlugin for Guest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        for kind in KINDS {
            host.register_command(kind, kind)?;
        }
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args: &str) -> Result<(), String> {
        let kind = KINDS
            .into_iter()
            .find(|kind| *kind == id)
            .ok_or_else(|| format!("no command {id}"))?;
        // Held across `start_job`: the first step can run before it returns.
        let mut started = self.log.started.lock();
        let job = host.start_job(&format!("{kind} job"))?;
        started.push((job, kind));
        Ok(())
    }

    fn job_step(&self, _host: &Host, job: u64) -> Result<JobStep, String> {
        let kind = self.log.kind(job);
        let taken = self.log.step(job);
        match kind {
            "count" => Ok(JobStep {
                progress: Some(taken as f32 / 4.0),
                detail: if taken == 4 {
                    "counted to 4".to_owned()
                } else {
                    format!("step {taken}")
                },
                done: taken == 4,
                commit: false,
            }),
            "endless" => {
                std::thread::sleep(Duration::from_millis(2));
                Ok(JobStep {
                    progress: None,
                    detail: format!("tick {taken}"),
                    ..JobStep::default()
                })
            }
            "commit" if taken == 1 => Ok(JobStep {
                progress: Some(0.5),
                detail: "writing".to_owned(),
                done: false,
                commit: true,
            }),
            "commit" => {
                while !self.log.release.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(JobStep {
                    detail: "written".to_owned(),
                    done: true,
                    ..JobStep::default()
                })
            }
            _ => panic!("step exploded"),
        }
    }

    fn job_end(&self, _host: &Host, job: u64, cancelled: bool) {
        self.log.ended.lock().push((job, cancelled));
    }
}

fn write_test_package(root: &Path, id: &str) -> Package {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native Jobs"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI job test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    Package::open(&dir).expect("open package")
}

/// A context with a job registry, and the plugin `table` mounted on it as package `id`.
fn mounted(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Arc<JobManager>, aria_plugin::Registration) {
    let ctx = PluginContext::new();
    let manager = Arc::new(JobManager::new());
    let held = ctx.register_arc(Arc::clone(&manager));
    let pkg = write_test_package(temp.path(), id);
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    ctx.mount(plugin.as_ref()).expect("mount");
    (ctx, manager, held)
}

fn bound(guest: Guest) -> PluginRuntime<Guest> {
    let runtime = PluginRuntime::new(guest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

/// The snapshot of a job labelled `label` once `done` holds for it.
fn wait_for(manager: &JobManager, label: &str, done: impl Fn(&JobSnapshot) -> bool) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = manager
            .snapshot()
            .into_iter()
            .find(|snap| snap.label == label && done(snap));
        if let Some(snap) = found {
            return snap;
        }
        assert!(
            Instant::now() < deadline,
            "job {label:?} never reached the state: {:?}",
            manager.snapshot()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_ended(log: &Log, want: (u64, bool)) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !log.ended.lock().contains(&want) {
        assert!(
            Instant::now() < deadline,
            "job_end{want:?} never ran: {:?}",
            log.ended.lock()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn native_jobs_step_to_done_cancel_refuse_cancel_after_commit_and_end_on_unmount() {
    static LOG: Log = Log::new();
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| bound(Guest { log: &LOG }));
    let temp = TempDir::new().expect("temp dir");
    let (ctx, manager, _held) = mounted(&temp, "test.nativejobs", &plugin_table!(RT));

    ctx.run_command("count").expect("count");
    let done = wait_for(&manager, "Native Jobs: count job", |snap| {
        snap.state.is_terminal()
    });
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.detail, "counted to 4");
    assert_eq!(done.progress, Some(1.0));
    let (count, _) = LOG.started.lock()[0];
    assert_eq!(LOG.steps.lock()[0], (count, 4), "four steps, then done");
    wait_ended(&LOG, (count, false));

    ctx.run_command("endless").expect("endless");
    let running = wait_for(&manager, "Native Jobs: endless job", |snap| {
        snap.detail.starts_with("tick")
    });
    assert_eq!(running.state, JobState::Running);
    assert_eq!(running.progress, None, "unknown progress stays unknown");
    assert!(
        manager.cancel(running.id),
        "Cancel before commit is accepted"
    );
    let cancelled = wait_for(&manager, "Native Jobs: endless job", |snap| {
        snap.state.is_terminal()
    });
    assert_eq!(cancelled.state, JobState::Cancelled);
    let (endless, _) = LOG.started.lock()[1];
    wait_ended(&LOG, (endless, true));

    ctx.run_command("commit").expect("commit");
    let committed = wait_for(&manager, "Native Jobs: commit job", |snap| {
        !snap.cancellable
    });
    assert_eq!(committed.detail, "writing");
    assert!(
        !manager.cancel(committed.id),
        "Cancel after commit is refused"
    );
    LOG.release.store(true, Ordering::Release);
    let written = wait_for(&manager, "Native Jobs: commit job", |snap| {
        snap.state.is_terminal()
    });
    assert_eq!(written.state, JobState::Done);
    assert_eq!(written.detail, "written");

    ctx.run_command("endless").expect("second endless");
    let second = wait_for(&manager, "Native Jobs: endless job", |snap| {
        snap.id != cancelled.id && snap.detail.starts_with("tick")
    });
    ctx.unmount("test.nativejobs").expect("unmount");
    let stopped = wait_for(&manager, "Native Jobs: endless job", |snap| {
        snap.id == second.id && snap.state.is_terminal()
    });
    assert_eq!(
        stopped.state,
        JobState::Cancelled,
        "unmount cancels the job"
    );
    let (again, _) = LOG.started.lock()[3];
    wait_ended(&LOG, (again, true));
    assert!(ctx.failures().is_empty(), "{:?}", ctx.failures());
}

#[test]
fn a_panicking_native_job_step_fails_the_plugin_and_the_job() {
    static LOG: Log = Log::new();
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| bound(Guest { log: &LOG }));
    let temp = TempDir::new().expect("temp dir");
    let (ctx, manager, _held) = mounted(&temp, "test.nativejobs.panic", &plugin_table!(RT));

    ctx.run_command("explode").expect("explode starts");
    let failed = wait_for(&manager, "Native Jobs: explode job", |snap| {
        snap.state.is_terminal()
    });
    assert_eq!(failed.state, JobState::Failed);
    assert!(failed.detail.contains("step exploded"), "{}", failed.detail);
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].id, "test.nativejobs.panic");
    assert!(
        failures[0].reason.contains("step exploded"),
        "{}",
        failures[0].reason
    );
    assert!(
        LOG.ended.lock().is_empty(),
        "a faulted plugin is not called again"
    );
    assert!(
        ctx.run_command("count").is_err(),
        "the failed plugin's commands are gone"
    );
}

#[test]
fn a_plugin_table_without_job_entries_cannot_start_a_job() {
    static LOG: Log = Log::new();
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| bound(Guest { log: &LOG }));
    let mut table = plugin_table!(RT);
    table.struct_size = ARIA_PLUGIN_V1_OVERLAYS_SIZE;
    assert!(table.has_overlays() && !table.has_jobs());
    let temp = TempDir::new().expect("temp dir");
    let (ctx, manager, _held) = mounted(&temp, "test.nativejobs.short", &table);

    let err = ctx
        .run_command("count")
        .expect_err("start_job is refused")
        .to_string();
    assert!(err.contains("job_step and job_end"), "{err}");
    assert!(manager.snapshot().is_empty(), "no job was listed");
    assert!(LOG.started.lock().is_empty());
}
