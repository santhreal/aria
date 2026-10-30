//! Absolute storage roots and shared, versioned layouts.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};

use crate::cache_registry::{CacheId, CacheKind, cache_descriptors};
use crate::error::StorageError;
use crate::ops::{normalize_absolute, paths_overlap, resolve_physical_cache_root};

/// Absolute storage root plus optional per-disk-cache path overrides.
///
/// Overrides are held in a [`BTreeMap`] so snapshots and diagnostics remain
/// deterministic regardless of insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    root: PathBuf,
    overrides: BTreeMap<CacheId, PathBuf>,
}

impl StorageLayout {
    /// Create a layout with no overrides.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = normalize_absolute(&root.into(), "storage root", false)?;
        Ok(Self {
            root,
            overrides: BTreeMap::new(),
        })
    }

    /// Create a layout from stable string keys and absolute paths.
    ///
    /// Unknown keys, memory-cache keys, relative paths, and duplicate keys
    /// are rejected rather than ignored.
    pub fn with_overrides<I, K, P>(
        root: impl Into<PathBuf>,
        overrides: I,
    ) -> Result<Self, StorageError>
    where
        I: IntoIterator<Item = (K, P)>,
        K: AsRef<str>,
        P: Into<PathBuf>,
    {
        let mut layout = Self::new(root)?;
        for (key, path) in overrides {
            let id = CacheId::parse(key.as_ref()).map_err(|_| StorageError::UnknownCacheId)?;
            if layout.overrides.contains_key(&id) {
                return Err(StorageError::DuplicateOverride(id));
            }
            layout.set_override(id, path)?;
        }
        Ok(layout)
    }

    /// Absolute default root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Deterministically ordered cache overrides.
    pub fn overrides(&self) -> &BTreeMap<CacheId, PathBuf> {
        &self.overrides
    }

    /// Return the override for one cache, if set.
    pub fn override_for(&self, id: CacheId) -> Option<&Path> {
        self.overrides.get(&id).map(PathBuf::as_path)
    }

    /// Set an absolute override for a disk cache.
    pub fn set_override(
        &mut self,
        id: CacheId,
        path: impl Into<PathBuf>,
    ) -> Result<Option<PathBuf>, StorageError> {
        if id.descriptor().kind != CacheKind::Disk {
            return Err(StorageError::CacheIsNotDisk(id));
        }
        let path = normalize_absolute(&path.into(), "cache override", false)?;
        self.ensure_override_is_disjoint(id, &path)?;
        Ok(self.overrides.insert(id, path))
    }

    /// Set an override using an exact stable cache key.
    pub fn set_override_key(
        &mut self,
        key: &str,
        path: impl Into<PathBuf>,
    ) -> Result<Option<PathBuf>, StorageError> {
        let id = CacheId::parse(key).map_err(|_| StorageError::UnknownCacheId)?;
        self.set_override(id, path)
    }

    /// Remove and return an override.
    pub fn remove_override(&mut self, id: CacheId) -> Option<PathBuf> {
        self.overrides.remove(&id)
    }

    /// Resolve a cache path.
    ///
    /// Memory caches return `None`. Disk caches use an override when present,
    /// otherwise `root/default_relative`.
    pub fn resolve(&self, id: CacheId) -> Option<PathBuf> {
        let descriptor = id.descriptor();
        if descriptor.kind == CacheKind::Memory {
            return None;
        }
        self.overrides.get(&id).cloned().or_else(|| {
            descriptor
                .default_relative
                .map(|relative| self.root.join(relative))
        })
    }

    /// Resolve every disk cache in deterministic registry order.
    pub fn resolved_disk_paths(&self) -> Vec<(CacheId, PathBuf)> {
        cache_descriptors()
            .iter()
            .filter_map(|descriptor| {
                self.resolve(descriptor.id)
                    .map(|path| (descriptor.id, path))
            })
            .collect()
    }

    /// Validate every resolved disk cache root against the current filesystem.
    ///
    /// Existing parent symlinks are resolved to compare physical locations.
    /// Existing cache roots must be real directories, while missing roots are
    /// allowed beneath the nearest existing directory. This check is read-only
    /// and never creates missing paths.
    ///
    /// This is a point-in-time validation, not a race-free filesystem
    /// transaction; filesystem entries can change after it returns.
    pub fn validate_filesystem(&self) -> Result<(), StorageError> {
        let mut physical_paths = Vec::new();
        for descriptor in cache_descriptors() {
            if descriptor.kind != CacheKind::Disk {
                continue;
            }
            let path = self
                .resolve(descriptor.id)
                .ok_or(StorageError::DangerousPath("disk cache has no path"))?;
            physical_paths.push((descriptor.id, resolve_physical_cache_root(&path)?));
        }

        for (index, (cache, path)) in physical_paths.iter().enumerate() {
            for (other, other_path) in &physical_paths[index + 1..] {
                if paths_overlap(path, other_path) {
                    return Err(StorageError::CachePathsOverlap {
                        cache: *cache,
                        other: *other,
                    });
                }
            }
        }
        Ok(())
    }

    fn ensure_override_is_disjoint(
        &self,
        id: CacheId,
        candidate: &Path,
    ) -> Result<(), StorageError> {
        for descriptor in cache_descriptors() {
            if descriptor.id == id || descriptor.kind != CacheKind::Disk {
                continue;
            }
            let default = self
                .root
                .join(descriptor.default_relative.expect("disk cache has a path"));
            if paths_overlap(candidate, &default)
                || self
                    .overrides
                    .get(&descriptor.id)
                    .is_some_and(|other| paths_overlap(candidate, other))
            {
                return Err(StorageError::CachePathsOverlap {
                    cache: id,
                    other: descriptor.id,
                });
            }
        }
        Ok(())
    }
}

