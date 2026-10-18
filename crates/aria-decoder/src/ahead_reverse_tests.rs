//! Reverse look-ahead and allocation contracts for [`AheadDecoder`].

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering as Atomic};

use super::state::Behind;
use super::tests::{Counts, Mock, R30, ahead, cpu_frame, gpu_frame, pts, rt};
use super::*;

static NEXT_THREAD_TAG: AtomicU64 = AtomicU64::new(1);
static WATCHED_THREAD_TAG: AtomicU64 = AtomicU64::new(0);
static WATCHED_ALLOCS: AtomicU64 = AtomicU64::new(0);

std::thread_local! {
    static THREAD_TAG: Cell<u64> = const { Cell::new(0) };
}

pub(crate) struct TestCountingAllocator;

#[global_allocator]
static TEST_ALLOCATOR: TestCountingAllocator = TestCountingAllocator;

unsafe impl GlobalAlloc for TestCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let watched = WATCHED_THREAD_TAG.load(Atomic::Relaxed);
        if watched != 0
            && THREAD_TAG
                .try_with(|cell| cell.get() == watched)
                .unwrap_or(false)
        {
            WATCHED_ALLOCS.fetch_add(1, Atomic::Relaxed);
        }
        // SAFETY: Forwards directly to the system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forwards directly to the system allocator with the same pointer and layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let watched = WATCHED_THREAD_TAG.load(Atomic::Relaxed);
        if watched != 0
            && THREAD_TAG
                .try_with(|cell| cell.get() == watched)
                .unwrap_or(false)
        {
            WATCHED_ALLOCS.fetch_add(1, Atomic::Relaxed);
        }
        // SAFETY: Forwards directly to the system allocator with the same layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let watched = WATCHED_THREAD_TAG.load(Atomic::Relaxed);
        if watched != 0
            && THREAD_TAG
                .try_with(|cell| cell.get() == watched)
                .unwrap_or(false)
        {
            WATCHED_ALLOCS.fetch_add(1, Atomic::Relaxed);
        }
        // SAFETY: Forwards directly to the system allocator with the same arguments.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

struct WatchReset;

impl Drop for WatchReset {
    fn drop(&mut self) {
        WATCHED_THREAD_TAG.store(0, Atomic::Release);
    }
}

pub(crate) fn count_thread_allocs<T>(work: impl FnOnce() -> T) -> (T, u64) {
    let tag = THREAD_TAG.with(|cell| {
        let current = cell.get();
        if current == 0 {
            let next = NEXT_THREAD_TAG.fetch_add(1, Atomic::Relaxed);
            cell.set(next);
            next
        } else {
            current
        }
    });
    WATCHED_ALLOCS.store(0, Atomic::Relaxed);
    WATCHED_THREAD_TAG.store(tag, Atomic::Release);
    let _reset = WatchReset;
    let out = work();
    let count = WATCHED_ALLOCS.load(Atomic::Acquire);
    (out, count)
}

#[test]
fn reverse_walks_each_gop_once() {
    let (mock, counts) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    for frame in (0..40).rev() {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            Some(frame),
            "frame {frame}"
        );
    }
    decoder.idle();
    // The cold seek for 39, then one walk per GOP: 38, 29, 19, and 9.
    assert_eq!(counts.seeks(), 5);
    // 30..=39, then 30..=38, 20..=29, 10..=19, and 0..=9.
    assert_eq!(counts.decoded(), 49);
    assert_eq!(decoder.ahead(), 0, "nothing decodes ahead while reversing");
}

