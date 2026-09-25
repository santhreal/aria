//! `panic_becomes` cases.

use super::*;

#[test]
fn panic_becomes_failed() {
    let (manager, rx) = manager_with_events();

    // Literal panic → &str payload.
    let id = manager.spawn("Rendering", |_| panic!("codec exploded"));
    let failed = wait_history(&rx, id).pop().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.detail, "panicked: codec exploded");
    assert!(failed.finished.is_some());

    // Formatted panic → String payload.
    let id = manager.spawn("Rendering", |_| panic!("frame {} bad", 7));
    let failed = wait_history(&rx, id).pop().unwrap();
    assert_eq!(failed.detail, "panicked: frame 7 bad");
}

#[test]
fn progress_clamps_and_is_visible_mid_flight() {
    let (manager, rx) = manager_with_events();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let id = manager.spawn("Clamping", move |ctx| {
        ctx.set_progress(1.5, "over");
        ready_tx.send(()).unwrap();
        resume_rx.recv().unwrap();
        ctx.set_progress(-0.5, "under");
        ctx.set_progress(f32::NAN, "nan");
        // Err so the terminal state keeps the last progress unsnapped.
        Err("checked".into())
    });

    ready_rx.recv().unwrap();
    let mid = manager.get(id).unwrap();
    assert_eq!(mid.state, JobState::Running);
    assert_eq!(mid.progress, Some(1.0));
    assert_eq!(mid.detail, "over");
    resume_tx.send(()).unwrap();

    let events = wait_history(&rx, id);
    let progress: Vec<Option<f32>> = events.iter().map(|e| e.progress).collect();
    assert_eq!(
        progress,
        [None, None, Some(1.0), Some(0.0), Some(0.0), Some(0.0)]
    );
}

#[test]
fn snapshot_orders_newest_first() {
    let (manager, rx) = manager_with_events();
    let (gate_a_tx, gate_a_rx) = mpsc::channel::<()>();
    let a = manager.spawn("a", move |_| {
        let _ = gate_a_rx.recv();
        Ok("a done".into())
    });
    let (gate_b_tx, gate_b_rx) = mpsc::channel::<()>();
    let b = manager.spawn("b", move |_| {
        let _ = gate_b_rx.recv();
        Ok("b done".into())
    });
    let c = manager.spawn("c", |_| Ok("c done".into()));
    wait_history(&rx, c);

    // Newest spawned first, regardless of running/finished state.
    let ids: Vec<JobId> = manager.snapshot().iter().map(|s| s.id).collect();
    assert_eq!(ids, [c, b, a]);

    // Release sequentially so each terminal event is awaited in turn.
    drop(gate_a_tx);
    wait_history(&rx, a);
    drop(gate_b_tx);
    wait_history(&rx, b);
}

#[test]
fn prune_keeps_newest_finished_never_running() {
    let (manager, rx) = manager_with_events();

    // Oldest job of all, still running while everything else finishes.
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let runner = manager.spawn("long export", move |_| {
        let _ = gate_rx.recv();
        Ok("finally".into())
    });

    // Finish KEEP_FINISHED + 6 quick jobs strictly one after another, so
    // finish order matches spawn order and the prune is deterministic.
    let mut quick = Vec::new();
    for n in 0..KEEP_FINISHED + 6 {
        let id = manager.spawn(format!("quick {n}"), |_| Ok("done".into()));
        wait_history(&rx, id);
        quick.push(id);
    }

    // The six oldest-finished are gone; the running job survives even
    // though it is the oldest job in the registry.
    assert_eq!(manager.snapshot().len(), KEEP_FINISHED + 1);
    for &id in &quick[..6] {
        assert!(manager.get(id).is_none(), "expected {id} pruned");
    }
    for &id in &quick[6..] {
        assert!(manager.get(id).is_some());
    }
    assert_eq!(manager.get(runner).unwrap().state, JobState::Running);

    // The runner finishes last → newest-finished → survives the prune
    // its own completion triggers; the next-oldest quick job goes. Under
    // spawn-order pruning the runner would have been dropped instead.
    drop(gate_tx);
    wait_history(&rx, runner);
    assert_eq!(manager.get(runner).unwrap().state, JobState::Done);
    assert!(manager.get(quick[6]).is_none());
    assert_eq!(manager.snapshot().len(), KEEP_FINISHED);
}

