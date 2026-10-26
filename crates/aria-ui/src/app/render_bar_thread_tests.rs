//! The `aria-render-bar` thread is started once per session and ends with it.
//!
//! The class closed here is a thread whose lifetime does not match the
//! session's: a new thread started for every request, which put a thread
//! spawn on the frame of every edit, and a thread left running after the
//! session that started it is gone, holding the engine and a pass in progress.
//! The cases count the threads started across a run of requests, and wait,
//! bounded by [`BOUND`], for the thread to stop and release its state once the
//! session drops, idle and mid-pass.
//!
//! Not caught: a thread started anywhere other than
//! `RenderBarState::ensure_submitted`, which the count does not see, or how
//! long a request waits for the idle thread to wake.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::RenderBarShared;
use crate::app::AppState;
use crate::app::premiere_tests::{with_clip, with_sequence};

/// How long the thread may take to go idle, to stop, or to release its state.
const BOUND: Duration = Duration::from_secs(10);

/// Waits until the thread is computing nothing and no request waits for it.
fn wait_idle(shared: &RenderBarShared) {
    let deadline = Instant::now() + BOUND;
    let mut state = shared.state.lock();
    while state.busy || state.pending.is_some() {
        assert!(
            !shared.cv.wait_until(&mut state, deadline).timed_out(),
            "the render-bar thread was still busy after {BOUND:?}"
        );
    }
}

/// Waits until the thread is recorded as stopped.
fn wait_stopped(shared: &RenderBarShared) {
    let deadline = Instant::now() + BOUND;
    let mut state = shared.state.lock();
    while state.running {
        assert!(
            !shared.cv.wait_until(&mut state, deadline).timed_out(),
            "the render-bar thread was still running {BOUND:?} after its session dropped"
        );
    }
}

/// Drops the last handle the test holds and waits until no thread holds the
/// worker's state either.
fn wait_released(shared: Arc<RenderBarShared>) {
    let weak = Arc::downgrade(&shared);
    drop(shared);
    let deadline = Instant::now() + BOUND;
    while weak.strong_count() > 0 {
        assert!(
            Instant::now() < deadline,
            "the render-bar thread still held its state {BOUND:?} after it stopped"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Asks for the render bar of `state` as the ruler does and waits, bounded by
/// [`BOUND`], for the thread to serve the request. Returns its sequence
/// number.
fn served(state: &AppState) -> u64 {
    let _ = state.render_bar();
    let shared = &state.render_bar_state.shared;
    let seq = shared.latest_seq();
    assert!(
        shared.wait_within(seq, BOUND).is_some(),
        "request {seq} was not served within {BOUND:?}"
    );
    seq
}

/// Three requests in a row, each made once the last one finished, are served
/// by the thread the first one started, which stays up between them.
#[test]
fn successive_requests_share_one_thread() {
    let (mut state, _) = with_clip();
    let mut last = state.render_bar_state.shared.latest_seq();
    for (mark_in, mark_out) in [(None, None), (Some(0), Some(2)), (Some(1), Some(3))] {
        state.mark_in = mark_in;
        state.mark_out = mark_out;
        let seq = served(&state);
        assert_eq!(seq, last + 1, "each work area is a new request");
        last = seq;
        let shared = &state.render_bar_state.shared;
        wait_idle(shared);
        assert!(
            shared.state.lock().running,
            "the thread stopped once request {seq} was served"
        );
    }
    assert_eq!(
        state.render_bar_state.shared.state.lock().starts,
        1,
        "a request started a thread of its own"
    );
}

/// Dropping the session ends its idle thread, which releases the state it
/// shares with the session.
#[test]
fn an_idle_thread_ends_when_its_session_drops() {
    let (state, _) = with_clip();
    served(&state);
    let shared = Arc::clone(&state.render_bar_state.shared);
    wait_idle(&shared);
    drop(state);
    wait_stopped(&shared);
    wait_released(shared);
}

/// Dropping the session mid-pass abandons the pass at its next frame and ends
/// the thread: the request records no segments.
#[test]
fn a_thread_mid_pass_ends_when_its_session_drops() {
    let state = with_sequence(2, 200);
    let _ = state.render_bar();
    let shared = Arc::clone(&state.render_bar_state.shared);
    let seq = shared.latest_seq();
    let deadline = Instant::now() + BOUND;
    while shared.is_pending() {
        assert!(
            Instant::now() < deadline,
            "no render-bar thread took the request within {BOUND:?}"
        );
        std::thread::yield_now();
    }
    drop(state);
    wait_stopped(&shared);
    assert!(
        shared.state.lock().completed_seq < seq,
        "the pass ran to its end after its session dropped"
    );
    wait_released(shared);
}
