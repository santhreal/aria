//! `commit_boundary` cases.

use super::*;

#[test]
fn cancellation_wins_before_commit_boundary_and_prevents_commit() {
    let (manager, rx) = manager_with_events();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (boundary_tx, boundary_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let committed = Arc::new(AtomicBool::new(false));
    let committed_in_job = Arc::clone(&committed);
    let id = manager.spawn("cancel before commit", move |ctx| {
        let token = ctx.cancellation_token();
        ready_tx.send(()).unwrap();
        resume_rx.recv().unwrap();

        let may_commit = ctx.try_begin_commit();
        boundary_tx
            .send((may_commit, ctx.cancelled(), token.cancelled()))
            .unwrap();
        if may_commit {
            committed_in_job.store(true, Ordering::Relaxed);
        }
        finish_rx.recv().unwrap();
        Ok("preparation stopped".into())
    });

    ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not reach cancellable preparation");
    let running = manager.get(id).unwrap();
    assert_eq!(running.state, JobState::Running);
    assert!(running.cancellable);
    assert!(!running.cancel_requested);

    assert!(manager.cancel(id));
    let requested = manager.get(id).unwrap();
    assert!(requested.cancellable);
    assert!(requested.cancel_requested);
    resume_tx.send(()).unwrap();

    assert_eq!(
        boundary_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("job did not attempt commit"),
        (false, true, true)
    );
    let still_cancelled = manager.get(id).unwrap();
    assert_eq!(still_cancelled.state, JobState::Running);
    assert!(still_cancelled.cancellable);
    assert!(still_cancelled.cancel_requested);
    assert!(!committed.load(Ordering::Relaxed));
    finish_tx.send(()).unwrap();

    let history = wait_history(&rx, id);
    let states: Vec<JobState> = history.iter().map(|event| event.state).collect();
    assert_eq!(
        states,
        [JobState::Queued, JobState::Running, JobState::Cancelled]
    );
    let terminal = history.last().unwrap();
    assert!(terminal.cancel_requested);
}

#[test]
fn commit_boundary_wins_before_cancel_and_preserves_false_token() {
    let (manager, rx) = manager_with_events();
    let (boundary_tx, boundary_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (observed_tx, observed_rx) = mpsc::channel();
    let committed = Arc::new(AtomicBool::new(false));
    let committed_in_job = Arc::clone(&committed);
    let id = manager.spawn("commit before cancel", move |ctx| {
        let token = ctx.cancellation_token();
        assert!(ctx.try_begin_commit());
        boundary_tx.send(token.clone()).unwrap();
        resume_rx.recv().unwrap();
        observed_tx
            .send((ctx.cancelled(), token.cancelled()))
            .unwrap();
        committed_in_job.store(true, Ordering::Relaxed);
        Ok("published".into())
    });

    let token = boundary_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not cross commit boundary");
    let committing = manager.get(id).unwrap();
    assert_eq!(committing.state, JobState::Running);
    assert!(!committing.cancellable);
    assert!(!committing.cancel_requested);
    assert!(!token.cancelled());

    assert!(!manager.cancel(id));
    assert!(!manager.cancel(id));
    assert!(!token.cancelled());
    let after_rejection = manager.get(id).unwrap();
    assert!(!after_rejection.cancellable);
    assert!(!after_rejection.cancel_requested);

    resume_tx.send(()).unwrap();
    assert_eq!(
        observed_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("job did not report post-boundary cancellation state"),
        (false, false)
    );
    let terminal = wait_history(&rx, id).pop().unwrap();
    assert_eq!(terminal.state, JobState::Done);
    assert!(!terminal.cancel_requested);
    assert!(committed.load(Ordering::Relaxed));
    assert!(!token.cancelled());
}

#[test]
fn cancel_and_commit_boundary_race_has_exactly_one_winner() {
    const ROUNDS: usize = 128;

    let (manager, rx) = manager_with_events();
    for round in 0..ROUNDS {
        let start = Arc::new(std::sync::Barrier::new(3));
        let worker_start = Arc::clone(&start);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (boundary_tx, boundary_rx) = mpsc::channel();
        let id = manager.spawn(format!("commit race {round}"), move |ctx| {
            ready_tx.send(()).unwrap();
            worker_start.wait();
            let boundary_won = ctx.try_begin_commit();
            boundary_tx.send(boundary_won).unwrap();
            Ok(if boundary_won {
                "committed".into()
            } else {
                "cancelled".into()
            })
        });

        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker did not reach race gate");
        let cancel_manager = manager.clone();
        let canceller_start = Arc::clone(&start);
        let canceller = thread::spawn(move || {
            canceller_start.wait();
            cancel_manager.cancel(id)
        });

        start.wait();
        let boundary_won = boundary_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker did not report race result");
        let cancel_won = canceller.join().expect("canceller panicked");
        assert_eq!(
            boundary_won, !cancel_won,
            "round {round}: cancel={cancel_won}, boundary={boundary_won}"
        );

        let terminal = wait_history(&rx, id).pop().unwrap();
        assert_eq!(
            terminal.state,
            if boundary_won {
                JobState::Done
            } else {
                JobState::Cancelled
            },
            "round {round}"
        );
        assert_eq!(terminal.cancel_requested, cancel_won, "round {round}");
    }
}

#[test]
fn failure_and_panic_after_commit_boundary_remain_failed() {
    let (manager, rx) = manager_with_events();

    let (failure_ready_tx, failure_ready_rx) = mpsc::channel();
    let (fail_tx, fail_rx) = mpsc::channel();
    let failure_id = manager.spawn("post-commit failure", move |ctx| {
        assert!(ctx.try_begin_commit());
        failure_ready_tx.send(ctx.cancellation_token()).unwrap();
        fail_rx.recv().unwrap();
        Err("publication failed".into())
    });
    let failure_token = failure_ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("failure job did not cross commit boundary");
    assert!(!manager.cancel(failure_id));
    assert!(!failure_token.cancelled());
    fail_tx.send(()).unwrap();
    let failed = wait_history(&rx, failure_id).pop().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.detail, "publication failed");
    assert!(!failed.cancel_requested);
    assert!(!failure_token.cancelled());

    let (panic_ready_tx, panic_ready_rx) = mpsc::channel();
    let (panic_tx, panic_rx) = mpsc::channel();
    let panic_id = manager.spawn("post-commit panic", move |ctx| {
        assert!(ctx.try_begin_commit());
        panic_ready_tx.send(ctx.cancellation_token()).unwrap();
        panic_rx.recv().unwrap();
        panic!("post-commit panic");
    });
    let panic_token = panic_ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("panic job did not cross commit boundary");
    assert!(!manager.cancel(panic_id));
    assert!(!panic_token.cancelled());
    panic_tx.send(()).unwrap();
    let panicked = wait_history(&rx, panic_id).pop().unwrap();
    assert_eq!(panicked.state, JobState::Failed);
    assert_eq!(panicked.detail, "panicked: post-commit panic");
    assert!(!panicked.cancel_requested);
    assert!(!panic_token.cancelled());
}

#[test]
fn commit_boundary_is_idempotent_and_emits_one_exact_transition() {
    let (manager, rx) = manager_with_events();
    let (calls_tx, calls_rx) = mpsc::channel();
    let id = manager.spawn("idempotent commit", move |ctx| {
        let first = ctx.try_begin_commit();
        let second = ctx.try_begin_commit();
        calls_tx.send((first, second)).unwrap();
        Ok("published".into())
    });

    assert_eq!(
        calls_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("job did not report boundary calls"),
        (true, true)
    );
    let history = wait_history(&rx, id);
    let states: Vec<JobState> = history.iter().map(|event| event.state).collect();
    assert_eq!(
        states,
        [
            JobState::Queued,
            JobState::Running,
            JobState::Running,
            JobState::Done,
        ]
    );
    let cancellable: Vec<bool> = history.iter().map(|event| event.cancellable).collect();
    assert_eq!(cancellable, [true, true, false, false]);
    assert!(history.iter().all(|snapshot| !snapshot.cancel_requested));
    assert_eq!(
        history
            .iter()
            .filter(|snapshot| snapshot.state == JobState::Running && !snapshot.cancellable)
            .count(),
        1
    );
}

#[test]
fn commit_boundary_subscribers_are_reentrant_and_panic_isolated() {
    let manager = JobManager::new();
    manager.subscribe(|snapshot| {
        if snapshot.label == "boundary subscribers"
            && snapshot.state == JobState::Running
            && !snapshot.cancellable
        {
            panic!("boundary subscriber failed");
        }
    });

    let reentrant_manager = manager.clone();
    let (reentrant_tx, reentrant_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        if snapshot.label == "boundary subscribers"
            && snapshot.state == JobState::Running
            && !snapshot.cancellable
        {
            let get_saw_closed = reentrant_manager
                .get(snapshot.id)
                .is_some_and(|stored| !stored.cancellable);
            let list_saw_closed = reentrant_manager
                .snapshot()
                .iter()
                .any(|stored| stored.id == snapshot.id && !stored.cancellable);
            let cancel_was_rejected = !reentrant_manager.cancel(snapshot.id);
            let _ = reentrant_tx.send((get_saw_closed, list_saw_closed, cancel_was_rejected));
        }
    });

    let (healthy_tx, healthy_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = healthy_tx.send(snapshot);
    });

    let id = manager.spawn("boundary subscribers", |ctx| {
        assert!(ctx.try_begin_commit());
        Ok("published".into())
    });
    assert_eq!(
        reentrant_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("boundary subscriber deadlocked or was suppressed"),
        (true, true, true)
    );

    let history = wait_history(&healthy_rx, id);
    let states: Vec<JobState> = history.iter().map(|event| event.state).collect();
    assert_eq!(
        states,
        [
            JobState::Queued,
            JobState::Running,
            JobState::Running,
            JobState::Done,
        ]
    );
    assert_eq!(manager.get(id).unwrap().state, JobState::Done);
}

