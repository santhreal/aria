//! Jobs that run at most once per process, on a thread of their own, and that
//! a process ending through `exit` stops before its libraries' exit handlers
//! and destructors run.
//!
//! A GStreamer plugin loaded while the C library runs those destructors aborts
//! the process in GObject's type registration (`cannot register existing
//! fundamental type 'gchar'`), and a CUDA context still being created when the
//! CUDA driver's exit handlers run aborts it in `pthread_mutex_lock`. Work that
//! may load a plugin on a thread of its own runs as a [`OnceJob`]: the decode
//! device warm-up, and the encoder's codec probe in `aria-encoder`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How long a process ending through `exit` waits for each started job to
/// finish the step it is in: longer than the slowest step any job takes, one
/// decode device held for its warm-up bound.
const EXIT_WAIT: Duration = Duration::from_secs(crate::gst_context::DEVICE_WAIT_SECS + 5);

/// Where a [`OnceJob`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    Idle,
    Running,
    Finished,
}

/// A job that runs at most once per process, on a thread of its own, and can
/// be waited for or stopped.
///
/// A started job is stopped when the process ends through `exit`, the path a
/// return from `main` and every test binary take, and the step in progress is
/// waited for before any exit handler runs. The job checks
/// [`OnceJob::stopping`] between its steps and returns once it holds.
/// [`crate::exit_process`] ends the process through `_exit`, which runs no exit
/// handler and does not wait.
pub struct OnceJob {
    stage: Mutex<Stage>,
    finished: Condvar,
    stopping: AtomicBool,
}