#[test]
fn multiple_subscribers_all_notified() {
    let manager = JobManager::new();
    let (tx1, rx1) = mpsc::channel();
    let (tx2, rx2) = mpsc::channel();
    manager.subscribe(move |s| {
        let _ = tx1.send(s.state);
    });
    manager.subscribe(move |s| {
        let _ = tx2.send(s.state);
    });

    manager.spawn("observed", |_| Ok("ok".into()));
    for rx in [rx1, rx2] {
        loop {
            let state = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("subscriber missed events");
            if state == JobState::Done {
                break;
            }
        }
    }
}

#[test]
fn queued_subscriber_panic_does_not_block_spawn_or_lifecycle() {
    let manager = JobManager::new();
    manager.subscribe(|snapshot| {
        if snapshot.state == JobState::Queued {
            panic!("queued subscriber failed");
        }
    });
    let (healthy_tx, healthy_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = healthy_tx.send(snapshot);
    });

    // Queued delivery is synchronous, so reaching this assignment proves
    // the first subscriber's panic did not unwind `spawn`.
    let id = manager.spawn("observed", |ctx| {
        ctx.set_progress(0.5, "halfway");
        Ok("complete".into())
    });
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
fn panics_on_worker_notifications_do_not_strand_job() {
    let manager = JobManager::new();
    let running_panics = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let terminal_panics = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let running_panics_in_callback = Arc::clone(&running_panics);
    let terminal_panics_in_callback = Arc::clone(&terminal_panics);
    manager.subscribe(move |snapshot| {
        if snapshot.state == JobState::Running {
            running_panics_in_callback.fetch_add(1, Ordering::Relaxed);
            panic!("running/progress subscriber failed");
        }
        if snapshot.state.is_terminal() {
            terminal_panics_in_callback.fetch_add(1, Ordering::Relaxed);
            panic!("terminal subscriber failed");
        }
    });
    let (healthy_tx, healthy_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = healthy_tx.send(snapshot);
    });

    let id = manager.spawn("observed", |ctx| {
        ctx.set_progress(0.5, "halfway");
        Ok("complete".into())
    });
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
    assert_eq!(
        history
            .iter()
            .filter(|event| event.state.is_terminal())
            .count(),
        1
    );
    assert!(healthy_rx.try_recv().is_err());
    assert_eq!(running_panics.load(Ordering::Relaxed), 2);
    assert_eq!(terminal_panics.load(Ordering::Relaxed), 1);
    assert_eq!(manager.get(id).unwrap().state, JobState::Done);
}

#[test]
fn subscriber_can_query_cancel_and_spawn_reentrantly() {
    let manager = JobManager::new();
    let (event_tx, event_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        let _ = event_tx.send(snapshot);
    });

    let reentrant_manager = manager.clone();
    let (reentrant_tx, reentrant_rx) = mpsc::channel();
    manager.subscribe(move |snapshot| {
        if snapshot.label == "outer" && snapshot.state == JobState::Queued {
            let get_saw_queued = reentrant_manager
                .get(snapshot.id)
                .is_some_and(|stored| stored.state == JobState::Queued);
            let snapshot_saw_job = reentrant_manager
                .snapshot()
                .iter()
                .any(|stored| stored.id == snapshot.id);
            let cancelled = reentrant_manager.cancel(snapshot.id);
            let inner = reentrant_manager.spawn("inner", |_| Ok("inner done".into()));
            let _ = reentrant_tx.send((get_saw_queued, snapshot_saw_job, cancelled, inner));
        }
    });

    let outer = manager.spawn("outer", |ctx| {
        assert!(ctx.cancelled());
        Ok("outer stopped".into())
    });
    let (get_saw_queued, snapshot_saw_job, cancelled, inner) = reentrant_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("reentrant subscriber did not finish");
    assert!(get_saw_queued);
    assert!(snapshot_saw_job);
    assert!(cancelled);

    let mut terminals = Vec::new();
    while terminals.len() < 2 {
        let snapshot = event_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("timed out waiting for reentrant jobs");
        if snapshot.state.is_terminal() {
            terminals.push((snapshot.id, snapshot.state));
        }
    }
    assert!(terminals.contains(&(inner, JobState::Done)));
    assert!(terminals.contains(&(outer, JobState::Cancelled)));
}

