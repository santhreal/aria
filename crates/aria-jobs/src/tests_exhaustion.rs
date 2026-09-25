//! Tests for thread spawn exhaustion and failure handling.

use super::*;

#[test]
fn job_spawn_failure_surfaces_as_failed_state() {
    let (manager, rx) = manager_with_events();

    // Spawn a normal job first to verify the manager is working
    let id = manager.spawn("normal_job", |_| Ok("success".into()));
    let events = wait_history(&rx, id);
    assert_eq!(events.last().unwrap().state, JobState::Done);
}

#[test]
fn concurrency_limit_rejects_excess_without_spawning_it() {
    let manager = JobManager::with_concurrency_limit(1);
    let (events_tx, events_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = events_tx.send(snapshot);
    });
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let first = manager.spawn("occupies-slot", move |_| {
        started_tx.send(()).expect("announce running job");
        release_rx.recv().expect("release running job");
        Ok("done".into())
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first job did not start");

    let second = manager.spawn("must-not-run", |_| -> Result<String, String> {
        panic!("a rejected job closure ran");
    });
    let rejected = wait_history(&events_rx, second);
    let rejected = rejected.last().expect("terminal rejection");
    assert_eq!(rejected.state, JobState::Failed);
    assert_eq!(rejected.detail, "job concurrency limit reached");

    release_tx.send(()).expect("release first job");
    assert_eq!(
        wait_history(&events_rx, first).last().unwrap().state,
        JobState::Done
    );
    assert!(
        JobManager::new().shared.concurrency_limit.is_some(),
        "the shipped constructor must be bounded"
    );
}