/// A backward request whose sync sample cannot start a decode backs off to
/// an earlier one, exactly as the wrapped decoder's `frame_at` does, and
/// returns the requested frame instead of the next sync sample.
///
/// Closes the class "decode-ahead walks back without the open-GOP back-off":
/// the backward walk landed on the next GOP and returned its first frame for
/// every target in the open GOP. Does not cover a decoder that emits
/// corrupt frames instead of skipping them.
#[test]
fn a_backward_walk_over_an_open_gop_backs_off_to_the_exact_frame() {
    let (mock, _) = Mock::new(60, 10);
    let mut decoder = ahead(mock.with_open_gops(&[20, 40]));
    for frame in (0..60).rev() {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            Some(frame),
            "frame {frame}"
        );
    }
    let (bare, _) = Mock::new(60, 10);
    let mut bare = bare.with_open_gops(&[20, 40]);
    let (mock, _) = Mock::new(60, 10);
    let mut decoder = ahead(mock.with_open_gops(&[20, 40]));
    for target in [45, 44, 41, 40, 25, 21, 20, 19, 59, 58, 3] {
        assert_eq!(
            pts(decoder.frame_at(rt(target))),
            pts(bare.frame_at(rt(target))),
            "target {target}"
        );
    }
}

#[test]
fn reverse_then_forward_returns_the_frames_in_order() {
    let (bare, _) = Mock::new(40, 10);
    let mut bare = bare;
    let (mock, _) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    let targets = [25, 24, 23, 15, 16, 17, 30, 2, 1, 3, 39];
    for target in targets {
        assert_eq!(
            pts(decoder.frame_at(rt(target))),
            pts(bare.frame_at(rt(target))),
            "target {target}"
        );
    }
}

#[test]
fn behind_keeps_the_newest_frames_within_its_byte_budget() {
    let mut behind = Behind::default();
    let size = BEHIND_BYTES / 3 + 1;
    for frame in 0..5 {
        behind.push(cpu_frame(frame, size));
        assert!(behind.bytes <= BEHIND_BYTES, "{} bytes held", behind.bytes);
    }
    let held: Vec<i64> = behind.frames.iter().map(|frame| frame.pts.value).collect();
    assert_eq!(held, [3, 4]);
    assert_eq!(behind.bytes, 2 * size);
    // The oldest held frame lies past 2: an earlier frame no longer held may
    // cover it, so it is not answered from here.
    assert!(behind.take(rt(2), R30).is_none());
    assert_eq!(
        behind.take(rt(3), R30).map(|frame| frame.pts.value),
        Some(3)
    );
    assert_eq!(behind.bytes, 0, "taking 3 drops the passed frame 4");
}

#[test]
fn behind_keeps_at_most_its_surface_budget() {
    let mut behind = Behind::default();
    for frame in 0..BEHIND_SURFACES as i64 + 3 {
        behind.push(gpu_frame(frame));
    }
    assert_eq!(behind.surfaces, BEHIND_SURFACES);
    assert_eq!(behind.frames.len(), BEHIND_SURFACES);
    assert_eq!(behind.frames.front().map(|frame| frame.pts.value), Some(3));
}

