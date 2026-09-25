//! `cancellation_token` cases.

use super::*;

#[test]
fn structured_output_boundaries_are_exact_and_values_are_not_rewritten() {
    let max_name = format!("a{}", "z".repeat(MAX_JOB_OUTPUT_NAME_BYTES - 1));
    let max_ascii_value = "v".repeat(MAX_JOB_OUTPUT_VALUE_BYTES);
    let max_unicode_value = "é".repeat(MAX_JOB_OUTPUT_VALUE_BYTES / 2);
    assert_eq!(max_unicode_value.len(), MAX_JOB_OUTPUT_VALUE_BYTES);

    let empty = JobOutput::new("empty_value", "").expect("empty values are intentional");
    assert_eq!(empty.name(), "empty_value");
    assert_eq!(empty.value(), "");

    let ascii =
        JobOutput::new(max_name.clone(), max_ascii_value.clone()).expect("exact name/value bounds");
    assert_eq!(ascii.name(), max_name);
    assert_eq!(ascii.value(), max_ascii_value);

    let unicode =
        JobOutput::new("unicode", max_unicode_value.clone()).expect("exact UTF-8 byte bound");
    assert_eq!(unicode.value(), max_unicode_value);

    assert_eq!(
        JobOutput::new(format!("{max_name}z"), "").unwrap_err(),
        JobCompletionError::NameTooLong
    );
    assert_eq!(
        JobOutput::new("value", format!("{max_ascii_value}v")).unwrap_err(),
        JobCompletionError::ValueTooLong
    );
    assert_eq!(
        JobOutput::new("value", format!("{max_unicode_value}a")).unwrap_err(),
        JobCompletionError::ValueTooLong
    );
}

#[test]
fn structured_output_rejects_invalid_names_and_multiline_or_control_values() {
    assert_eq!(
        JobOutput::new("", "").unwrap_err(),
        JobCompletionError::EmptyName
    );
    for name in [
        "0key",
        "_key",
        "Content_key",
        "content-key",
        "content.key",
        "content key",
        "contentKey",
        "contenț",
    ] {
        assert_eq!(
            JobOutput::new(name, "").unwrap_err(),
            JobCompletionError::InvalidName,
            "accepted invalid name {name:?}"
        );
    }
    for name in ["a", "a0", "content_key", "analyzer_version", "z_9"] {
        JobOutput::new(name, "").expect("valid output name");
    }

    for value in [
        "line\nbreak",
        "line\rbreak",
        "tab\tvalue",
        "nul\0value",
        "delete\u{7f}",
        "next\u{85}line",
        "separator\u{2028}value",
        "paragraph\u{2029}value",
    ] {
        assert_eq!(
            JobOutput::new("value", value).unwrap_err(),
            JobCompletionError::InvalidValue,
            "accepted invalid value {value:?}"
        );
    }
}

#[test]
fn completion_rejects_duplicate_names_and_output_count_overflow() {
    let mut completion = JobCompletion::new("indexed transcript");
    for index in 0..MAX_JOB_OUTPUTS {
        completion = completion
            .with_output(format!("output_{index}"), index.to_string())
            .expect("output at count bound");
    }
    assert_eq!(completion.outputs().len(), MAX_JOB_OUTPUTS);

    assert_eq!(
        completion
            .clone()
            .with_output("output_0", "replacement")
            .unwrap_err(),
        JobCompletionError::DuplicateName
    );
    assert_eq!(
        completion.with_output("overflow", "8").unwrap_err(),
        JobCompletionError::TooManyOutputs
    );
}

#[test]
fn completion_validation_errors_are_bounded_and_never_echo_input() {
    const HOSTILE: &str = "HOSTILE_INPUT_MUST_NOT_APPEAR";
    for error in [
        JobCompletionError::EmptyName,
        JobCompletionError::NameTooLong,
        JobCompletionError::InvalidName,
        JobCompletionError::ValueTooLong,
        JobCompletionError::InvalidValue,
        JobCompletionError::DuplicateName,
        JobCompletionError::TooManyOutputs,
    ] {
        assert!(
            error.to_string().len() <= 80,
            "unbounded validation error: {error}"
        );
    }

    let errors = [
        JobOutput::new(format!("a{}{HOSTILE}", "x".repeat(64)), "").unwrap_err(),
        JobOutput::new(format!("A{HOSTILE}"), "").unwrap_err(),
        JobOutput::new("value", format!("{HOSTILE}\n")).unwrap_err(),
        JobOutput::new("value", format!("{HOSTILE}{}", "x".repeat(512))).unwrap_err(),
        JobCompletion::new("detail")
            .with_output("same", "first")
            .unwrap()
            .with_output("same", HOSTILE)
            .unwrap_err(),
    ];

    for error in errors {
        let message = error.to_string();
        assert!(message.len() <= 80, "unbounded validation error: {message}");
        assert!(!message.contains(HOSTILE), "error reflected hostile input");
    }
}

