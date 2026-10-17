//! Tests for this module.
//!
//! The cases live in sibling files, one per area, declared at the bottom. This
//! file holds only the shared fixtures; a child sees them through `use super::*`.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::*;

/// A manager plus a channel carrying every subscriber event, so tests
/// rendezvous on real transitions instead of sleeping.
fn manager_with_events() -> (JobManager, mpsc::Receiver<JobSnapshot>) {
    let manager = JobManager::new();
    let (tx, rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = tx.send(snapshot);
    });
    (manager, rx)
}

/// Drain events for `id` (skipping other jobs') until its terminal
/// event; returns the job's full ordered event history.
fn wait_history(rx: &mpsc::Receiver<JobSnapshot>, id: JobId) -> Vec<JobSnapshot> {
    let mut events = Vec::new();
    loop {
        let snapshot = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("timed out waiting for job events");
        if snapshot.id != id {
            continue;
        }
        let terminal = snapshot.state.is_terminal();
        if terminal {
            assert!(
                !snapshot.cancellable,
                "terminal job {id} still accepts cancellation"
            );
        }
        events.push(snapshot);
        if terminal {
            return events;
        }
    }
}

fn assert_one_line_bounded(value: &str, max_bytes: usize) {
    assert!(
        value.len() <= max_bytes,
        "{} bytes exceeds {max_bytes}",
        value.len()
    );
    assert!(std::str::from_utf8(value.as_bytes()).is_ok());
    assert!(
        !value
            .chars()
            .any(|character| character.is_control()
                || matches!(character, '\u{2028}' | '\u{2029}')),
        "unsafe one-line value: {value:?}"
    );
}

fn oversized_text(prefix: &str, cap: usize) -> String {
    let mut value = format!("{prefix}\n\r\t\0\u{2028}\u{2029}");
    while value.len() <= cap + 8 {
        value.push_str("动画🦀");
    }
    value
}

#[path = "tests_cancellation_token.rs"]
mod cancellation_token;

#[path = "tests_commit_boundary.rs"]
mod commit_boundary;

#[path = "tests_panic_becomes.rs"]
mod panic_becomes;

#[path = "tests_exhaustion.rs"]
mod exhaustion;
