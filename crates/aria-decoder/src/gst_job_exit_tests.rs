//! A started [`OnceJob`] returns at its next step once stopped, and a process
//! that ends through `exit` waits for the step the job is in before any exit
//! handler runs.
//!
//! WHY: the decode device warm-up thread loaded a GStreamer plugin while a
//! test binary that had printed `ok` ran its libraries' destructors, and the
//! load aborted the process in GObject's type registration (`cannot register
//! existing fundamental type 'gchar'`). Then the CUDA driver, loaded inside the
//! warm-up's step, registered exit handlers after the job's own, and they ran
//! first and tore the driver down under `cuCtxCreate` (`pthread_mutex_lock`
//! assertion, SIGABRT). The class is "a detached job thread inside a library
//! call when the process ends". The children below start a job whose one step
//! outlasts its test and let libtest end the process; one step registers an
//! exit handler of its own, as a library loaded there does. The parents assert
//! the step finished before the child ended, before that handler ran, and that
//! the job returned at the stop. The in-process tests bound the stop and hold
//! that a stopped job never starts. They do not check that the warm-up or the
//! codec probe tests [`OnceJob::stopping`] between its steps, do not cover an
//! `exit` called on a thread other than the main thread while a library's exit
//! handler is registered inside a step, and do not cover a process ended
//! through `_exit` ([`crate::exit_process`]), which runs no exit handler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{OnceJob, Stage};

/// How long the child's one step runs; its test returns long before.
const STEP: Duration = Duration::from_millis(500);

/// Printed by the child's job once its step ends.
const STEP_DONE: &str = "the step in progress at exit finished";

/// Printed by the child's job when it finds the stop after its step.
const STOPPED: &str = "the job found the stop and returned";

/// Printed by the child's job when it runs on past its step.
const RAN_ON: &str = "the job ran on past the stop";

/// How long a child or a job may take to start, run, and end.
const BOUND: Duration = Duration::from_secs(30);

#[test]
#[ignore = "child process of an_exit_waits_for_the_step_in_progress_and_stops_the_job"]
fn exit_child() {
    static JOB: OnceJob = OnceJob::new();
    let (entered, entering) = mpsc::channel::<()>();
    let started = JOB.start("aria-test-exit-stop", move || {
        let _ = entered.send(());
        std::thread::sleep(STEP);
        println!("{STEP_DONE}");
        if JOB.stopping() {
            println!("{STOPPED}");
            return;
        }
        println!("{RAN_ON}");
    });
    assert!(started, "the job did not start");
    entering
        .recv_timeout(BOUND)
        .expect("the job enters its step");
}

#[test]
fn an_exit_waits_for_the_step_in_progress_and_stops_the_job() {
    let ran = crate::test_child::run("gst_job::exit_tests::exit_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the child did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert!(
        status.success(),
        "the child ended with {status}\n{}",
        ran.transcript()
    );
    assert!(
        ran.stdout.contains(STEP_DONE),
        "the child ended inside its job's step\n{}",
        ran.transcript()
    );
    assert!(
        ran.stdout.contains(STOPPED) && !ran.stdout.contains(RAN_ON),
        "the job did not return at the stop\n{}",
        ran.transcript()
    );
}

/// Whether the library child's job is inside its step.
static IN_STEP: AtomicBool = AtomicBool::new(false);

/// Printed by the exit handler the library child's step registers when it
/// runs while the step is in progress.
const TORN_DOWN_UNDER: &str = "a library's exit handler ran inside the step";

/// Printed by that handler when it runs once the step has ended.
const TORN_DOWN_AFTER: &str = "a library's exit handler ran after the step";

/// The exit handler of a library the library child's step loads.
extern "C" fn library_teardown() {
    if IN_STEP.load(Ordering::Acquire) {
        println!("{TORN_DOWN_UNDER}");
    } else {
        println!("{TORN_DOWN_AFTER}");
    }
}

#[test]
#[ignore = "child process of a_library_loaded_inside_a_step_is_torn_down_after_the_step"]
fn library_child() {
    static JOB: OnceJob = OnceJob::new();
    let (entered, entering) = mpsc::channel::<i32>();
    let started = JOB.start("aria-test-exit-library", move || {
        IN_STEP.store(true, Ordering::Release);
        #[allow(unsafe_code)]
        // SAFETY: registers a handler with the C exit path, as a library
        // loaded in this step does; `library_teardown` takes no arguments,
        // does not unwind, and reads only a static.
        let registered = unsafe { libc::atexit(library_teardown) };
        let _ = entered.send(registered);
        std::thread::sleep(STEP);
        IN_STEP.store(false, Ordering::Release);
        println!("{STEP_DONE}");
    });
    assert!(started, "the job did not start");
    let registered = entering
        .recv_timeout(BOUND)
        .expect("the job enters its step");
    assert_eq!(registered, 0, "the step's exit handler did not register");
}

#[test]
fn a_library_loaded_inside_a_step_is_torn_down_after_the_step() {
    let ran = crate::test_child::run("gst_job::exit_tests::library_child", BOUND);
    let status = ran.status.unwrap_or_else(|| {
        panic!(
            "the child did not end within {BOUND:?}\n{}",
            ran.transcript()
        )
    });
    assert!(
        status.success(),
        "the child ended with {status}\n{}",
        ran.transcript()
    );
    assert!(
        ran.stdout.contains(STEP_DONE),
        "the child ended inside its job's step\n{}",
        ran.transcript()
    );
    assert!(
        ran.stdout.contains(TORN_DOWN_AFTER) && !ran.stdout.contains(TORN_DOWN_UNDER),
        "a library loaded inside the step was torn down before the step ended\n{}",
        ran.transcript()
    );
}

/// A job that does not return at the stop is waited for up to the bound and
/// no longer; once it returns, a stop finds no job running.
#[test]
fn a_stop_waits_no_longer_than_its_bound() {
    static JOB: OnceJob = OnceJob::new();
    let (release, released) = mpsc::channel::<()>();
    let (entered, entering) = mpsc::channel::<()>();
    assert!(
        JOB.start("aria-test-stop-bound", move || {
            let _ = entered.send(());
            let _ = released.recv();
        }),
        "the job did not start"
    );
    entering.recv_timeout(BOUND).expect("the job runs");

    let bound = Duration::from_millis(300);
    let asked = Instant::now();
    assert!(
        !JOB.stop(bound),
        "the stop reported no job running while one ran"
    );
    let waited = asked.elapsed();
    assert!(
        waited >= bound && waited < BOUND,
        "the stop waited {waited:?} for a bound of {bound:?}"
    );
    assert!(JOB.stopping(), "the stop was not recorded");

    release.send(()).expect("release the job");
    JOB.wait();
    assert_eq!(JOB.stage(), Stage::Finished);
    assert!(
        JOB.stop(Duration::ZERO),
        "a stop found a finished job running"
    );
}

/// A job stopped before it starts never runs, and a waiter finds none.
#[test]
fn a_stopped_job_never_starts() {
    static JOB: OnceJob = OnceJob::new();
    assert!(
        JOB.stop(Duration::ZERO),
        "a stop found a job that never started running"
    );
    assert!(
        !JOB.start("aria-test-stopped", || unreachable!("a stopped job ran")),
        "a start after the stop was accepted"
    );
    assert_eq!(JOB.stage(), Stage::Idle);
    JOB.wait();
}
