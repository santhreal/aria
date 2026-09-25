//! Tests for this module.
//!
//! The cases live in sibling files, one per area, declared at the bottom. This
//! file holds only the shared fixtures; a child sees them through `use super::*`.

//! Tests for [`cache`].
//!
//! Declared from `cache.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;
use std::sync::atomic::{AtomicU8, AtomicUsize};
use std::sync::{Arc, Barrier};
use std::thread;

fn write(cache: &DownloadCache, key: &str, len: usize) -> PathBuf {
    let path = cache.path_for(key).unwrap();
    std::fs::write(&path, vec![0u8; len]).unwrap();
    path
}

fn backdate(path: &Path) {
    let earlier = SystemTime::now() - std::time::Duration::from_secs(120);
    let file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(earlier))
        .unwrap();
}

#[path = "cache_tests_switch_root.rs"]
mod switch_root;

#[path = "cache_tests_maintenance_excludes.rs"]
mod maintenance_excludes;