impl OnceJob {
    /// A job that has not started.
    pub const fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Idle),
            finished: Condvar::new(),
            stopping: AtomicBool::new(false),
        }
    }

    /// Run `job` on a thread named `name` unless a job was started or stopped
    /// before. Returns whether this call started it.
    pub fn start(&'static self, name: &str, job: impl FnOnce() + Send + 'static) -> bool {
        {
            let mut stage = self.stage.lock().unwrap_or_else(PoisonError::into_inner);
            if *stage != Stage::Idle || self.stopping() {
                return false;
            }
            *stage = Stage::Running;
        }
        stop_at_exit(self);
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Finishes even when `job` panics, so no waiter blocks forever.
                let _finish = Finish(self);
                job();
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, thread = name, "the job thread did not start; its work is skipped");
            self.finish();
        }
        true
    }

    fn finish(&self) {
        *self.stage.lock().unwrap_or_else(PoisonError::into_inner) = Stage::Finished;
        self.finished.notify_all();
    }

    /// Block until a started job finishes; return at once when none runs.
    pub fn wait(&self) {
        let stage = self.stage.lock().unwrap_or_else(PoisonError::into_inner);
        let _finished = self
            .finished
            .wait_while(stage, |stage| *stage == Stage::Running)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// Whether [`OnceJob::stop`] was called. The job returns at its next step
    /// once this holds.
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// Make the job return at its next step and block until it finishes or
    /// `bound` passes; a job not started yet never starts. Returns whether no
    /// job runs.
    pub fn stop(&self, bound: Duration) -> bool {
        self.stopping.store(true, Ordering::Release);
        let stage = self.stage.lock().unwrap_or_else(PoisonError::into_inner);
        let (stage, _) = self
            .finished
            .wait_timeout_while(stage, bound, |stage| *stage == Stage::Running)
            .unwrap_or_else(PoisonError::into_inner);
        *stage != Stage::Running
    }

    #[cfg(test)]
    pub(crate) fn stage(&self) -> Stage {
        *self.stage.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for OnceJob {
    fn default() -> Self {
        Self::new()
    }
}

/// Marks its [`OnceJob`] finished when dropped.
struct Finish(&'static OnceJob);

impl Drop for Finish {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// Every [`OnceJob`] started in this process, in start order.
static STARTED: Mutex<Vec<&'static OnceJob>> = Mutex::new(Vec::new());

/// Stop `job` when the process ends through `exit`.
fn stop_at_exit(job: &'static OnceJob) {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    STARTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(job);
    REGISTERED.call_once(|| {
        #[allow(unsafe_code)]
        // SAFETY: registers a handler with the C exit path; `stop_started`
        // takes no arguments, does not unwind, and reads only statics.
        let registered = unsafe { libc::atexit(stop_started) };
        if registered != 0 {
            tracing::warn!(
                "the exit handler did not register; a process exit does not wait for started jobs"
            );
        }
    });
}

/// Stop every started [`OnceJob`], waiting up to [`EXIT_WAIT`] for each. Runs
/// once the process ends through `exit`: first as the main thread's
/// [`MainExit`] drops, and again as an exit handler, for an `exit` called on
/// another thread, which finds no job left once the first has run.
extern "C" fn stop_started() {
    let started = std::mem::take(&mut *STARTED.lock().unwrap_or_else(PoisonError::into_inner));
    for job in started {
        job.stop(EXIT_WAIT);
    }
}

/// Stops every started job as the main thread ends the process.
///
/// Exit handlers run in the reverse of the order they were registered in, so
/// a handler a library registers while a job's step loads it runs before the
/// one [`stop_at_exit`] registered, and tears the library down under the step:
/// the CUDA driver's, registered while the decode device warm-up loads its
/// plugin. `exit` drops the exiting thread's thread locals before it runs any
/// exit handler, and the main thread is the one that calls it when `main`
/// returns, so this thread local, armed on the main thread before `main`
/// starts, waits for the step before any library is torn down.
struct MainExit;

impl Drop for MainExit {
    fn drop(&mut self) {
        stop_started();
    }
}

thread_local! {
    static MAIN_EXIT: MainExit = const { MainExit };
}

/// Arms [`MainExit`]. Runs on the main thread before `main`, from the
/// executable's initializer list.
extern "C" fn arm_main_exit() {
    MAIN_EXIT.with(|_| {});
}

#[allow(unsafe_code)]
#[used]
// SAFETY: `.init_array` holds the functions the loader calls on the main
// thread before `main`; `arm_main_exit` takes no arguments, does not unwind,
// and only registers a thread local's destructor.
#[unsafe(link_section = ".init_array")]
static ARM_MAIN_EXIT: extern "C" fn() = arm_main_exit;

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: a decoder opened while the device warm-up runs must wait for it
    /// rather than open a second device beside it, and must not wait forever
    /// when the warm-up job panics. Covers start-once, the wait, and the panic
    /// path; does not cover the CUDA device itself, which the build host lacks.
    #[test]
    fn a_waiter_blocks_until_the_one_job_finishes_even_when_it_panics() {
        use std::sync::mpsc;

        static JOB: OnceJob = OnceJob::new();
        JOB.wait();
        assert_eq!(JOB.stage(), Stage::Idle, "waiting started nothing");

        let (release, released) = mpsc::channel::<()>();
        let (ran, runs) = mpsc::channel::<()>();
        let started = JOB.start("aria-test-once", move || {
            let _ = ran.send(());
            let _ = released.recv();
            panic!("the job panics after its release");
        });
        assert!(started, "the first start did not run the job");
        assert!(
            !JOB.start("aria-test-once", || unreachable!("a second job ran")),
            "a second start was accepted"
        );
        runs.recv_timeout(Duration::from_secs(5))
            .expect("the job runs");

        let (done, waited) = mpsc::channel::<()>();
        let waiter = std::thread::spawn(move || {
            JOB.wait();
            let _ = done.send(());
        });
        assert!(
            waited.recv_timeout(Duration::from_millis(200)).is_err(),
            "the wait returned while the job ran"
        );
        release.send(()).expect("release the job");
        waited
            .recv_timeout(Duration::from_secs(5))
            .expect("the wait ends once the job panics");
        waiter.join().expect("waiter");
        assert_eq!(JOB.stage(), Stage::Finished);
    }
}

#[cfg(test)]
#[path = "gst_job_exit_tests.rs"]
mod exit_tests;
