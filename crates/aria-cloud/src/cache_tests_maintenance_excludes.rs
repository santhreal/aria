//! `maintenance_excludes` cases.

use super::*;

#[test]
fn maintenance_excludes_new_leases_until_it_drops() {
    use std::sync::atomic::AtomicBool;

    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 100);
    let maintenance = cache.begin_maintenance(Duration::ZERO).unwrap();
    let acquired = AtomicBool::new(false);

    thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let _lease = cache.lease("stock/after-clear.mp4").unwrap();
            acquired.store(true, Ordering::Release);
        });
        thread::sleep(Duration::from_millis(20));
        assert!(!acquired.load(Ordering::Acquire));
        drop(maintenance);
        worker.join().unwrap();
    });
    assert!(acquired.load(Ordering::Acquire));
}

#[test]
fn a_rejected_lease_does_not_leak_activity() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 100);
    assert!(cache.lease("../outside.mp4").is_err());
    assert!(cache.begin_maintenance(Duration::ZERO).is_ok());
}

#[test]
fn destructive_operations_fail_closed_during_project_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 0);
    let path = write(&cache, "stock/unclassified.mp4", 8);

    cache.block_destructive_operations();
    assert!(cache.destructive_operations_blocked());
    cache.enforce_quota();
    assert!(path.exists());
    assert!(cache.clear().is_err());
    assert!(path.exists());

    cache.allow_destructive_operations();
    assert!(!cache.destructive_operations_blocked());
    cache.enforce_quota();
    assert!(!path.exists());
}

#[test]
fn under_quota_evicts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 1000);
    let a = write(&cache, "a.bin", 100);
    cache.enforce_quota();
    assert!(a.exists());
}

#[test]
fn hit_returns_existing_and_misses_absent() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 1000);
    assert!(cache.hit("nope.bin").is_none());
    write(&cache, "yes.bin", 10);
    assert!(cache.hit("yes.bin").is_some());
}

#[test]
fn cache_keys_cannot_escape_or_replace_directories() {
    let dir = tempfile::tempdir().unwrap();
    let cache_root = dir.path().join("cache");
    let cache = DownloadCache::new(cache_root.clone(), 1_000);

    for key in [
        "",
        ".",
        "..",
        "../escaped.bin",
        "stock/../../escaped.bin",
        "/absolute.bin",
    ] {
        assert!(cache.path_for(key).is_err(), "{key:?} must be rejected");
        assert!(cache.hit(key).is_none(), "{key:?} cannot be a cache hit");
    }
    assert!(!dir.path().join("escaped.bin").exists());

    std::fs::create_dir_all(cache_root.join("occupied")).unwrap();
    assert!(cache.path_for("occupied").is_err());
}

#[test]
fn relative_cache_roots_fail_closed() {
    let cache = DownloadCache::new(PathBuf::from("relative-cache"), 1_000);
    assert!(cache.path_for("asset.bin").is_err());
    assert!(cache.hit("asset.bin").is_none());
    assert_eq!(cache.size_bytes(), 0);
    assert!(cache.clear().is_err());
}

#[cfg(unix)]
#[test]
fn nested_symlinks_are_never_followed_or_cleared_through() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let cache_root = dir.path().join("cache");
    std::fs::create_dir(&cache_root).unwrap();
    let protected = outside.path().join("protected.bin");
    std::fs::write(&protected, b"keep").unwrap();
    symlink(outside.path(), cache_root.join("linked")).unwrap();

    let cache = DownloadCache::new(cache_root.clone(), 0);
    assert!(cache.path_for("linked/new.bin").is_err());
    assert!(cache.hit("linked/protected.bin").is_none());

    cache.enforce_quota();
    assert!(protected.exists(), "eviction must not traverse the symlink");

    if !cache_root.join("linked").exists() {
        symlink(outside.path(), cache_root.join("linked")).unwrap();
    }
    cache.clear().unwrap();
    assert!(protected.exists(), "clearing must not traverse the symlink");
    assert!(!cache_root.join("linked").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_cache_root_fails_closed() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let protected = outside.path().join("protected.bin");
    std::fs::write(&protected, b"keep").unwrap();
    let cache_root = dir.path().join("cache-link");
    symlink(outside.path(), &cache_root).unwrap();
    let cache = DownloadCache::new(cache_root, 0);

    assert!(cache.path_for("new.bin").is_err());
    assert!(cache.hit("protected.bin").is_none());
    assert_eq!(cache.size_bytes(), 0);
    cache.enforce_quota();
    assert!(cache.clear().is_err());
    assert!(protected.exists());
}

#[test]
fn clear_empties_but_keeps_root() {
    let dir = tempfile::tempdir().unwrap();
    let cache = DownloadCache::new(dir.path().to_path_buf(), 1000);
    write(&cache, "stock/a.bin", 10);
    write(&cache, "bundles/b.bin", 10);
    cache.clear().unwrap();
    assert_eq!(cache.size_bytes(), 0);
    assert!(cache.root().exists());
}

#[test]
fn concurrent_leases_and_maintenance_stress() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(DownloadCache::new(dir.path().to_path_buf(), 500));
    for i in 0..10 {
        write(&cache, &format!("item_{i}.bin"), 100);
    }

    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();

    for thread_idx in 0..4 {
        let c = Arc::clone(&cache);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            for i in 0..50 {
                let key = format!("item_{}.bin", (thread_idx + i) % 10);
                let _ = c.hit(&key);
                if let Ok(lease) = c.lease(&key) {
                    let _ = std::fs::write(lease.path(), b"touch");
                }
            }
        }));
    }

    for _ in 0..4 {
        let c = Arc::clone(&cache);
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            b.wait();
            for _ in 0..20 {
                c.enforce_quota();
                let _ = c.size_bytes();
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
