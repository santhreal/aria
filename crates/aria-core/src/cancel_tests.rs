//! What a cancellation token promises the work that checks it.
//!
//! The class these close: a stop signal that arrives at the canceller but not
//! at the worker — a clone that carries its own flag, a token that can be
//! un-cancelled between two checks, or a "never" token that costs the fast path
//! an atomic.

use super::*;

#[test]
fn a_fresh_token_is_not_cancelled() {
    let cancel = Cancel::new();
    assert!(!cancel.is_cancelled());
    assert!(cancel.is_cancellable());
}

#[test]
fn a_clone_shares_the_signal() {
    let held = Cancel::new();
    let passed = held.clone();
    held.cancel();
    assert!(
        passed.is_cancelled(),
        "the worker's clone did not see the cancellation"
    );
}

#[test]
fn cancelling_twice_is_the_same_as_once() {
    let cancel = Cancel::new();
    cancel.cancel();
    cancel.cancel();
    assert!(cancel.is_cancelled());
}

#[test]
fn cancellation_does_not_come_back() {
    let cancel = Cancel::new();
    cancel.cancel();
    // Every check after the first sees it too: work that checks per layer must
    // not be able to miss a signal that arrived during an earlier layer.
    for _ in 0..100 {
        assert!(cancel.is_cancelled());
    }
}

#[test]
fn a_never_token_cannot_be_cancelled() {
    let cancel = Cancel::never();
    cancel.cancel();
    assert!(
        !cancel.is_cancelled(),
        "a token nobody holds reported a cancellation"
    );
    assert!(
        !cancel.is_cancellable(),
        "a never token claims someone could cancel it"
    );
}

#[test]
fn the_default_is_never() {
    let cancel = Cancel::default();
    assert!(!cancel.is_cancellable());
    // Which is what makes it free: no allocation behind it.
    assert_eq!(
        std::mem::size_of::<Cancel>(),
        std::mem::size_of::<Option<Arc<AtomicBool>>>()
    );
}

#[test]
fn a_cancellation_from_another_thread_is_seen() {
    let held = Cancel::new();
    let worker = held.clone();
    let done = std::thread::spawn(move || {
        // Spin the way a layer loop checks: no sleep, no lock.
        while !worker.is_cancelled() {
            std::hint::spin_loop();
        }
        true
    });
    held.cancel();
    assert!(
        done.join().expect("the worker thread"),
        "the worker never observed the cancellation"
    );
}