/// An immutable, versioned copy of a [`SharedStorageLayout`].
///
/// The layout and generation are captured while holding one read lock, so
/// every path resolved through this snapshot belongs to the reported
/// generation. Mutating a layout returned by [`Self::into_parts`] cannot
/// affect the shared value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayoutSnapshot {
    layout: StorageLayout,
    generation: u64,
}

impl StorageLayoutSnapshot {
    /// Return the captured layout.
    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    /// Return the generation captured with the layout.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Resolve a cache against the captured layout.
    pub fn resolve(&self, id: CacheId) -> Option<PathBuf> {
        self.layout.resolve(id)
    }

    /// Consume the snapshot into its detached layout and generation.
    pub fn into_parts(self) -> (StorageLayout, u64) {
        (self.layout, self.generation)
    }
}

/// A borrowed layout generation held stable for one complete cache operation.
///
/// While this lease exists, a writer cannot publish a replacement layout.
/// Cache workers should acquire one at the start of an operation and retain it
/// until every filesystem access using its resolved paths has finished.
pub struct StorageLayoutLease<'a> {
    layout: StorageLayout,
    generation: u64,
    shared: &'a SharedStorageLayout,
}

impl StorageLayoutLease<'_> {
    /// Return the coherently borrowed layout.
    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    /// Return the generation held by this lease.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Resolve a cache against the held generation.
    pub fn resolve(&self, id: CacheId) -> Option<PathBuf> {
        self.layout.resolve(id)
    }
}

impl fmt::Debug for StorageLayoutLease<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageLayoutLease")
            .field("layout", &self.layout)
            .field("generation", &self.generation)
            .finish()
    }
}

impl Drop for StorageLayoutLease<'_> {
    fn drop(&mut self) {
        self.shared.release_read();
    }
}

/// A cheap-to-clone, thread-safe, versioned [`StorageLayout`].
///
/// Clones share one layout through an [`Arc`]. Readers observe the layout and
/// generation coherently, while writers use an expected generation to prevent
/// stale worker results from replacing newer configuration. The initial
/// generation is zero and each successful write increments it exactly once.
///
/// Write operations prepare a complete replacement before committing it, so an
/// unwinding update leaves the prior layout and generation valid.
#[derive(Debug, Clone)]
pub struct SharedStorageLayout {
    inner: Arc<SharedStorageLayoutInner>,
}

