//! `switch_root` cases.

use super::*;

#[test]
fn quota_evicts_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 250);
    let old = write(&cache, "stock/old.bin", 100);
    // Ensure distinct mtimes even on coarse filesystems.
    backdate(&old);
    let new = write(&cache, "stock/new.bin", 200);

    cache.enforce_quota();
    assert!(!old.exists(), "oldest file evicted");
    assert!(new.exists(), "newest file kept");
    assert_eq!(cache.size_bytes(), 200);
}

#[test]
fn quota_updates_apply_to_subsequent_enforcement() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 300);
    let old = write(&cache, "stock/old.bin", 100);
    backdate(&old);
    let new = write(&cache, "stock/new.bin", 200);

    assert_eq!(cache.quota_bytes(), 300);
    cache.enforce_quota();
    assert!(old.exists());
    assert!(new.exists());

    cache.set_quota_bytes(200);
    assert_eq!(cache.quota_bytes(), 200);
    cache.enforce_quota();
    assert!(!old.exists(), "updated quota evicts the oldest file");
    assert!(new.exists(), "updated quota does not over-evict");
    assert_eq!(cache.size_bytes(), 200);
}

#[test]
fn quota_is_safe_to_read_and_update_from_concurrent_threads() {
    const WORKERS: usize = 4;
    const ROUNDS: u64 = 64;

    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(DownloadCache::new(dir.path().to_path_buf(), 0));
    let phase = Arc::new(Barrier::new(WORKERS));
    let handles: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let cache = Arc::clone(&cache);
            let phase = Arc::clone(&phase);
            thread::spawn(move || {
                let mut observed_quotas = Vec::with_capacity(ROUNDS as usize);
                for round in 0..ROUNDS {
                    phase.wait();
                    let first_quota = round * WORKERS as u64;
                    cache.set_quota_bytes(first_quota + worker as u64);
                    phase.wait();
                    observed_quotas.push(cache.quota_bytes());
                    phase.wait();
                }
                observed_quotas
            })
        })
        .collect();

    for handle in handles {
        for (round, observed) in handle.join().unwrap().into_iter().enumerate() {
            let first_quota = round as u64 * WORKERS as u64;
            let quotas_this_round = first_quota..first_quota + WORKERS as u64;
            assert!(
                quotas_this_round.contains(&observed),
                "observed quota {observed} outside {quotas_this_round:?}"
            );
        }
    }
}

#[test]
fn total_size_saturates_instead_of_overflowing() {
    assert_eq!(saturating_total_bytes([u64::MAX - 1, 1, 1]), u64::MAX);
}

#[test]
fn protected_project_media_survives_eviction_and_explicit_clear() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 1);
    let protected = write(&cache, "stock/project.mp4", 100);
    backdate(&protected);
    let disposable = write(&cache, "stock/thumbnail.img", 20);
    cache.protect_path(&protected).unwrap();

    assert_eq!(cache.protected_path_count(), 1);
    cache.enforce_quota();
    assert!(protected.exists());
    assert!(!disposable.exists());

    let another = write(&cache, "ai/unused.png", 12);
    let report = cache.clear().unwrap();
    assert_eq!(report.removed_bytes, 12);
    assert_eq!(report.removed_files, 1);
    assert_eq!(report.retained_protected_files, 1);
    assert!(protected.exists());
    assert!(!another.exists());
}

#[test]
fn missing_project_media_can_be_protected_before_redownload() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 0);
    let future = dir.path().join("stock/future.mp4");
    cache.protect_path(&future).unwrap();
    assert_eq!(cache.protected_path_count(), 1);

    write(&cache, "stock/future.mp4", 8);
    cache.enforce_quota();
    assert!(future.exists());
    let report = cache.clear().unwrap();
    assert_eq!(report.retained_protected_files, 1);
    assert!(future.exists());
}

#[test]
fn protected_paths_must_be_contained_cache_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache_root = dir.path().join("cache");
    let cache = DownloadCache::new(cache_root.clone(), 100);
    std::fs::create_dir_all(&cache_root).unwrap();

    for path in [
        dir.path().join("outside.mp4"),
        cache_root.clone(),
        cache_root.join("../outside.mp4"),
        PathBuf::from("relative.mp4"),
    ] {
        assert!(cache.protect_path(&path).is_err(), "{path:?}");
    }

    let directory = cache_root.join("directory");
    std::fs::create_dir(&directory).unwrap();
    assert!(cache.protect_path(&directory).is_err());
    assert_eq!(cache.protected_path_count(), 0);
}

