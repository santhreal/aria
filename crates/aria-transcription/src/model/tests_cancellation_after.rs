//! `cancellation_after` cases.

use super::*;

#[test]
fn cancellation_after_begin_commit_cannot_reclassify_success() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    let downloader = CountingDownloader::new(b"hello");
    let cancelled = AtomicBool::new(false);
    let cancellation_checks = AtomicUsize::new(0);
    let checks_at_commit = AtomicUsize::new(0);
    let begin_calls = AtomicUsize::new(0);

    let installed = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &|| {
                cancellation_checks.fetch_add(1, Ordering::SeqCst);
                cancelled.load(Ordering::SeqCst)
            },
            &|| {
                begin_calls.fetch_add(1, Ordering::SeqCst);
                checks_at_commit
                    .store(cancellation_checks.load(Ordering::SeqCst), Ordering::SeqCst);
                cancelled.store(true, Ordering::SeqCst);
                true
            },
        )
        .expect("accepted commit must finish despite later cancellation");

    assert_eq!(installed, path);
    assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        cancellation_checks.load(Ordering::SeqCst),
        checks_at_commit.load(Ordering::SeqCst)
    );
    assert_eq!(fs::read(&path).expect("read installed model"), b"hello");
    assert_no_temporary_files(temp.path());
}

#[test]
fn ready_reuse_skips_begin_commit_and_remains_cancellation_aware() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = ModelManager::new(temp.path()).expect("valid manager");
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    fs::write(&path, b"hello").expect("write verified model");
    let downloader = |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        panic!("verified model must not be downloaded")
    };
    let begin_calls = AtomicUsize::new(0);

    let reused = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &never_cancelled,
            &|| {
                begin_calls.fetch_add(1, Ordering::SeqCst);
                true
            },
        )
        .expect("ready model reuse succeeds");
    assert_eq!(reused, path);
    assert_eq!(begin_calls.load(Ordering::SeqCst), 0);

    let cancellation_checks = AtomicUsize::new(0);
    let error = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &|| cancellation_checks.fetch_add(1, Ordering::SeqCst) + 1 >= 5,
            &|| {
                begin_calls.fetch_add(1, Ordering::SeqCst);
                true
            },
        )
        .expect_err("cancellation after ready verification must be observed");
    assert!(matches!(error, ModelManagerError::Cancelled));
    assert!(cancellation_checks.load(Ordering::SeqCst) >= 5);
    assert_eq!(begin_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fs::read(&path).expect("read reused model"), b"hello");
    assert_no_temporary_files(temp.path());
}

#[cfg(unix)]
#[test]
fn post_commit_mapping_error_is_not_reclassified_as_cancellation() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let pinned_root = temp.path().join("models-pinned");
    let outside = temp.path().join("outside");
    fs::create_dir(&root).expect("create model root");
    fs::create_dir(&outside).expect("create outside directory");
    let manager = ModelManager::new(&root).expect("valid manager");
    let downloader = CountingDownloader::new(b"hello");
    let cancelled = AtomicBool::new(false);
    let begin_calls = AtomicUsize::new(0);

    let error = manager
        .ensure_spec_with_cancellation_and_commit(
            &TINY_SPEC,
            &downloader,
            &|| cancelled.load(Ordering::SeqCst),
            &|| {
                begin_calls.fetch_add(1, Ordering::SeqCst);
                cancelled.store(true, Ordering::SeqCst);
                fs::rename(&root, &pinned_root).expect("rename pinned root");
                symlink(&outside, &root).expect("replace root with symlink");
                true
            },
        )
        .expect_err("post-commit mapping change must remain an actual error");

    assert!(matches!(
        error,
        ModelManagerError::RootMappingChanged { .. }
    ));
    assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read(pinned_root.join(TINY_SPEC.filename())).expect("read committed model"),
        b"hello"
    );
    assert!(!outside.join(TINY_SPEC.filename()).exists());
    assert_no_temporary_files(&pinned_root);
}