#[derive(Debug)]
struct SharedStorageLayoutInner {
    state: Mutex<SharedStorageLayoutState>,
    condvar: Condvar,
}

#[derive(Debug)]
struct SharedStorageLayoutState {
    layout: StorageLayout,
    generation: u64,
    active_readers: usize,
    writer_active: bool,
    next_batch_id: u64,
    queue: VecDeque<Batch>,
}

#[derive(Debug)]
enum BatchKind {
    Readers { remaining_to_enter: usize },
    Writer,
}

#[derive(Debug)]
struct Batch {
    id: u64,
    kind: BatchKind,
}

struct WriteGuard<'a> {
    shared: &'a SharedStorageLayout,
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        self.shared.release_write();
    }
}

impl SharedStorageLayout {
    /// Create shared storage state at generation zero.
    ///
    /// The supplied layout has already been validated by [`StorageLayout`]'s
    /// constructors and mutation methods.
    pub fn new(layout: StorageLayout) -> Self {
        Self::from_generation(layout, 0)
    }

    /// Clone the current layout and generation under one mutex hold.
    ///
    /// This does not join the lease queue. A lease already held on this thread
    /// may still snapshot while a writer is waiting; taking another lease
    /// instead would wait behind that writer and never drop, which is a deadlock.
    pub fn snapshot(&self) -> StorageLayoutSnapshot {
        let (layout, generation) = self.observe();
        StorageLayoutSnapshot { layout, generation }
    }