/// WHY: Closes the defect class where reverse playback across a GOP boundary
/// drains `Behind` to zero at the keyframe and then synchronously seeks and
/// walks the preceding GOP on the caller thread when the next earlier frame
/// is requested. Verifies that the preceding GOP is pre-walked on the decode
/// pool before the boundary frame returns, that crossing the boundary performs
/// zero seeks on that call, and that `Behind::prepend` respects `BEHIND_BYTES`
/// and `BEHIND_SURFACES`. Does not cover hardware codec latency inside a
/// platform backend.
#[test]
fn reverse_playback_across_gop_boundaries_prewalks_on_the_pool() {
    let (mock, counts): (Mock, Arc<Counts>) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    for frame in (30..=35).rev() {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            Some(frame),
            "warmup frame {frame}"
        );
    }
    let behind_at_boundary = decoder.behind();
    assert!(
        behind_at_boundary >= 10,
        "preceding GOP 20..=29 must be pre-walked before frame 29 is requested, got {behind_at_boundary}"
    );
    let caller_decodes = || {
        counts
            .threads()
            .into_iter()
            .filter(|name| !name.starts_with(DECODE_THREAD_PREFIX))
            .count()
    };
    let caller_before_boundary = caller_decodes();
    assert_eq!(pts(decoder.frame_at(rt(29))), Some(29));
    assert_eq!(
        caller_decodes(),
        caller_before_boundary,
        "frame 29 must be served from Behind without decoding on the caller thread"
    );
    for frame in (0..29).rev() {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            Some(frame),
            "reverse frame {frame}"
        );
    }
    assert_eq!(
        caller_decodes(),
        caller_before_boundary,
        "crossing GOP boundaries 20 -> 19 and 10 -> 9 must not decode on the caller thread"
    );
    decoder.idle();
    let threads = counts.threads();
    // Initial cold frame_at(35) walks 30..=35 (6 frames) and frame_at(34) walks
    // 30..=34 (5 frames) on the caller after the initial pool fill; every
    // subsequent pre-walked GOP (20..=29, 10..=19, 0..=9: 30 frames) runs on
    // the decode pool.
    assert!(
        threads.len() >= 30,
        "expected at least 30 decoded frames, got {}",
        threads.len()
    );
    let tail_threads = &threads[threads.len() - 30..];
    assert!(
        tail_threads
            .iter()
            .all(|name| name.starts_with(DECODE_THREAD_PREFIX)),
        "preceding GOP pre-walks must run on decode pool threads: {tail_threads:?}"
    );

    // Budget invariant across GOP prepend: newest frames before the current
    // GOP are retained within BEHIND_BYTES and BEHIND_SURFACES.
    let chunk = BEHIND_BYTES / 3 + 1;
    let mut current_gop = Behind::default();
    current_gop.push(cpu_frame(10, chunk));
    let mut prev_gop = Behind::default();
    prev_gop.push(cpu_frame(8, chunk));
    prev_gop.push(cpu_frame(9, chunk));
    current_gop.prepend(prev_gop);
    assert!(current_gop.bytes <= BEHIND_BYTES);
    let kept_pts: Vec<i64> = current_gop.frames.iter().map(|f| f.pts.value).collect();
    assert_eq!(kept_pts, [9, 10]);

    let mut current_gpu = Behind::default();
    for pts_val in 10..10 + (BEHIND_SURFACES as i64 / 2) {
        current_gpu.push(gpu_frame(pts_val));
    }
    let mut prev_gpu = Behind::default();
    for pts_val in 0..10 {
        prev_gpu.push(gpu_frame(pts_val));
    }
    current_gpu.prepend(prev_gpu);
    assert_eq!(current_gpu.surfaces, BEHIND_SURFACES);
    assert_eq!(
        current_gpu.frames.back().map(|f| f.pts.value),
        Some(10 + (BEHIND_SURFACES as i64 / 2) - 1)
    );
}

/// WHY: Closes the defect class where `schedule` boxes a closure on the heap
/// (`Box<dyn FnOnce() + Send>`) on every `frame_at` and `request` call that
/// wakes the decode pool. Verifies that across 64 consecutive pool-scheduling
/// requests, individual `schedule` calls do not allocate a closure on the
/// calling thread. Does not cover allocations inside a platform codec's own
/// `next_frame` implementation on the pool thread.
#[test]
fn schedule_queues_pool_serves_without_per_call_heap_allocation() {
    let (mock, _) = Mock::new(200, 10);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    decoder.idle();

    let mut zero_alloc_calls = 0usize;
    let mut total_allocs = 0u64;
    for frame in 1..=64i64 {
        let (got, allocs) = count_thread_allocs(|| decoder.frame_at(rt(frame)));
        assert_eq!(pts(got), Some(frame));
        if allocs == 0 {
            zero_alloc_calls += 1;
        }
        total_allocs += allocs;
        decoder.idle();
    }
    assert!(
        zero_alloc_calls >= 60,
        "expected at least 60 of 64 schedule calls to perform 0 heap allocations, got {zero_alloc_calls} (total_allocs={total_allocs})"
    );
    assert!(
        total_allocs <= 4,
        "expected at most 4 channel-block allocations across 64 schedule calls, got {total_allocs}"
    );
}
