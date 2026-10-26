//! A started [`OnceJob`] returns at its next step once stopped, and a process
//! that ends through `exit` waits for the step the job is in.
//!
//! WHY: the decode device warm-up thread loaded a GStreamer plugin while a
//! test binary that had printed `ok` ran its libraries' destructors, and the
//! load aborted the process in GObject's type registration (`cannot register
//! existing fundamental type 'gchar'`). The class is "a detached job thread
//! inside a library call when the process ends". The child below starts a job
//! whose one step outlasts its test and lets libtest end the process; the
//! parent asserts the step finished and the job returned at the stop before
//! the child ended. The in-process tests bound the stop and hold that a
//! stopped job never starts. They do not check that the warm-up or the codec
//! probe tests [`OnceJob::stopping`] between its steps, and do not cover a
//! process ended through `_exit` ([`crate::exit_process`]), which runs no exit
//! handler.

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