#[test]
fn switch_root_moves_cache_and_atomically_remaps_protected_paths() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    let new_root = dir.path().join("new-cache");
    let cache = DownloadCache::new(old_root.clone(), 1_000);
    let protected = write(&cache, "stock/project.mp4", 100);
    let disposable = write(&cache, "stock/disposable.bin", 20);
    cache.protect_path(&protected).unwrap();

    cache
        .switch_root(&new_root, |old, new| {
            assert_eq!(old, old_root);
            assert_eq!(new, new_root);
            std::fs::rename(old, new)
        })
        .unwrap();

    let moved_protected = new_root.join("stock/project.mp4");
    let moved_disposable = new_root.join("stock/disposable.bin");
    assert_eq!(cache.root(), new_root);
    assert!(!old_root.exists());
    assert!(moved_protected.exists());
    assert!(moved_disposable.exists());
    assert_eq!(
        &*read_protected(&cache.protected_paths),
        &HashSet::from([moved_protected.clone()])
    );

    cache.set_quota_bytes(0);
    cache.enforce_quota();
    assert!(
        moved_protected.exists(),
        "the remapped protection must survive eviction"
    );
    assert!(!moved_disposable.exists());
    assert!(!disposable.exists());
}

#[test]
fn switch_root_closure_failure_rolls_back_all_memory_state() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    let new_root = dir.path().join("new-cache");
    let cache = DownloadCache::new(old_root.clone(), 1_000);
    let protected = write(&cache, "stock/project.mp4", 8);
    cache.protect_path(&protected).unwrap();
    let protected_before = read_protected(&cache.protected_paths).clone();

    let result = cache.switch_root(&new_root, |old, new| {
        assert_eq!(old, old_root);
        assert_eq!(new, new_root);
        std::fs::create_dir(new)?;
        Err(io::Error::other("injected migration failure"))
    });

    assert!(matches!(
        result,
        Err(DownloadCacheSwitchError::Migration(error))
            if error.kind() == io::ErrorKind::Other
    ));
    assert_eq!(cache.root(), old_root);
    assert_eq!(&*read_protected(&cache.protected_paths), &protected_before);
    assert_eq!(
        cache.path_for("stock/after-failure.bin").unwrap(),
        old_root.join("stock/after-failure.bin")
    );
}

#[test]
fn active_lease_delays_switch_root_and_keeps_its_old_path_stable() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    let new_root = dir.path().join("new-cache");
    let cache = Arc::new(DownloadCache::new(old_root.clone(), 1_000));
    let lease = cache.lease("stock/in-flight.mp4").unwrap();
    let lease_path = lease.path().to_path_buf();
    let started = Arc::new(Barrier::new(2));
    let closure_called = Arc::new(AtomicBool::new(false));

    let worker = {
        let cache = Arc::clone(&cache);
        let started = Arc::clone(&started);
        let closure_called = Arc::clone(&closure_called);
        let new_root = new_root.clone();
        thread::spawn(move || {
            started.wait();
            cache.switch_root(&new_root, |old, new| {
                closure_called.store(true, Ordering::Release);
                std::fs::rename(old, new)
            })
        })
    };

    started.wait();
    thread::sleep(Duration::from_millis(20));
    assert!(!closure_called.load(Ordering::Acquire));
    assert_eq!(cache.root(), old_root);
    assert_eq!(lease.path(), lease_path);

    drop(lease);
    worker.join().unwrap().unwrap();
    assert!(closure_called.load(Ordering::Acquire));
    assert_eq!(cache.root(), new_root);
    assert_eq!(lease_path, old_root.join("stock/in-flight.mp4"));
}

#[test]
fn switch_root_rejects_invalid_destinations_without_calling_closure() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    std::fs::create_dir(&old_root).unwrap();
    let cache = DownloadCache::new(old_root.clone(), 1_000);
    let calls = AtomicUsize::new(0);
    let migrate = |_: &Path, _: &Path| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok::<(), io::Error>(())
    };

    assert!(matches!(
        cache.switch_root("relative-cache", migrate),
        Err(DownloadCacheSwitchError::RelativeDestination)
    ));
    let filesystem_root = old_root.ancestors().last().unwrap().to_path_buf();
    assert!(matches!(
        cache.switch_root(&filesystem_root, migrate),
        Err(DownloadCacheSwitchError::FilesystemRootDestination)
    ));
    assert!(matches!(
        cache.switch_root(&old_root, migrate),
        Err(DownloadCacheSwitchError::SameDestination)
    ));

    let existing_directory = dir.path().join("existing-directory");
    std::fs::create_dir(&existing_directory).unwrap();
    assert!(matches!(
        cache.switch_root(&existing_directory, migrate),
        Err(DownloadCacheSwitchError::ExistingDestination)
    ));
    let existing_file = dir.path().join("existing-file");
    std::fs::write(&existing_file, b"occupied").unwrap();
    assert!(matches!(
        cache.switch_root(&existing_file, migrate),
        Err(DownloadCacheSwitchError::ExistingDestination)
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlink_destination = dir.path().join("destination-link");
        symlink(dir.path().join("missing-target"), &symlink_destination).unwrap();
        assert!(matches!(
            cache.switch_root(&symlink_destination, migrate),
            Err(DownloadCacheSwitchError::SymlinkDestination)
        ));
    }

    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(cache.root(), old_root);
}