#[test]
fn ordinary_spawn_completes_with_empty_outputs() {
    let (manager, rx) = manager_with_events();
    let id = manager.spawn("ordinary", |_| Ok("complete".into()));
    let history = wait_history(&rx, id);

    assert_eq!(history.last().unwrap().state, JobState::Done);
    assert!(history.iter().all(|snapshot| snapshot.outputs.is_empty()));
    assert!(manager.get(id).unwrap().outputs.is_empty());
}

#[test]
fn structured_completion_outputs_appear_only_in_terminal_done_snapshot() {
    let (manager, rx) = manager_with_events();
    let id = manager.spawn_with_completion("structured", |context| {
        context.set_progress(0.5, "indexing");
        let completion = JobCompletion::new("indexed\ntranscript");
        assert_eq!(completion.detail(), "indexed\ntranscript");
        Ok(completion
            .with_output("content_key", "transcript:clip-7")?
            .with_output("segment_count", "12")?)
    });
    let history = wait_history(&rx, id);

    assert_eq!(
        history
            .iter()
            .map(|snapshot| snapshot.state)
            .collect::<Vec<_>>(),
        [
            JobState::Queued,
            JobState::Running,
            JobState::Running,
            JobState::Done,
        ]
    );
    assert!(
        history[..history.len() - 1]
            .iter()
            .all(|snapshot| snapshot.outputs.is_empty())
    );
    let done = history.last().unwrap();
    assert_eq!(done.detail, "indexed transcript");
    assert_eq!(
        done.outputs,
        [
            JobOutput::new("content_key", "transcript:clip-7").unwrap(),
            JobOutput::new("segment_count", "12").unwrap(),
        ]
    );
    assert_eq!(manager.get(id).unwrap().outputs, done.outputs);
}

#[test]
fn accepted_cancellation_discards_returned_completion_outputs() {
    let (manager, rx) = manager_with_events();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let id = manager.spawn_with_completion("cancel outputs", move |_| {
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        Ok(JobCompletion::new("prepared").with_output("content_key", "must-not-publish")?)
    });

    ready_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not reach cancellation gate");
    assert!(manager.get(id).unwrap().outputs.is_empty());
    assert!(manager.cancel(id));
    release_tx.send(()).unwrap();

    let history = wait_history(&rx, id);
    let terminal = history.last().unwrap();
    assert_eq!(terminal.state, JobState::Cancelled);
    assert!(terminal.outputs.is_empty());
    assert!(history.iter().all(|snapshot| snapshot.outputs.is_empty()));
    assert!(manager.get(id).unwrap().outputs.is_empty());
}

