use std::collections::HashSet;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;

use aria_storage::{
    CACHE_REGISTRY, CacheId, CacheKind, CacheTier, NeverCancelled, SharedStorageLayout,
    SharedStorageLayoutError, SharedStorageLayoutTransitionError, StorageError, StorageLayout,
    cache_descriptor_by_key, clear_cache, measure_disk_usage, relocate_cache,
};

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        for _ in 0..128 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("aria-storage-integration-{id}"));
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

#[path = "storage_layout_and_validation.rs"]
mod layout_and_validation;

#[path = "storage_shared_layout.rs"]
mod shared_layout;

#[path = "storage_usage_and_relocation.rs"]
mod usage_and_relocation;

#[path = "storage_cache_contracts.rs"]
mod cache_contracts;
