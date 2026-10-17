//! Process-wide allocator wrapper for counting render-thread heap allocations.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_THREAD_TAG: AtomicU64 = AtomicU64::new(1);
static TRACKED_THREAD_TAG: AtomicU64 = AtomicU64::new(0);
static TRACKED_ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static TRACKED_ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

std::thread_local! {
    static THREAD_TAG: Cell<u64> = const { Cell::new(0) };
}

/// System allocator wrapper that counts allocations on one watched thread when
/// a measurement scope is active.
#[derive(Debug, Clone, Copy, Default)]
pub struct CountingAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

#[inline]
fn record_if_tracked(bytes: usize) {
    let target = TRACKED_THREAD_TAG.load(Ordering::Relaxed);
    if target != 0
        && THREAD_TAG
            .try_with(|cell| cell.get() == target)
            .unwrap_or(false)
    {
        TRACKED_ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        TRACKED_ALLOC_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_if_tracked(layout.size());
        // SAFETY: Forwards directly to the standard system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forwards directly to the standard system allocator with the same pointer and layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_if_tracked(layout.size());
        // SAFETY: Forwards directly to the standard system allocator with the same layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_if_tracked(new_size);
        // SAFETY: Forwards directly to the standard system allocator with the same parameters.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Allocation count and byte total captured during a measurement window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocStats {
    pub allocations: u64,
    pub bytes: u64,
}

struct ResetGuard;

impl Drop for ResetGuard {
    fn drop(&mut self) {
        TRACKED_THREAD_TAG.store(0, Ordering::Release);
    }
}

/// Measure heap allocations performed on the calling thread while `work` runs.
pub fn measure_thread_allocs<T>(work: impl FnOnce() -> T) -> (T, AllocStats) {
    let tag = THREAD_TAG.with(|cell| {
        let current = cell.get();
        if current == 0 {
            let next = NEXT_THREAD_TAG.fetch_add(1, Ordering::Relaxed);
            cell.set(next);
            next
        } else {
            current
        }
    });
    TRACKED_ALLOC_COUNT.store(0, Ordering::Relaxed);
    TRACKED_ALLOC_BYTES.store(0, Ordering::Relaxed);
    TRACKED_THREAD_TAG.store(tag, Ordering::Release);
    let _guard = ResetGuard;
    let out = work();
    TRACKED_THREAD_TAG.store(0, Ordering::Release);
    let stats = AllocStats {
        allocations: TRACKED_ALLOC_COUNT.load(Ordering::Relaxed),
        bytes: TRACKED_ALLOC_BYTES.load(Ordering::Relaxed),
    };
    (out, stats)
}
