use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::ops::{RelocationStrategy, relocate_with_strategy};

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        for _ in 0..128 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("aria-storage-unit-{id}"));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
        panic!("could not allocate test directory");
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn shared_layout_generation_exhaustion_fails_closed() {
    let temporary = TestDirectory::new();
    let original = StorageLayout::new(temporary.path.join("original")).unwrap();
    let replacement = StorageLayout::new(temporary.path.join("replacement")).unwrap();
    let shared = SharedStorageLayout::with_generation_for_test(original.clone(), u64::MAX);

    assert_eq!(
        shared.replace(u64::MAX, replacement),
        Err(SharedStorageLayoutError::GenerationExhausted)
    );

    let update_was_called = std::cell::Cell::new(false);
    assert_eq!(
        shared.update(u64::MAX, |_| update_was_called.set(true)),
        Err(SharedStorageLayoutError::GenerationExhausted)
    );
    assert!(!update_was_called.get());

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.generation(), u64::MAX);
    assert_eq!(snapshot.layout(), &original);
}

#[test]
fn forced_copy_strategy_exercises_fallback_without_second_volume() {
    let temporary = TestDirectory::new();
    let old_path = temporary.path.join("old");
    let new_path = temporary.path.join("new");
    fs::create_dir(&old_path).unwrap();
    fs::create_dir(old_path.join("nested")).unwrap();
    fs::write(old_path.join("nested").join("data"), b"fallback").unwrap();

    let report = relocate_with_strategy(
        &old_path,
        &new_path,
        &NeverCancelled,
        |completed| {
            assert_eq!(
                fs::read(completed.join("nested").join("data")).unwrap(),
                b"fallback"
            );
            assert!(old_path.exists(), "copy source must survive until persist");
            Ok(())
        },
        RelocationStrategy::ForceCopy,
    )
    .unwrap();

    assert!(report.used_copy_fallback);
    assert_eq!(report.bytes, 8);
    assert_eq!(report.files, 1);
    assert!(!old_path.exists());
    assert_eq!(
        fs::read(new_path.join("nested").join("data")).unwrap(),
        b"fallback"
    );
}

/// WHY: a cross-device relocation killed part way (SIGKILL, a crash, a power
/// cut) left its hidden `.aria-storage-relocation-…tmp` directory, with a
/// partial copy of the whole cache, beside the destination for good. The
/// next copying relocation into that folder deletes every such directory no
/// live relocation holds, with its lock file, and leaves a live one and every
/// other entry alone. Not caught: a live relocation from a build that made no
/// lock file, whose directory reads as dead.
#[test]
fn the_next_relocation_deletes_what_a_killed_relocation_left() {
    use aria_core::scratch::{self, ScratchLock};

    let temporary = TestDirectory::new();
    let old_path = temporary.path.join("old");
    fs::create_dir(&old_path).unwrap();
    fs::write(old_path.join("data"), b"cache").unwrap();
    let scratch_name = |pid: u32| format!(".aria-storage-relocation-{pid}-1.tmp");
    let live = temporary.path.join(scratch_name(6));
    let held = ScratchLock::claim(&live).unwrap().expect("free name");
    fs::create_dir(&live).unwrap();
    // Dead: a directory beside an unheld lock file, and one with no lock file.
    for pid in [7, 8] {
        let dead = temporary.path.join(scratch_name(pid));
        fs::create_dir(&dead).unwrap();
        fs::write(dead.join("data"), b"partial").unwrap();
    }
    fs::File::create(scratch::lock_path(&temporary.path.join(scratch_name(7)))).unwrap();
    // Not relocation names: each number must be digits.
    for other in ["notes", "9-x", "old-copy"] {
        let name = format!(".aria-storage-relocation-{other}.tmp");
        fs::create_dir(temporary.path.join(name)).unwrap();
    }

    relocate_with_strategy(
        &old_path,
        &temporary.path.join("new"),
        &NeverCancelled,
        |_| Ok(()),
        RelocationStrategy::ForceCopy,
    )
    .unwrap();

    let mut names: Vec<String> = fs::read_dir(&temporary.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            ".aria-storage-relocation-6-1.tmp",
            ".aria-storage-relocation-6-1.tmp.lock",
            ".aria-storage-relocation-9-x.tmp",
            ".aria-storage-relocation-notes.tmp",
            ".aria-storage-relocation-old-copy.tmp",
            "new",
        ]
    );
    drop(held);
}

#[test]
fn forced_copy_persistence_failure_removes_destination_not_source() {
    let temporary = TestDirectory::new();
    let old_path = temporary.path.join("old");
    let new_path = temporary.path.join("new");
    fs::create_dir(&old_path).unwrap();
    fs::write(old_path.join("data"), b"authoritative").unwrap();

    let error = relocate_with_strategy(
        &old_path,
        &new_path,
        &NeverCancelled,
        |_| Err("settings unavailable".into()),
        RelocationStrategy::ForceCopy,
    )
    .unwrap_err();

    assert!(matches!(error, StorageError::PersistenceFailed { .. }));
    assert_eq!(fs::read(old_path.join("data")).unwrap(), b"authoritative");
    assert!(!new_path.exists());
}