#[test]
fn blocked_destructive_operations_prevent_switch_root() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    std::fs::create_dir(&old_root).unwrap();
    let cache = DownloadCache::new(old_root.clone(), 1_000);
    let closure_called = AtomicBool::new(false);
    cache.block_destructive_operations();

    let result = cache.switch_root(dir.path().join("new-cache"), |_, _| {
        closure_called.store(true, Ordering::Release);
        Ok::<(), io::Error>(())
    });

    assert!(matches!(
        result,
        Err(DownloadCacheSwitchError::DestructiveOperationsBlocked)
    ));
    assert!(!closure_called.load(Ordering::Acquire));
    assert_eq!(cache.root(), old_root);
}

#[test]
fn concurrent_root_readers_observe_only_complete_old_or_new_paths() {
    const READERS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache-with-a-distinct-name");
    let new_root = dir.path().join("new-cache-with-another-distinct-name");
    std::fs::create_dir(&old_root).unwrap();
    let cache = Arc::new(DownloadCache::new(old_root.clone(), 1_000));
    let start = Arc::new(Barrier::new(READERS + 1));
    let stop = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicU8::new(0));

    let readers: Vec<_> = (0..READERS)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let start = Arc::clone(&start);
            let stop = Arc::clone(&stop);
            let observed = Arc::clone(&observed);
            let old_root = old_root.clone();
            let new_root = new_root.clone();
            thread::spawn(move || {
                start.wait();
                while !stop.load(Ordering::Acquire) {
                    let root = cache.root();
                    if root == old_root {
                        observed.fetch_or(0b01, Ordering::Release);
                    } else if root == new_root {
                        observed.fetch_or(0b10, Ordering::Release);
                    } else {
                        panic!("observed a partial or unknown root: {root:?}");
                    }
                    thread::yield_now();
                }
            })
        })
        .collect();

    start.wait();
    let deadline = Instant::now() + Duration::from_secs(1);
    while observed.load(Ordering::Acquire) & 0b01 == 0 {
        assert!(Instant::now() < deadline, "readers never observed old root");
        thread::yield_now();
    }

    cache
        .switch_root(&new_root, |old, new| {
            std::fs::rename(old, new)?;
            thread::sleep(Duration::from_millis(10));
            Ok::<(), io::Error>(())
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(1);
    while observed.load(Ordering::Acquire) & 0b10 == 0 {
        assert!(Instant::now() < deadline, "readers never observed new root");
        thread::yield_now();
    }
    stop.store(true, Ordering::Release);
    for reader in readers {
        reader.join().unwrap();
    }
    assert_eq!(observed.load(Ordering::Acquire), 0b11);
}

#[test]
fn root_snapshots_and_switches_recover_from_lock_poisoning() {
    let dir = tempfile::tempdir().unwrap();
    let old_root = dir.path().join("old-cache");
    let new_root = dir.path().join("new-cache");
    std::fs::create_dir(&old_root).unwrap();
    let cache = DownloadCache::new(old_root.clone(), 1_000);

    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _root = cache.root.write().unwrap();
        panic!("poison root lock");
    }));
    assert!(poisoned.is_err());
    assert_eq!(cache.root(), old_root);

    cache
        .switch_root(&new_root, |old, new| std::fs::rename(old, new))
        .unwrap();
    assert_eq!(cache.root(), new_root);
}

#[test]
fn leases_exclude_maintenance_until_they_drop() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 100);
    let lease = cache.lease("stock/in-flight.mp4").unwrap();
    assert_eq!(lease.path(), dir.path().join("stock/in-flight.mp4"));
    assert!(cache.begin_maintenance(Duration::ZERO).is_err());

    drop(lease);
    assert!(cache.begin_maintenance(Duration::ZERO).is_ok());
}