#[test]
fn concurrent_ensure_commits_one_complete_download() {
    const WORKERS: usize = 8;

    let temp = TempDir::new().expect("temporary directory");
    let manager = Arc::new(ModelManager::new(temp.path()).expect("valid manager"));
    let downloader = Arc::new(CountingDownloader::new(b"hello"));
    let begin_calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(WORKERS));
    let mut workers = Vec::new();

    for _ in 0..WORKERS {
        let manager = Arc::clone(&manager);
        let downloader = Arc::clone(&downloader);
        let begin_calls = Arc::clone(&begin_calls);
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            manager.ensure_spec_with_cancellation_and_commit(
                &TINY_SPEC,
                downloader.as_ref(),
                &never_cancelled,
                &|| {
                    begin_calls.fetch_add(1, Ordering::SeqCst);
                    true
                },
            )
        }));
    }

    for worker in workers {
        worker
            .join()
            .expect("worker did not panic")
            .expect("concurrent ensure succeeds");
    }

    assert_eq!(downloader.calls(), 1);
    assert_eq!(begin_calls.load(Ordering::SeqCst), 1);
    let path = manager.path_for_spec(&TINY_SPEC).expect("known path");
    assert_eq!(fs::read(path).expect("read committed model"), b"hello");
    assert_no_temporary_files(temp.path());
}

#[test]
fn cancellation_while_waiting_for_model_lock_returns_promptly() {
    let temp = TempDir::new().expect("temporary directory");
    let manager = Arc::new(ModelManager::new(temp.path()).expect("valid manager"));
    let entered_download = Arc::new(Barrier::new(2));
    let release_download = Arc::new(Barrier::new(2));
    let first_calls = Arc::new(AtomicUsize::new(0));

    let first_downloader = Arc::new({
        let entered_download = Arc::clone(&entered_download);
        let release_download = Arc::clone(&release_download);
        let first_calls = Arc::clone(&first_calls);
        move |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
            first_calls.fetch_add(1, Ordering::SeqCst);
            entered_download.wait();
            release_download.wait();
            Ok(Box::new(Cursor::new(b"hello")))
        }
    });
    let first_worker = thread::spawn({
        let manager = Arc::clone(&manager);
        let first_downloader = Arc::clone(&first_downloader);
        move || manager.ensure_spec(&TINY_SPEC, first_downloader.as_ref())
    });
    entered_download.wait();

    let second_downloader = Arc::new(CountingDownloader::new(b"hello"));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (check_sender, check_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let second_worker = thread::spawn({
        let manager = Arc::clone(&manager);
        let second_downloader = Arc::clone(&second_downloader);
        let cancelled = Arc::clone(&cancelled);
        move || {
            let result = manager.ensure_spec_with_cancellation(
                &TINY_SPEC,
                second_downloader.as_ref(),
                &|| {
                    let _ = check_sender.send(());
                    cancelled.load(Ordering::SeqCst)
                },
            );
            result_sender.send(result).expect("send second result");
        }
    });

    let mut observed_lock_retry = true;
    for _ in 0..3 {
        if check_receiver.recv_timeout(Duration::from_secs(1)).is_err() {
            observed_lock_retry = false;
            break;
        }
    }
    let cancellation_started = Instant::now();
    cancelled.store(true, Ordering::SeqCst);
    let second_result = result_receiver.recv_timeout(Duration::from_secs(1));
    let cancellation_elapsed = cancellation_started.elapsed();

    release_download.wait();
    let first_result = first_worker.join().expect("first worker did not panic");
    second_worker.join().expect("second worker did not panic");

    assert!(
        observed_lock_retry,
        "second installation did not reach a contended lock retry"
    );
    assert!(
        cancellation_elapsed < Duration::from_secs(1),
        "lock-wait cancellation took {cancellation_elapsed:?}"
    );
    assert!(matches!(
        second_result.expect("lock waiter did not cancel promptly"),
        Err(ModelManagerError::Cancelled)
    ));
    first_result.expect("first installation succeeds");
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(second_downloader.calls(), 0);
    assert_no_temporary_files(temp.path());
}

#[cfg(unix)]
#[test]
fn canonical_aliases_share_one_model_lock() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let physical_root = temp.path().join("physical");
    let alias_parent = temp.path().join("alias");
    fs::create_dir(&physical_root).expect("create physical root");
    symlink(temp.path(), &alias_parent).expect("create intermediate alias");

    let direct = ModelManager::new(&physical_root).expect("direct manager has a valid final root");
    let aliased = ModelManager::new(alias_parent.join("physical"))
        .expect("aliased manager has a valid final root");
    let direct_snapshot = direct
        .capture_existing_root()
        .expect("capture direct root")
        .expect("direct root exists");
    let aliased_snapshot = aliased
        .capture_existing_root()
        .expect("capture aliased root")
        .expect("aliased root exists");
    let direct_target = direct_snapshot.physical_model_path(&TINY_SPEC);
    let aliased_target = aliased_snapshot.physical_model_path(&TINY_SPEC);

    assert_eq!(direct_target, aliased_target);
    let direct_lock = model_lock(&direct_target);
    let aliased_lock = model_lock(&aliased_target);
    assert!(Arc::ptr_eq(&direct_lock, &aliased_lock));
}