#[test]
fn persistence_errors_are_bounded() {
    let temporary = TestDirectory::new();
    let old_path = temporary.path.join("old");
    let new_path = temporary.path.join("new");
    fs::create_dir(&old_path).unwrap();

    let error = relocate_cache(&old_path, &new_path, &NeverCancelled, |_| {
        Err("x".repeat(16_384))
    })
    .unwrap_err();
    assert!(error.to_string().len() < 400);
    assert!(old_path.exists());
    assert!(!new_path.exists());
}

#[test]
fn rollback_failure_is_reported_explicitly() {
    let temporary = TestDirectory::new();
    let old_path = temporary.path.join("old");
    let new_path = temporary.path.join("new");
    fs::create_dir(&old_path).unwrap();
    fs::write(old_path.join("original"), b"data").unwrap();

    let error = relocate_cache(&old_path, &new_path, &NeverCancelled, |_| {
        fs::create_dir(&old_path).unwrap();
        Err("settings unavailable".into())
    })
    .unwrap_err();

    assert!(matches!(error, StorageError::RollbackFailed { .. }));
    assert_eq!(fs::read(new_path.join("original")).unwrap(), b"data");
}

#[test]
fn deep_tree_traversal_is_rejected_at_depth_limit() {
    let temporary = TestDirectory::new();
    let mut current = temporary.path.join("root");
    fs::create_dir(&current).unwrap();
    for i in 0..70 {
        current = current.join(format!("level_{i}"));
        fs::create_dir(&current).unwrap();
    }
    fs::write(current.join("file.txt"), b"deep").unwrap();

    let result = measure_disk_usage(temporary.path.join("root"), &NeverCancelled);
    assert!(matches!(result, Err(StorageError::InvalidPath(_))));
}

#[test]
fn snapshot_under_a_lease_does_not_wait_behind_a_writer() {
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    let temporary = TestDirectory::new();
    let layout = StorageLayout::new(temporary.path.join("root")).unwrap();
    let shared = SharedStorageLayout::new(layout);
    let barrier = Arc::new(Barrier::new(2));
    let (tx, rx) = mpsc::channel();

    let writer = {
        let s = shared.clone();
        let b = Arc::clone(&barrier);
        let dest = temporary.path.join("next");
        thread::spawn(move || {
            b.wait();
            let snap = s.snapshot();
            let new_layout = StorageLayout::new(dest).unwrap();
            s.replace(snap.generation(), new_layout).expect("replace");
        })
    };

    let reader = {
        let s = shared;
        let b = Arc::clone(&barrier);
        thread::spawn(move || {
            let lease = s.lease();
            b.wait();
            // The writer is now blocked on this lease. Observing must still finish.
            let snap = s.snapshot();
            assert!(snap.generation() >= lease.generation());
            let _ = s.resolve(crate::CacheId::Proxies);
            drop(lease);
            tx.send(()).unwrap();
        })
    };

    rx.recv_timeout(Duration::from_secs(5))
        .expect("snapshot under a lease deadlocked behind a waiting writer");
    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn concurrent_lease_and_relocation_fairness_stress() {
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let temporary = TestDirectory::new();
    let layout = StorageLayout::new(temporary.path.join("root")).unwrap();
    let shared = SharedStorageLayout::new(layout);

    let barrier = Arc::new(Barrier::new(6));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();

    // 4 reader threads continuously acquiring leases and snapshots
    for _ in 0..4 {
        let s = shared.clone();
        let b = Arc::clone(&barrier);
        let done = tx.clone();
        handles.push(thread::spawn(move || {
            b.wait();
            let mut last_gen = 0;
            for _ in 0..100 {
                let lease = s.lease();
                let generation = lease.generation();
                assert!(generation >= last_gen, "lease generation must be monotonic");
                last_gen = generation;
                let resolved = lease.resolve(crate::CacheId::Proxies);
                assert_eq!(
                    resolved,
                    lease.layout().resolve(crate::CacheId::Proxies),
                    "lease resolved path must match lease layout"
                );
                let snapshot = s.snapshot();
                assert!(snapshot.generation() >= generation);
                drop(lease);
            }
            done.send(()).unwrap();
        }));
    }

    // 2 writer threads replacing layout and performing transitions
    for t in 0..2 {
        let s = shared.clone();
        let b = Arc::clone(&barrier);
        let temp_path = temporary.path.clone();
        let done = tx.clone();
        handles.push(thread::spawn(move || {
            b.wait();
            for i in 0..20 {
                let snap = s.snapshot();
                let new_layout =
                    StorageLayout::new(temp_path.join(format!("root_{t}_{i}"))).unwrap();
                if i % 2 == 0 {
                    let _ = s.replace(snap.generation(), new_layout);
                } else {
                    let _ = s.transition(snap.generation(), new_layout, |old, new| {
                        assert_ne!(old.root(), new.root());
                        Ok::<(), io::Error>(())
                    });
                }
            }
            done.send(()).unwrap();
        }));
    }
    drop(tx);

    for _ in 0..6 {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("lease fairness stress deadlocked");
    }
    for handle in handles {
        handle.join().unwrap();
    }
}