#[test]
fn success_path_events_in_order() {
    let (manager, rx) = manager_with_events();
    let id = manager.spawn("Exporting draft.mp4", |ctx| {
        ctx.set_progress(0.25, "frame 900/3600");
        ctx.set_progress(0.75, "frame 2700/3600");
        Ok("wrote draft.mp4".into())
    });

    let events = wait_history(&rx, id);
    let states: Vec<JobState> = events.iter().map(|e| e.state).collect();
    assert_eq!(
        states,
        [
            JobState::Queued,
            JobState::Running,
            JobState::Running,
            JobState::Running,
            JobState::Done,
        ]
    );
    let progress: Vec<Option<f32>> = events.iter().map(|e| e.progress).collect();
    assert_eq!(progress, [None, None, Some(0.25), Some(0.75), Some(1.0)]);
    assert_eq!(events[2].detail, "frame 900/3600");
    assert_eq!(events.iter().filter(|e| e.state.is_terminal()).count(), 1);

    let done = events.last().unwrap();
    assert_eq!(done.detail, "wrote draft.mp4");
    assert!(done.finished.is_some());

    let got = manager.get(id).expect("finished job still known");
    assert_eq!(got.state, JobState::Done);
    assert_eq!(got.detail, "wrote draft.mp4");
    assert_eq!(got.progress, Some(1.0));
}