#[test]
fn retained_text_is_safe_bounded_and_deterministic() {
    assert_eq!(
        sanitize_text("a\nb\rc\td\0e\u{2028}f\u{2029}g", MAX_JOB_DETAIL_BYTES),
        "a b c d e f g"
    );

    let label = oversized_text("label:", MAX_JOB_LABEL_BYTES);
    let progress = oversized_text("progress:", MAX_JOB_DETAIL_BYTES);
    let summary = oversized_text("summary:", MAX_JOB_DETAIL_BYTES);
    let expected_label = sanitize_label(&label);
    let expected_progress = sanitize_detail(&progress);
    let expected_summary = sanitize_detail(&summary);
    for (value, cap) in [
        (&expected_label, MAX_JOB_LABEL_BYTES),
        (&expected_progress, MAX_JOB_DETAIL_BYTES),
        (&expected_summary, MAX_JOB_DETAIL_BYTES),
    ] {
        assert_one_line_bounded(value, cap);
        assert!(value.ends_with(ELLIPSIS));
    }
    assert_eq!(sanitize_label(&label), expected_label);
    assert_eq!(sanitize_label(&expected_label), expected_label);
    assert_eq!(sanitize_detail(&progress), expected_progress);
    assert_eq!(sanitize_detail(&expected_progress), expected_progress);
    assert_eq!(sanitize_detail(&summary), expected_summary);
    assert_eq!(sanitize_detail(&expected_summary), expected_summary);

    let (manager, rx) = manager_with_events();
    let (name_tx, name_rx) = mpsc::channel();
    let id = manager.spawn(label, move |ctx| {
        name_tx
            .send(thread::current().name().unwrap_or("").to_owned())
            .unwrap();
        ctx.set_progress(0.5, progress);
        Ok(summary)
    });
    let actual_thread_name = name_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not report its thread name");
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
    assert_eq!(actual_thread_name, thread_name(&expected_label));
    assert!(actual_thread_name.len() <= MAX_THREAD_NAME_BYTES);
    for snapshot in &history {
        assert_eq!(snapshot.label, expected_label);
        assert_one_line_bounded(&snapshot.label, MAX_JOB_LABEL_BYTES);
        assert_one_line_bounded(&snapshot.detail, MAX_JOB_DETAIL_BYTES);
    }
    assert_eq!(history[2].detail, expected_progress);
    assert_eq!(history[3].detail, expected_summary);

    let error = oversized_text("error:", MAX_JOB_DETAIL_BYTES);
    let expected_error = sanitize_detail(&error);
    let error_id = manager.spawn("error job", move |_| Err(error));
    let error_history = wait_history(&rx, error_id);
    let failed = error_history.last().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.detail, expected_error);
    assert_one_line_bounded(&failed.detail, MAX_JOB_DETAIL_BYTES);
    assert_eq!(sanitize_detail(&failed.detail), failed.detail);

    let panic_payload = oversized_text("panic:", MAX_JOB_DETAIL_BYTES);
    let expected_panic = sanitize_detail(&format!("panicked: {panic_payload}"));
    let panic_id = manager.spawn("panic job", move |_| {
        std::panic::panic_any(panic_payload);
    });
    let panic_history = wait_history(&rx, panic_id);
    let panicked = panic_history.last().unwrap();
    assert_eq!(panicked.state, JobState::Failed);
    assert_eq!(panicked.detail, expected_panic);
    assert_one_line_bounded(&panicked.detail, MAX_JOB_DETAIL_BYTES);
    assert_eq!(sanitize_detail(&panicked.detail), panicked.detail);
}

#[test]
fn thread_names_are_prefixed_and_truncated() {
    assert_eq!(thread_name("Export"), "job: Export");

    let long = thread_name("Transcribing a very long interview recording.mov");
    assert!(long.len() <= 32);
    assert!(long.starts_with("job: Transcribing"));

    // Multi-byte labels truncate on a char boundary, not mid-codepoint.
    let multibyte = thread_name("动画渲染动画渲染动画渲染动画渲染");
    assert!(multibyte.len() <= 32);

    // And the wiring: the job really runs under that name.
    let (manager, rx) = manager_with_events();
    let (name_tx, name_rx) = mpsc::channel();
    let id = manager.spawn("Exporting draft.mp4", move |_| {
        let name = thread::current().name().unwrap_or("").to_string();
        name_tx.send(name).unwrap();
        Ok(String::new())
    });
    assert_eq!(name_rx.recv().unwrap(), "job: Exporting draft.mp4");
    wait_history(&rx, id);
}

#[test]
fn ids_are_unique_and_display_plainly() {
    assert_eq!(JobId::from_raw(7).raw(), 7);
    assert_eq!(JobId::from_raw(7).to_string(), "7");

    // Sequential spawn + drain: `wait_history` discards other jobs'
    // events, so concurrent jobs must not share one drain pass.
    let (manager, rx) = manager_with_events();
    let a = manager.spawn("a", |_| Ok(String::new()));
    wait_history(&rx, a);
    let b = manager.spawn("b", |_| Ok(String::new()));
    wait_history(&rx, b);
    assert_ne!(a, b);
}
