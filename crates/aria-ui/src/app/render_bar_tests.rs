//! A wait for render-bar segments ends when no pass is left to compute them.
//!
//! The class closed here is a wait with no exit: a caller blocked on a request
//! that the `aria-render-bar` thread will never complete, because the thread
//! panicked mid-pass, was never started, or went idle with the pass abandoned.
//! Each case runs the wait on a thread of its own and fails when it has not
//! returned within [`BOUND`], so a wait that hangs is reported as a failure
//! instead of hanging the run.
//!
//! Not caught: a pass that runs forever without panicking, which keeps the
//! wait open by design.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use super::{RenderBarState, Unwinding};

/// How long a wait that has nothing left to wait for may take to return.
const BOUND: Duration = Duration::from_secs(10);

/// Runs `wait_for(target)` on a thread and returns its result, or fails when
/// it has not returned within [`BOUND`].
fn wait_bounded(bar: &RenderBarState, target: u64) -> Option<usize> {
    let (tx, rx) = mpsc::channel();
    let waiting = Arc::clone(&bar.shared);
    std::thread::spawn(move || {
        let _ = tx.send(waiting.wait_for(target).map(|segments| segments.len()));
    });
    rx.recv_timeout(BOUND)
        .unwrap_or_else(|_| panic!("the wait for request {target} did not end within {BOUND:?}"))
}

/// A request is outstanding: it has a sequence number past the completed one
/// and the serving thread is recorded as running and computing it.
fn outstanding(bar: &RenderBarState) -> u64 {
    let mut state = bar.shared.state.lock();
    state.latest_seq += 1;
    state.running = true;
    state.busy = true;
    state.latest_seq
}

#[test]
fn a_pass_that_panics_ends_the_wait() {
    let bar = RenderBarState::default();
    let target = outstanding(&bar);
    let shared = Arc::clone(&bar.shared);
    // The serving thread panics with the wait already blocked on it.
    let (started, go) = mpsc::channel::<()>();
    let drain = std::thread::Builder::new()
        .name("aria-render-bar".to_owned())
        .spawn(move || {
            let _unwinding = Unwinding(&shared);
            go.recv().expect("the test sends the go");
            panic!("a pass that panics");
        })
        .expect("start the serving thread");
    let waited = {
        let (tx, rx) = mpsc::channel();
        let waiting = Arc::clone(&bar.shared);
        std::thread::spawn(move || {
            let _ = tx.send(waiting.wait_for(target).is_some());
        });
        std::thread::sleep(Duration::from_millis(50));
        started
            .send(())
            .expect("the serving thread waits for the go");
        rx.recv_timeout(BOUND)
            .unwrap_or_else(|_| panic!("the wait outlived a panicked pass by {BOUND:?}"))
    };
    assert!(drain.join().is_err(), "the serving thread panicked");
    assert!(!waited, "a request no pass completed returned segments");
    assert!(
        !bar.shared.state.lock().running,
        "the panicked thread left a serving thread recorded"
    );
}

#[test]
fn a_pass_that_never_started_ends_the_wait() {
    let bar = RenderBarState::default();
    let target = outstanding(&bar);
    // What `spawn_serve` does when the thread cannot be started.
    bar.shared.state.lock().running = false;
    assert_eq!(wait_bounded(&bar, target), None);
}

#[test]
fn a_pass_abandoned_by_a_live_thread_ends_the_wait() {
    let bar = RenderBarState::default();
    let target = outstanding(&bar);
    let waiting = Arc::clone(&bar.shared);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(waiting.wait_for(target).is_some());
    });
    std::thread::sleep(Duration::from_millis(50));
    // What the serving thread does when it abandons the pass and finds no
    // newer request: it stays running and goes idle.
    bar.shared.state.lock().busy = false;
    bar.shared.cv.notify_all();
    let completed = rx
        .recv_timeout(BOUND)
        .unwrap_or_else(|_| panic!("the wait outlived an abandoned pass by {BOUND:?}"));
    assert!(!completed, "a request no pass completed returned segments");
}

#[test]
fn a_completed_request_returns_its_segments() {
    let bar = RenderBarState::default();
    let target = outstanding(&bar);
    {
        let mut state = bar.shared.state.lock();
        state.completed_seq = target;
        state.running = false;
    }
    assert_eq!(wait_bounded(&bar, target), Some(0));
}