    /// Hold one generation stable for a complete cache operation.
    ///
    /// Keep the returned lease alive until all work using its paths finishes.
    /// Long-lived leases delay relocation, so callers should scope them to one
    /// refresh, download, install, render, or maintenance operation.
    pub fn lease(&self) -> StorageLayoutLease<'_> {
        let (layout, generation) = self.acquire_read();
        StorageLayoutLease {
            layout,
            generation,
            shared: self,
        }
    }

    /// Resolve one cache from a single coherent read of the current layout.
    ///
    /// Memory caches return `None`.
    pub fn resolve(&self, id: CacheId) -> Option<PathBuf> {
        self.resolve_versioned(id).0
    }

    /// Resolve one cache together with the generation used for that path.
    ///
    /// The returned path and generation are captured under the same read lock.
    /// Memory caches return `None` for the path.
    pub fn resolve_versioned(&self, id: CacheId) -> (Option<PathBuf>, u64) {
        let (layout, generation) = self.observe();
        (layout.resolve(id), generation)
    }

    /// Replace the layout when `expected_generation` is still current.
    ///
    /// The replacement must be a previously validated [`StorageLayout`].
    /// Success returns the newly installed generation. A stale generation or
    /// exhausted generation leaves the current layout untouched.
    pub fn replace(
        &self,
        expected_generation: u64,
        replacement: StorageLayout,
    ) -> Result<u64, SharedStorageLayoutError> {
        let _write_guard = self.acquire_write();
        let mut state = self.inner.state.lock();
        let next_generation = checked_next_generation(&state, expected_generation)?;
        state.layout = replacement;
        state.generation = next_generation;
        Ok(next_generation)
    }

    /// Run an exclusive transition and publish `replacement` on success.
    ///
    /// Existing [`StorageLayoutLease`] values finish before `transition`
    /// starts, and new leases wait until this method returns. The callback
    /// receives the old and replacement layouts while publication is
    /// exclusive. A callback error or panic leaves the shared layout and
    /// generation unchanged.
    pub fn transition<F, E>(
        &self,
        expected_generation: u64,
        replacement: StorageLayout,
        transition: F,
    ) -> Result<u64, SharedStorageLayoutTransitionError<E>>
    where
        F: FnOnce(&StorageLayout, &StorageLayout) -> Result<(), E>,
    {
        let _write_guard = self.acquire_write();
        let (old_layout, next_generation) = {
            let state = self.inner.state.lock();
            let next_generation = checked_next_generation(&state, expected_generation)
                .map_err(SharedStorageLayoutTransitionError::Layout)?;
            (state.layout.clone(), next_generation)
        };
        transition(&old_layout, &replacement)
            .map_err(SharedStorageLayoutTransitionError::Transition)?;
        {
            let mut state = self.inner.state.lock();
            state.layout = replacement;
            state.generation = next_generation;
        }
        Ok(next_generation)
    }

    /// Update a cloned layout when `expected_generation` is still current.
    ///
    /// The callback runs with the write lock held and should be brief. It must
    /// not call methods on this shared value, which would deadlock. The clone
    /// begins valid, and [`StorageLayout`] mutation methods preserve that
    /// validity. The callback is responsible for handling their errors.
    ///
    /// A stale or exhausted generation refuses the update without invoking the
    /// callback. If the callback panics, the panic propagates and no state is
    /// committed.
    pub fn update<F>(
        &self,
        expected_generation: u64,
        update: F,
    ) -> Result<u64, SharedStorageLayoutError>
    where
        F: FnOnce(&mut StorageLayout),
    {
        let _write_guard = self.acquire_write();
        let (mut replacement, next_generation) = {
            let state = self.inner.state.lock();
            let next_generation = checked_next_generation(&state, expected_generation)?;
            (state.layout.clone(), next_generation)
        };
        update(&mut replacement);
        {
            let mut state = self.inner.state.lock();
            state.layout = replacement;
            state.generation = next_generation;
        }
        Ok(next_generation)
    }

    fn from_generation(layout: StorageLayout, generation: u64) -> Self {
        Self {
            inner: Arc::new(SharedStorageLayoutInner {
                state: Mutex::new(SharedStorageLayoutState {
                    layout,
                    generation,
                    active_readers: 0,
                    writer_active: false,
                    next_batch_id: 0,
                    queue: VecDeque::new(),
                }),
                condvar: Condvar::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_generation_for_test(layout: StorageLayout, generation: u64) -> Self {
        Self::from_generation(layout, generation)
    }

    /// Point-in-time copy. Does not increment `active_readers` or wait for writers.
    fn observe(&self) -> (StorageLayout, u64) {
        let state = self.inner.state.lock();
        (state.layout.clone(), state.generation)
    }

    fn acquire_read(&self) -> (StorageLayout, u64) {
        let mut state = self.inner.state.lock();
        if state.queue.is_empty() && !state.writer_active {
            state.active_readers += 1;
            return (state.layout.clone(), state.generation);
        }

        let batch_id = if let Some(last) = state.queue.back_mut() {
            match &mut last.kind {
                BatchKind::Readers { remaining_to_enter } => {
                    *remaining_to_enter += 1;
                    last.id
                }
                BatchKind::Writer => {
                    let id = state.next_batch_id;
                    state.next_batch_id += 1;
                    state.queue.push_back(Batch {
                        id,
                        kind: BatchKind::Readers {
                            remaining_to_enter: 1,
                        },
                    });
                    id
                }
            }
        } else {
            let id = state.next_batch_id;
            state.next_batch_id += 1;
            state.queue.push_back(Batch {
                id,
                kind: BatchKind::Readers {
                    remaining_to_enter: 1,
                },
            });
            id
        };

        struct WaitGuard<'a> {
            shared: &'a SharedStorageLayout,
            batch_id: u64,
            entered: bool,
        }

        impl Drop for WaitGuard<'_> {
            fn drop(&mut self) {
                if !self.entered {
                    self.shared.cancel_waiting_reader(self.batch_id);
                }
            }
        }

        let mut wait_guard = WaitGuard {
            shared: self,
            batch_id,
            entered: false,
        };

        loop {
            if !state.writer_active
                && state
                    .queue
                    .front()
                    .is_some_and(|front| front.id == batch_id)
            {
                state.active_readers += 1;
                let batch_drained = {
                    let front = state
                        .queue
                        .front_mut()
                        .expect("the matching reader batch is at the front");
                    let BatchKind::Readers { remaining_to_enter } = &mut front.kind else {
                        unreachable!("a reader wait cannot match a writer batch");
                    };
                    *remaining_to_enter -= 1;
                    *remaining_to_enter == 0
                };
                if batch_drained {
                    state.queue.pop_front();
                    self.inner.condvar.notify_all();
                }
                wait_guard.entered = true;
                return (state.layout.clone(), state.generation);
            }
            self.inner.condvar.wait(&mut state);
        }
    }

    fn cancel_waiting_reader(&self, batch_id: u64) {
        let mut state = self.inner.state.lock();
        if let Some(pos) = state.queue.iter().position(|b| b.id == batch_id)
            && let BatchKind::Readers { remaining_to_enter } = &mut state.queue[pos].kind
        {
            *remaining_to_enter = remaining_to_enter.saturating_sub(1);
            if *remaining_to_enter == 0 {
                state.queue.remove(pos);
                self.inner.condvar.notify_all();
            }
        }
    }

    fn release_read(&self) {
        let mut state = self.inner.state.lock();
        state.active_readers = state
            .active_readers
            .checked_sub(1)
            .expect("active_readers underflow");
        if state.active_readers == 0 {
            self.inner.condvar.notify_all();
        }
    }

    fn acquire_write(&self) -> WriteGuard<'_> {
        let mut state = self.inner.state.lock();
        if state.queue.is_empty() && !state.writer_active && state.active_readers == 0 {
            state.writer_active = true;
            return WriteGuard { shared: self };
        }

        let id = state.next_batch_id;
        state.next_batch_id += 1;
        state.queue.push_back(Batch {
            id,
            kind: BatchKind::Writer,
        });

        struct WaitGuard<'a> {
            shared: &'a SharedStorageLayout,
            batch_id: u64,
            entered: bool,
        }

        impl Drop for WaitGuard<'_> {
            fn drop(&mut self) {
                if !self.entered {
                    self.shared.cancel_waiting_writer(self.batch_id);
                }
            }
        }

        let mut wait_guard = WaitGuard {
            shared: self,
            batch_id: id,
            entered: false,
        };

        loop {
            if !state.writer_active
                && state.active_readers == 0
                && let Some(front) = state.queue.front()
                && front.id == id
            {
                state.queue.pop_front();
                state.writer_active = true;
                wait_guard.entered = true;
                return WriteGuard { shared: self };
            }
            self.inner.condvar.wait(&mut state);
        }
    }

    fn cancel_waiting_writer(&self, batch_id: u64) {
        let mut state = self.inner.state.lock();
        if let Some(pos) = state.queue.iter().position(|b| b.id == batch_id) {
            state.queue.remove(pos);
            self.inner.condvar.notify_all();
        }
    }

    fn release_write(&self) {
        let mut state = self.inner.state.lock();
        state.writer_active = false;
        self.inner.condvar.notify_all();
    }
}

impl From<StorageLayout> for SharedStorageLayout {
    fn from(layout: StorageLayout) -> Self {
        Self::new(layout)
    }
}

fn checked_next_generation(
    state: &SharedStorageLayoutState,
    expected_generation: u64,
) -> Result<u64, SharedStorageLayoutError> {
    if state.generation != expected_generation {
        return Err(SharedStorageLayoutError::StaleGeneration {
            expected: expected_generation,
            current: state.generation,
        });
    }
    state
        .generation
        .checked_add(1)
        .ok_or(SharedStorageLayoutError::GenerationExhausted)
}

// The error types live beside this file, declared here. A pure move: the
// module tree is unchanged, so `crate::layout::SharedStorageLayoutError` still
// names the same type.
#[path = "layout_errors.rs"]
mod errors;

pub use errors::{SharedStorageLayoutError, SharedStorageLayoutTransitionError};
