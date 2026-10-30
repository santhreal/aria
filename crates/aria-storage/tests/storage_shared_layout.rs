//! `storage` tests covering shared layout. A `#[path]` child of the `storage`
//! test binary; the tests reach their fixtures through `super::*`.

use super::*;

#[test]
fn shared_layout_snapshot_and_path_are_version_coherent() {
    let temporary = TestDirectory::new();
    let first_layout = StorageLayout::new(temporary.path.join("first")).unwrap();
    let second_layout = StorageLayout::new(temporary.path.join("second")).unwrap();
    let shared = SharedStorageLayout::new(first_layout.clone());

    let first_snapshot = shared.snapshot();
    assert_eq!(first_snapshot.generation(), 0);
    assert_eq!(first_snapshot.layout(), &first_layout);
    assert_eq!(
        first_snapshot.resolve(CacheId::Proxies),
        first_layout.resolve(CacheId::Proxies)
    );

    let (first_path, first_path_generation) = shared.resolve_versioned(CacheId::Proxies);
    assert_eq!(first_path_generation, 0);
    assert_eq!(first_path, first_layout.resolve(CacheId::Proxies));

    assert_eq!(shared.replace(0, second_layout.clone()), Ok(1));

    assert_eq!(first_snapshot.generation(), 0);
    assert_eq!(first_snapshot.layout(), &first_layout);
    assert_eq!(
        first_snapshot.resolve(CacheId::Proxies),
        first_layout.resolve(CacheId::Proxies)
    );

    let second_snapshot = shared.snapshot();
    assert_eq!(second_snapshot.generation(), 1);
    assert_eq!(second_snapshot.layout(), &second_layout);
    let (layout, generation) = second_snapshot.into_parts();
    assert_eq!(layout, second_layout);
    assert_eq!(generation, 1);

    let (second_path, second_path_generation) = shared.resolve_versioned(CacheId::Proxies);
    assert_eq!(second_path_generation, 1);
    assert_eq!(second_path, second_layout.resolve(CacheId::Proxies));

    let updated_download = temporary.path.join("updated-download");
    assert_eq!(
        shared.update(1, |layout| {
            layout
                .set_override(CacheId::Download, &updated_download)
                .unwrap();
        }),
        Ok(2)
    );
    let updated_snapshot = shared.snapshot();
    assert_eq!(updated_snapshot.generation(), 2);
    assert_eq!(
        updated_snapshot.resolve(CacheId::Download),
        Some(updated_download)
    );
}

#[test]
fn shared_layout_memory_ids_resolve_without_paths() {
    let temporary = TestDirectory::new();
    let shared = SharedStorageLayout::new(StorageLayout::new(&temporary.path).unwrap());

    let memory_ids: Vec<_> = CACHE_REGISTRY
        .iter()
        .filter(|descriptor| descriptor.kind == CacheKind::Memory)
        .map(|descriptor| descriptor.id)
        .collect();
    assert!(!memory_ids.is_empty());

    for id in memory_ids {
        assert_eq!(shared.resolve(id), None);
        assert_eq!(shared.resolve_versioned(id), (None, 0));
        assert_eq!(shared.snapshot().resolve(id), None);
    }
}

#[test]
fn shared_layout_refuses_stale_writers_without_exposing_paths() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let winner = StorageLayout::new(temporary.path.join("winner")).unwrap();
    let loser_marker = "never-log-this-storage-path";
    let loser = StorageLayout::new(temporary.path.join(loser_marker)).unwrap();
    let shared = SharedStorageLayout::new(initial);

    assert_eq!(shared.replace(0, winner.clone()), Ok(1));

    let error = shared.replace(0, loser).unwrap_err();
    assert_eq!(
        error,
        SharedStorageLayoutError::StaleGeneration {
            expected: 0,
            current: 1,
        }
    );
    assert!(!error.to_string().contains(loser_marker));
    assert!(!format!("{error:?}").contains(loser_marker));

    let update_was_called = std::cell::Cell::new(false);
    assert_eq!(
        shared.update(0, |_| update_was_called.set(true)),
        Err(SharedStorageLayoutError::StaleGeneration {
            expected: 0,
            current: 1,
        })
    );
    assert!(!update_was_called.get());

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.generation(), 1);
    assert_eq!(snapshot.layout(), &winner);
}