#[cfg(unix)]
#[test]
fn root_swap_during_download_fails_closed_and_cleans_pinned_temp() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let pinned_root = temp.path().join("models-pinned");
    let outside = temp.path().join("outside");
    fs::create_dir(&root).expect("create model root");
    fs::create_dir(&outside).expect("create outside directory");
    let manager = ModelManager::new(&root).expect("valid manager");

    let root_for_swap = root.clone();
    let pinned_for_swap = pinned_root.clone();
    let outside_for_swap = outside.clone();
    let downloader = move |_spec: &ModelSpec| -> Result<DownloadReader, DownloadError> {
        fs::rename(&root_for_swap, &pinned_for_swap).expect("rename pinned root");
        symlink(&outside_for_swap, &root_for_swap).expect("replace root with symlink");
        Ok(Box::new(Cursor::new(b"hello")))
    };

    let error = manager
        .ensure_spec(&TINY_SPEC, &downloader)
        .expect_err("changed root mapping must fail");

    assert!(matches!(
        error,
        ModelManagerError::RootMappingChanged { .. }
    ));
    assert!(root.is_symlink());
    assert!(!outside.join(TINY_SPEC.filename()).exists());
    assert!(!pinned_root.join(TINY_SPEC.filename()).exists());
    assert_directory_empty(&pinned_root);
}

#[cfg(unix)]
#[test]
fn remove_refuses_a_changed_root_mapping() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let pinned_root = temp.path().join("models-pinned");
    let outside = temp.path().join("outside");
    fs::create_dir(&root).expect("create model root");
    fs::create_dir(&outside).expect("create outside directory");
    fs::write(root.join(TINY_SPEC.filename()), b"hello").expect("write model");
    let manager = ModelManager::new(&root).expect("valid manager");
    let snapshot = manager
        .capture_existing_root()
        .expect("capture root")
        .expect("root exists");

    fs::rename(&root, &pinned_root).expect("rename pinned root");
    symlink(&outside, &root).expect("replace root with symlink");
    let error = manager
        .remove_from_snapshot(&TINY_SPEC, &snapshot)
        .expect_err("changed root mapping must fail");

    assert!(matches!(
        error,
        ModelManagerError::RootMappingChanged { .. }
    ));
    assert_eq!(
        fs::read(pinned_root.join(TINY_SPEC.filename())).expect("pinned model survives"),
        b"hello"
    );
    assert!(!outside.join(TINY_SPEC.filename()).exists());
}

#[cfg(unix)]
#[test]
fn verification_never_reports_ready_after_root_mapping_change() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("models");
    let pinned_root = temp.path().join("models-pinned");
    let outside = temp.path().join("outside");
    fs::create_dir(&root).expect("create model root");
    fs::create_dir(&outside).expect("create outside directory");
    fs::write(root.join(TINY_SPEC.filename()), b"hello").expect("write model");
    let manager = ModelManager::new(&root).expect("valid manager");
    let snapshot = manager
        .capture_existing_root()
        .expect("capture root")
        .expect("root exists");

    fs::rename(&root, &pinned_root).expect("rename pinned root");
    symlink(&outside, &root).expect("replace root with symlink");
    let error = manager
        .inspect_path(&TINY_SPEC, &snapshot)
        .expect_err("changed mapping cannot produce ready status");

    assert!(matches!(
        error,
        ModelManagerError::RootMappingChanged { .. }
    ));
    assert!(!outside.join(TINY_SPEC.filename()).exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_roots_for_every_operation() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().expect("temporary directory");
    let outside = temp.path().join("outside");
    let root = temp.path().join("models");
    fs::create_dir(&outside).expect("create outside directory");
    symlink(&outside, &root).expect("create root symlink");
    let manager = ModelManager::new(&root).expect("lexically valid manager");
    let downloader = CountingDownloader::new(b"hello");

    assert!(matches!(
        manager.status_spec(&TINY_SPEC),
        Err(ModelManagerError::UnsafeRoot { .. })
    ));
    assert!(matches!(
        manager.ensure_spec(&TINY_SPEC, &downloader),
        Err(ModelManagerError::UnsafeRoot { .. })
    ));
    assert!(matches!(
        manager.remove_spec(&TINY_SPEC),
        Err(ModelManagerError::UnsafeRoot { .. })
    ));
    assert_eq!(downloader.calls(), 0);
    assert_directory_empty(&outside);
}