#[test]
fn failure_keeps_reason_and_last_progress() {
    let (manager, rx) = manager_with_events();
    let id = manager.spawn("Analyzing clip.mov", |ctx| {
        ctx.set_progress(0.4, "scanning");
        Err("disk full".into())
    });

    let failed = wait_history(&rx, id).pop().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.detail, "disk full");
    // No snap-to-1.0 on failure — the bar stays where the job died.
    assert_eq!(failed.progress, Some(0.4));
    assert!(failed.finished.is_some());
}

#[test]
fn cancel_flag_decides_over_ok_return() {
    let (manager, rx) = manager_with_events();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let cancelling_handle = manager.clone();
    let id = manager.spawn("Transcribing interview.mov", move |ctx| {
        started_tx.send(()).unwrap();
        // Deterministic rendezvous: the test cancels while we wait here.
        resume_rx.recv().unwrap();
        assert!(ctx.cancelled());
        Ok("stopped at 00:42".into())
    });

    started_rx.recv().unwrap();
    assert_eq!(manager.get(id).unwrap().state, JobState::Running);
    // Cancel through a clone: all clones share one registry.
    assert!(cancelling_handle.cancel(id));
    assert!(manager.get(id).unwrap().cancel_requested);
    resume_tx.send(()).unwrap();

    let last = wait_history(&rx, id).pop().unwrap();
    assert_eq!(last.state, JobState::Cancelled);
    assert_eq!(last.detail, "stopped at 00:42");
    assert!(last.cancel_requested);

    // Finished and unknown jobs both refuse.
    assert!(!manager.cancel(id));
    assert!(!manager.cancel(JobId(9999)));
}

#[test]
fn cancel_flag_decides_over_err_return() {
    let (manager, rx) = manager_with_events();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let id = manager.spawn("Exporting", move |_| {
        started_tx.send(()).unwrap();
        resume_rx.recv().unwrap();
        Err("aborted mid-write".into())
    });

    started_rx.recv().unwrap();
    assert!(manager.cancel(id));
    resume_tx.send(()).unwrap();

    let last = wait_history(&rx, id).pop().unwrap();
    assert_eq!(last.state, JobState::Cancelled);
    assert_eq!(last.detail, "aborted mid-write");
}