#[test]
fn shared_layout_concurrent_readers_observe_only_coherent_versions() {
    const READER_COUNT: usize = 6;
    const LAST_GENERATION: u64 = 64;
    const READS_PER_READER: usize = 512;

    let temporary = TestDirectory::new();
    let layouts: Arc<Vec<_>> = Arc::new(
        (0..=LAST_GENERATION)
            .map(|generation| {
                StorageLayout::new(temporary.path.join(format!("layout-{generation}"))).unwrap()
            })
            .collect(),
    );
    let shared = SharedStorageLayout::new(layouts[0].clone());
    let start = Arc::new(Barrier::new(READER_COUNT + 1));

    let readers: Vec<_> = (0..READER_COUNT)
        .map(|_| {
            let layouts = Arc::clone(&layouts);
            let shared = shared.clone();
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                for read_index in 0..READS_PER_READER {
                    let snapshot = shared.snapshot();
                    let generation = snapshot.generation() as usize;
                    assert_eq!(snapshot.layout(), &layouts[generation]);
                    assert_eq!(
                        snapshot.resolve(CacheId::Proxies),
                        layouts[generation].resolve(CacheId::Proxies)
                    );

                    let (path, path_generation) = shared.resolve_versioned(CacheId::Download);
                    assert_eq!(
                        path,
                        layouts[path_generation as usize].resolve(CacheId::Download)
                    );

                    if read_index % 8 == 0 {
                        thread::yield_now();
                    }
                }
            })
        })
        .collect();

    start.wait();
    for generation in 1..=LAST_GENERATION {
        assert_eq!(
            shared.replace(generation - 1, layouts[generation as usize].clone()),
            Ok(generation)
        );
        thread::yield_now();
    }

    for reader in readers {
        reader.join().unwrap();
    }

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.generation(), LAST_GENERATION);
    assert_eq!(snapshot.layout(), &layouts[LAST_GENERATION as usize]);
}

#[test]
fn shared_layout_recovers_after_a_panicking_update() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::new(initial.clone());
    let panicking_writer = shared.clone();

    let panic = catch_unwind(AssertUnwindSafe(|| {
        panicking_writer
            .update(0, |candidate| {
                candidate
                    .set_override(
                        CacheId::Download,
                        temporary.path.join("uncommitted-override"),
                    )
                    .unwrap();
                panic!("intentional update panic");
            })
            .unwrap();
    }));
    assert!(panic.is_err());

    assert_eq!(shared.replace(0, replacement.clone()), Ok(1));
    let snapshot = shared.snapshot();
    assert_eq!(snapshot.generation(), 1);
    assert_eq!(snapshot.layout(), &replacement);
    assert_ne!(snapshot.layout(), &initial);
}

#[test]
fn shared_layout_lease_blocks_transition_until_operation_finishes() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::new(initial.clone());
    let lease = shared.lease();
    assert_eq!(lease.generation(), 0);
    assert_eq!(
        lease.resolve(CacheId::Proxies),
        initial.resolve(CacheId::Proxies)
    );

    let transition_started = Arc::new(AtomicBool::new(false));
    let callback_started = Arc::new(AtomicBool::new(false));
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let worker = {
        let shared = shared.clone();
        let transition_started = Arc::clone(&transition_started);
        let callback_started = Arc::clone(&callback_started);
        thread::spawn(move || {
            transition_started.store(true, Ordering::Release);
            attempt_tx.send(()).unwrap();
            shared.transition(0, replacement, |old, new| {
                callback_started.store(true, Ordering::Release);
                assert_ne!(old, new);
                Ok::<(), io::Error>(())
            })
        })
    };

    attempt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(transition_started.load(Ordering::Acquire));
    thread::sleep(Duration::from_millis(20));
    assert!(
        !callback_started.load(Ordering::Acquire),
        "exclusive transition must wait for the active lease"
    );

    drop(lease);
    assert_eq!(worker.join().unwrap().unwrap(), 1);
    assert!(callback_started.load(Ordering::Acquire));
    assert_eq!(shared.snapshot().generation(), 1);
}

#[test]
fn shared_layout_failed_transition_does_not_publish_replacement() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::new(initial.clone());

    let result = shared.transition(0, replacement.clone(), |old, new| {
        assert_eq!(old, &initial);
        assert_eq!(new, &replacement);
        Err(io::Error::other("injected transition failure"))
    });
    assert!(matches!(
        result,
        Err(SharedStorageLayoutTransitionError::Transition(error))
            if error.kind() == io::ErrorKind::Other
    ));

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.generation(), 0);
    assert_eq!(snapshot.layout(), &initial);

    let callback_called = AtomicBool::new(false);
    let stale = shared.transition(7, replacement, |_, _| {
        callback_called.store(true, Ordering::Release);
        Ok::<(), io::Error>(())
    });
    assert!(matches!(
        stale,
        Err(SharedStorageLayoutTransitionError::Layout(
            SharedStorageLayoutError::StaleGeneration {
                expected: 7,
                current: 0,
            }
        ))
    ));
    assert!(!callback_called.load(Ordering::Acquire));
}