#[test]
fn commit_boundary_and_rejected_late_cancel_preserve_outputs() {
    let (manager, rx) = manager_with_events();
    let (committing_tx, committing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let id = manager.spawn_with_completion("commit outputs", move |context| {
        assert!(context.try_begin_commit());
        committing_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        Ok(JobCompletion::new("published").with_output("content_key", "committed")?)
    });

    committing_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not cross commit boundary");
    let committing = manager.get(id).unwrap();
    assert_eq!(committing.state, JobState::Running);
    assert!(!committing.cancellable);
    assert!(committing.outputs.is_empty());
    assert!(!manager.cancel(id));
    release_tx.send(()).unwrap();

    let history = wait_history(&rx, id);
    assert!(
        history[..history.len() - 1]
            .iter()
            .all(|snapshot| snapshot.outputs.is_empty())
    );
    let terminal = history.last().unwrap();
    assert_eq!(terminal.state, JobState::Done);
    assert_eq!(
        terminal.outputs,
        [JobOutput::new("content_key", "committed").unwrap()]
    );
}

#[test]
fn structured_failure_and_panic_publish_no_outputs() {
    let (manager, rx) = manager_with_events();
    let failed_id = manager.spawn_with_completion("structured failure", |context| {
        assert!(context.try_begin_commit());
        Err("publication failed".into())
    });
    let failed = wait_history(&rx, failed_id);
    assert_eq!(failed.last().unwrap().state, JobState::Failed);
    assert!(failed.iter().all(|snapshot| snapshot.outputs.is_empty()));

    let panic_id = manager.spawn_with_completion(
        "structured panic",
        |context| -> Result<JobCompletion, String> {
            assert!(context.try_begin_commit());
            panic!("publication panicked");
        },
    );
    let panicked = wait_history(&rx, panic_id);
    assert_eq!(panicked.last().unwrap().state, JobState::Failed);
    assert!(panicked.iter().all(|snapshot| snapshot.outputs.is_empty()));
}

#[test]
fn retained_output_snapshots_are_immutable_clones() {
    let (manager, rx) = manager_with_events();
    let id = manager.spawn_with_completion("clone outputs", |_| {
        Ok(JobCompletion::new("done").with_output("content_key", "stable")?)
    });
    let terminal = wait_history(&rx, id).pop().unwrap();
    assert_eq!(terminal.outputs.len(), 1);

    let mut fetched = manager.get(id).unwrap();
    fetched.outputs.clear();
    assert!(fetched.outputs.is_empty());
    assert_eq!(manager.get(id).unwrap().outputs, terminal.outputs);

    let mut listed = manager.snapshot();
    listed
        .iter_mut()
        .find(|snapshot| snapshot.id == id)
        .unwrap()
        .outputs
        .clear();
    assert_eq!(manager.get(id).unwrap().outputs, terminal.outputs);
}

#[test]
fn cancellation_token_has_callback_safe_traits() {
    fn assert_traits<T: Clone + Send + Sync + 'static>() {}

    assert_traits::<CancellationToken>();
}

#[test]
fn cancellation_token_moves_into_static_callback_and_observes_cancel() {
    type CancelCallback = Arc<dyn Fn() -> bool + Send + Sync + 'static>;

    let (manager, rx) = manager_with_events();
    let (callback_tx, callback_rx) = mpsc::channel::<CancelCallback>();
    let (resume_tx, resume_rx) = mpsc::channel();
    let id = manager.spawn("callback cancellation", move |ctx| {
        let token = ctx.cancellation_token();
        let callback: CancelCallback = Arc::new(move || token.cancelled());
        if callback_tx.send(Arc::clone(&callback)).is_err() {
            return Err("callback receiver dropped".into());
        }
        resume_rx
            .recv()
            .map_err(|_| "resume sender dropped".to_owned())?;
        assert!(callback());
        Ok("callback observed cancellation".into())
    });

    let callback = callback_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not publish its cancellation callback");
    assert!(!callback());
    assert!(manager.cancel(id));
    assert!(callback());
    resume_tx.send(()).unwrap();

    let terminal = wait_history(&rx, id).pop().unwrap();
    assert_eq!(terminal.state, JobState::Cancelled);
}

#[test]
fn cancellation_token_and_context_agree_before_cancellation() {
    let (manager, rx) = manager_with_events();
    let (state_tx, state_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let id = manager.spawn("matching cancellation state", move |ctx| {
        let token = ctx.cancellation_token();
        state_tx
            .send((ctx.cancelled(), token.cancelled()))
            .map_err(|_| "state receiver dropped".to_owned())?;
        resume_rx
            .recv()
            .map_err(|_| "resume sender dropped".to_owned())?;
        Ok("states matched".into())
    });

    let (context_cancelled, token_cancelled) = state_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job did not publish cancellation state");
    assert_eq!(context_cancelled, token_cancelled);
    assert!(!context_cancelled);
    resume_tx.send(()).unwrap();
    assert_eq!(wait_history(&rx, id).last().unwrap().state, JobState::Done);
}

#[test]
fn cancellation_tokens_remain_read_only_after_terminal_completion() {
    let (manager, rx) = manager_with_events();

    let (uncancelled_tx, uncancelled_rx) = mpsc::channel();
    let uncancelled_id = manager.spawn("uncancelled token", move |ctx| {
        uncancelled_tx
            .send(ctx.cancellation_token())
            .map_err(|_| "token receiver dropped".to_owned())?;
        Ok("finished normally".into())
    });
    let uncancelled = uncancelled_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("uncancelled job did not publish its token");
    assert_eq!(
        wait_history(&rx, uncancelled_id).last().unwrap().state,
        JobState::Done
    );
    assert!(!uncancelled.cancelled());

    let (cancelled_tx, cancelled_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let cancelled_id = manager.spawn("cancelled token", move |ctx| {
        cancelled_tx
            .send(ctx.cancellation_token())
            .map_err(|_| "token receiver dropped".to_owned())?;
        resume_rx
            .recv()
            .map_err(|_| "resume sender dropped".to_owned())?;
        Ok("stopped".into())
    });
    let cancelled = cancelled_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("cancelled job did not publish its token");
    assert!(!cancelled.cancelled());
    assert!(manager.cancel(cancelled_id));
    resume_tx.send(()).unwrap();
    assert_eq!(
        wait_history(&rx, cancelled_id).last().unwrap().state,
        JobState::Cancelled
    );
    assert!(cancelled.cancelled());

    // Neither token keeps the context or registry alive, and both retain
    // their final read-only view after those owners are gone.
    drop(manager);
    assert!(!uncancelled.cancelled());
    assert!(cancelled.cancelled());
}