#[test]
fn committed_cleanup_failure_exposes_the_published_relocation() {
    let report = aria_storage::RelocationReport {
        bytes: 4096,
        files: 3,
        used_copy_fallback: true,
    };
    let error = StorageError::CommittedCleanupFailed {
        message: "old cache cleanup failed".into(),
        report,
    };

    assert_eq!(error.committed_relocation(), Some(report));
    assert_eq!(StorageError::Cancelled.committed_relocation(), None);
}
#[test]
fn rapid_relocation_churn_does_not_starve_waiting_readers() {
    const READER_COUNT: usize = 4;
    const WRITER_COUNT: usize = 2;
    const LEASE_CYCLES_PER_READER: usize = 100;
    const TRANSITIONS_PER_WRITER: usize = 25;

    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("root-0")).unwrap();
    let shared = SharedStorageLayout::new(initial);

    let barrier = Arc::new(Barrier::new(READER_COUNT + WRITER_COUNT + 1));
    let mut reader_handles = Vec::new();
    let mut writer_handles = Vec::new();

    for r in 0..READER_COUNT {
        let shared = shared.clone();
        let barrier = Arc::clone(&barrier);
        reader_handles.push(thread::spawn(move || {
            barrier.wait();
            let mut last_generation = 0;
            for _ in 0..LEASE_CYCLES_PER_READER {
                let lease = shared.lease();
                let generation = lease.generation();
                assert!(
                    generation >= last_generation,
                    "reader {r} observed non-monotonic generation {generation} < {last_generation}"
                );
                last_generation = generation;
                let resolved = lease.resolve(CacheId::Proxies);
                assert_eq!(
                    resolved,
                    lease.layout().resolve(CacheId::Proxies),
                    "resolved path must match leased layout"
                );
            }
        }));
    }

    for w in 0..WRITER_COUNT {
        let shared = shared.clone();
        let barrier = Arc::clone(&barrier);
        let temp_path = temporary.path.clone();
        writer_handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..TRANSITIONS_PER_WRITER {
                let snapshot = shared.snapshot();
                let replacement =
                    StorageLayout::new(temp_path.join(format!("writer-{w}-step-{i}"))).unwrap();
                if i % 2 == 0 {
                    let _ = shared.replace(snapshot.generation(), replacement);
                } else {
                    let _ = shared.transition(snapshot.generation(), replacement, |old, new| {
                        assert_ne!(old.root(), new.root());
                        Ok::<(), io::Error>(())
                    });
                }
            }
        }));
    }

    barrier.wait();
    for handle in reader_handles {
        handle.join().unwrap();
    }
    for handle in writer_handles {
        handle.join().unwrap();
    }

    let final_snapshot = shared.snapshot();
    assert!(final_snapshot.generation() > 0);
}

#[test]
fn failed_or_panicking_transition_releases_ownership_and_notifies_waiting_readers() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::new(initial.clone());

    let (tx, rx) = mpsc::channel();
    let reader_shared = shared.clone();
    let reader_initial = initial.clone();
    let reader_handle = thread::spawn(move || {
        tx.send(()).unwrap();
        let lease = reader_shared.lease();
        assert_eq!(lease.generation(), 0);
        assert_eq!(lease.layout(), &reader_initial);
    });

    rx.recv_timeout(Duration::from_secs(1)).unwrap();

    // Writer fails with error
    let err_result = shared.transition(0, replacement.clone(), |_, _| {
        Err(io::Error::other("forced transition error"))
    });
    assert!(err_result.is_err());

    reader_handle.join().unwrap();

    // Writer panics in transition callback
    let panic_shared = shared.clone();
    let replacement_panic = replacement.clone();
    let panic_result = catch_unwind(AssertUnwindSafe(move || {
        let _ = panic_shared.transition(0, replacement_panic, |_, _| -> io::Result<()> {
            panic!("forced transition panic");
        });
    }));
    assert!(panic_result.is_err());

    // Waiting readers and next operations recover cleanly
    let lease = shared.lease();
    assert_eq!(lease.generation(), 0);
    assert_eq!(lease.layout(), &initial);
    drop(lease);

    assert_eq!(shared.replace(0, replacement), Ok(1));
    assert_eq!(shared.snapshot().generation(), 1);
}

#[test]
fn lock_poisoning_recovers_cleanly_and_reports_actionable_errors() {
    let temporary = TestDirectory::new();
    let initial = StorageLayout::new(temporary.path.join("initial")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::new(initial);

    let panicking_shared = shared.clone();
    let panic_result = catch_unwind(AssertUnwindSafe(move || {
        let _ = panicking_shared.update(0, |_| {
            panic!("poisoning update panic");
        });
    }));
    assert!(panic_result.is_err());

    // Actionable error on stale generation
    let stale_error = shared.replace(99, replacement.clone()).unwrap_err();
    assert_eq!(
        stale_error,
        SharedStorageLayoutError::StaleGeneration {
            expected: 99,
            current: 0,
        }
    );
    assert_eq!(
        stale_error.to_string(),
        "stale storage layout generation: expected 99, current 0"
    );

    // Valid replacement succeeds
    assert_eq!(shared.replace(0, replacement.clone()), Ok(1));
    let lease = shared.lease();
    assert_eq!(lease.generation(), 1);
    assert_eq!(lease.layout(), &replacement);
}
