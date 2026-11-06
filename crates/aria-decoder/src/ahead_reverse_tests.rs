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

    let current_count = BEHIND_SURFACES as i64 / 2;
    let prev_count = BEHIND_SURFACES as i64 - current_count + 2;
    let mut current_gpu = Behind::default();
    for pts_val in prev_count..prev_count + current_count {
        current_gpu.push(gpu_frame(pts_val));
    }
    let mut prev_gpu = Behind::default();
    for pts_val in 0..prev_count {
        prev_gpu.push(gpu_frame(pts_val));
    }
    current_gpu.prepend(prev_gpu);
    assert_eq!(current_gpu.surfaces, BEHIND_SURFACES);
    assert_eq!(
        current_gpu.frames.back().map(|f| f.pts.value),
        Some(prev_count + current_count - 1)
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

/// Wrapper around [`Mock`] that pauses its first decode-pool visit to
/// `pause_at` until the test thread signals `resume`.
struct GateMock {
    inner: Mock,
    pause_at: i64,
    entered: mpsc::SyncSender<()>,
    resume: Mutex<Option<Receiver<()>>>,
}

impl GateMock {
    fn new(inner: Mock, pause_at: i64) -> (Self, Receiver<()>, mpsc::SyncSender<()>) {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        (
            Self {
                inner,
                pause_at,
                entered: entered_tx,
                resume: Mutex::new(Some(resume_rx)),
            },
            entered_rx,
            resume_tx,
        )
    }
}

impl VideoDecoder for GateMock {
    fn info(&self) -> &SourceInfo {
        self.inner.info()
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        self.inner.seek(target)
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let on_pool = std::thread::current()
            .name()
            .is_some_and(|name| name.starts_with(DECODE_THREAD_PREFIX));
        let frame = self.inner.next_frame()?;
        if on_pool
            && frame.as_ref().is_some_and(|f| f.pts.value == self.pause_at)
            && let Some(rx) = self.resume.lock().take()
        {
            let _ = self.entered.try_send(());
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
        Ok(frame)
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        self.inner.frame_at(target)
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        self.inner.frame_at_nearest(target)
    }
}

/// WHY: Closes the defect class where 60-frame GOPs (`h264-1080p60`,
/// `hevc-2160p60`, `stack-4k`) delay `Op::BackFill` until only 15 frames
/// remain in `Behind` (45 frames into a 60-frame GOP) or evict pre-walked
/// 1080p frames because two 60-frame 1080p GOPs (`356 MiB`) exceed a 256 MiB
/// `BEHIND_BYTES` cap. Verifies that `Op::Back` near the end of a 60-frame
/// 1080p GOP immediately pre-walks the preceding 60-frame GOP without frame
/// eviction, and that stepping across the GOP boundary (`120 -> 119`)
/// immediately starts pre-walking the next preceding GOP (`0..=59`). Does not
/// cover hardware decoder surface pool exhaustion on GPU frames.
#[test]
fn reverse_60_frame_gops_prewalk_immediately_and_fit_two_1080p_gops() {
    const NV12_1080P_BYTES: usize = 1920 * 1080 * 3 / 2;
    let (mock, counts) = Mock::new(180, 60);
    let mut decoder = ahead(mock.with_planes(NV12_1080P_BYTES));

    assert_eq!(pts(decoder.frame_at(rt(179))), Some(179));
    assert_eq!(pts(decoder.frame_at(rt(178))), Some(178));
    decoder.idle();
    assert_eq!(
        decoder.behind(),
        118,
        "58 frames of 120..=177 plus all 60 pre-walked frames of 60..=119 must fit in Behind"
    );
    assert_eq!(counts.seeks(), 3, "seeks for 179, 178, and pre-walk 119");

    let caller_decodes = || {
        counts
            .threads()
            .into_iter()
            .filter(|name| !name.starts_with(DECODE_THREAD_PREFIX))
            .count()
    };
    let caller_after_start = caller_decodes();

    for frame in (120..=177).rev() {
        assert_eq!(pts(decoder.frame_at(rt(frame))), Some(frame));
    }
    assert_eq!(
        decoder.behind(),
        60,
        "all 60 frames of 60..=119 remain held"
    );
    assert_eq!(counts.seeks(), 3);

    // Crossing the GOP boundary 120 -> 119 drops Behind from 60 to 59 frames
    // and must immediately schedule Op::BackFill for 0..=59.
    assert_eq!(pts(decoder.frame_at(rt(119))), Some(119));
    decoder.idle();
    assert_eq!(
        decoder.behind(),
        119,
        "59 frames of 60..=118 plus all 60 pre-walked frames of 0..=59 must be held after stepping to 119"
    );
    assert_eq!(
        counts.seeks(),
        4,
        "fourth seek pre-walked 0..=59 on the pool"
    );

    for frame in (0..119).rev() {
        assert_eq!(pts(decoder.frame_at(rt(frame))), Some(frame));
    }
    decoder.idle();
    assert_eq!(counts.seeks(), 4, "each 60-frame GOP was walked once");
    assert_eq!(
        caller_decodes(),
        caller_after_start,
        "every frame after the initial reverse step was served from Behind without caller-thread decodes"
    );
}

/// WHY: Closes two coupled reverse-playback defect classes:
/// 1. `State::fills_toward` returned `false` while `parked`, so a reverse
///    `request` or `frame_at` for `behind_floor - 1` while `Op::BackFill` was
///    in flight canceled or superseded the active `Op::BackFill`.
/// 2. `Op::BackFill` had no cancellation check in `seek_walk` and `Behind`
///    dropped partial/stale walks without recycling planes to `plane_pool`,
///    so repositioning during a 300-frame GOP walk decoded the full GOP before
///    yielding the decoder and leaked the walk's plane buffers.
/// Does not cover codec-internal packet queue flushing inside GStreamer.
#[test]
fn reverse_backfill_in_flight_serves_boundary_step_and_cancels_on_reposition() {
    use std::time::Duration;

    // Part 1: fills_toward while parked and boundary step during in-flight BackFill.
    let (mock, _) = Mock::new(40, 10);
    let (gated, entered_rx, resume_tx) = GateMock::new(mock, 22);
    let mut decoder = AheadDecoder::new(Box::new(gated));

    assert_eq!(pts(decoder.frame_at(rt(35))), Some(35));
    assert_eq!(pts(decoder.frame_at(rt(34))), Some(34));
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Op::BackFill for 20..=29 must enter next_frame(22) on the pool");

    for frame in [33, 32, 31] {
        assert_eq!(pts(decoder.frame_at(rt(frame))), Some(frame));
    }
    {
        let state = decoder.shared.lock();
        assert!(state.parked);
        assert_eq!(state.behind_floor, Some(rt(30)));
        for (target, ask, expected) in [
            (29, Ask::Exact, true),
            (28, Ask::Exact, true),
            (27, Ask::Exact, false),
            (30, Ask::Exact, false),
            (31, Ask::Exact, false),
            (29, Ask::Nearest, false),
            (28, Ask::Nearest, false),
        ] {
            assert_eq!(
                state.fills_toward(
                    Want {
                        target: rt(target),
                        ask
                    },
                    R30
                ),
                expected,
                "fills_toward({target}, {ask:?})"
            );
        }
    }

    // request(29, false) and request(28, false) must recognize the active
    // BackFill and leave state.want as None.
    decoder.request(rt(29), false);
    decoder.request(rt(28), false);
    {
        let state = decoder.shared.lock();
        assert!(
            state.want.is_none(),
            "request within two steps before behind_floor must not queue a competing want"
        );
        assert!(!state.stop);
    }

    let ungate = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15));
        let _ = resume_tx.send(());
    });
    assert_eq!(pts(decoder.frame_at(rt(29))), Some(29));
    ungate.join().unwrap();
    assert!(
        decoder.behind() >= 9,
        "frames 20..=28 from the in-flight BackFill must remain in Behind after taking 29"
    );

    // Part 2: Every reposition mode cancels an in-flight 300-frame BackFill
    // within a few frames and recycles the partial Behind planes.
    #[derive(Clone, Copy, Debug)]
    enum CancelOp {
        Release,
        Seek,
        ForwardJump,
        ScrubNearest,
    }

    const PLANE_LEN: usize = 73_728;
    for op in [
        CancelOp::Release,
        CancelOp::Seek,
        CancelOp::ForwardJump,
        CancelOp::ScrubNearest,
    ] {
        while crate::take_plane_vec(PLANE_LEN).iter().any(|&b| b != 0) {}

        let (mock, counts) = Mock::new(600, 300);
        let (gated, entered_rx, resume_tx) = GateMock::new(mock.with_planes(PLANE_LEN), 5);
        let mut decoder = AheadDecoder::new(Box::new(gated));

        assert_eq!(pts(decoder.frame_at(rt(305))), Some(305));
        assert_eq!(pts(decoder.frame_at(rt(304))), Some(304));
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Op::BackFill for 0..=299 must enter next_frame(5) on the pool");
        let decoded_at_pause = counts.decoded();

        match op {
            CancelOp::Release => {
                decoder.release();
                let _ = resume_tx.send(());
                decoder.idle();
                assert!(
                    counts.decoded() <= decoded_at_pause + 1,
                    "{op:?}: BackFill must stop immediately after release, decoded {} vs {decoded_at_pause}",
                    counts.decoded()
                );
            }
            CancelOp::Seek => {
                let ungate = std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(15));
                    let _ = resume_tx.send(());
                });
                decoder.seek(rt(590)).unwrap();
                ungate.join().unwrap();
                decoder.idle();
                assert!(
                    counts.decoded() <= decoded_at_pause + AHEAD_FRAMES + 5,
                    "{op:?}: BackFill must abort without walking the 300-frame GOP, decoded {} vs {decoded_at_pause}",
                    counts.decoded()
                );
            }
            CancelOp::ForwardJump => {
                let ungate = std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(15));
                    let _ = resume_tx.send(());
                });
                assert_eq!(pts(decoder.frame_at(rt(310))), Some(310));
                ungate.join().unwrap();
                decoder.idle();
                assert!(
                    counts.decoded() <= decoded_at_pause + 11 + AHEAD_FRAMES + 5,
                    "{op:?}: BackFill must abort on forward jump without walking 300 frames, decoded {} vs {decoded_at_pause}",
                    counts.decoded()
                );
            }
            CancelOp::ScrubNearest => {
                let ungate = std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(15));
                    let _ = resume_tx.send(());
                });
                assert_eq!(pts(decoder.frame_at_nearest(rt(310))), Some(300));
                ungate.join().unwrap();
                decoder.idle();
                assert!(
                    counts.decoded() < decoded_at_pause + 20,
                    "{op:?}: BackFill must abort on nearest scrub without walking 300 frames, decoded {} vs {decoded_at_pause}",
                    counts.decoded()
                );
            }
        }
        decoder.release();
        let mut saw_backfill_plane = false;
        for _ in 0..64 {
            let buf = crate::take_plane_vec(PLANE_LEN);
            if buf.iter().all(|&b| b == 0) {
                break;
            }
            if (0..=20).any(|pts_val| buf[0] == super::tests::plane_fill(pts_val)) {
                saw_backfill_plane = true;
                break;
            }
        }
        assert!(
            saw_backfill_plane,
            "{op:?}: planes from frames 0..=5 of the canceled BackFill must return to plane_pool"
        );
    }

    // Part 3: A stale or errored BackFill outcome dropped in settle_fill
    // recycles its Behind planes via Drop.
    while crate::take_plane_vec(PLANE_LEN).iter().any(|&b| b != 0) {}
    let (mock, _) = Mock::new(20, 10);
    let mut state = State::new(Box::new(mock));
    let mut stale_behind = Behind::default();
    let mut stale_frame = cpu_frame(77, 0);
    if let aria_core::FrameData::Cpu(img) = &mut stale_frame.data {
        img.planes[0].data = vec![super::tests::plane_fill(77); PLANE_LEN];
    }
    stale_behind.push(stale_frame);
    state.settle_fill(
        Op::BackFill {
            target: rt(9),
            floor: rt(10),
        },
        state::Outcome {
            result: Ok(None),
            behind: stale_behind,
        },
    );
    let recycled = crate::take_plane_vec(PLANE_LEN);
    assert_eq!(
        recycled[0],
        super::tests::plane_fill(77),
        "stale BackFill outcome dropped by settle_fill must recycle its Behind planes"
    );
}

/// WHY: Closes the defect class where a caller asking for the frame the pool
/// is walking back to stops that walk: `claim` read the in-flight request as
/// an idle fill, set `stop`, and the canceled walk answered `None`. The
/// renderer then showed the GOP's sync frame, so reverse playback flashed the
/// keyframe on every frame look-ahead had requested. Verifies the gated case
/// (the caller arrives mid-walk), a reverse sweep that requests each next
/// frame before asking for it, as the renderer does, and that a finished
/// request leaves nothing in flight. Does not cover a caller arriving for a
/// different frame, which still stops the walk.
#[test]
fn a_caller_waiting_for_the_walk_in_flight_gets_its_frame() {
    use std::time::Duration;

    let (mock, _) = Mock::new(40, 10);
    let (gated, entered_rx, resume_tx) = GateMock::new(mock, 22);
    let mut decoder = AheadDecoder::new(Box::new(gated));
    assert_eq!(pts(decoder.frame_at(rt(25))), Some(25));
    decoder.request(rt(23), false);
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the walk back to 23 must reach frame 22 on the pool");
    decoder.request(rt(23), false);
    assert!(
        decoder.shared.lock().want.is_none(),
        "a repeat request for the frame in flight queues nothing"
    );
    let ungate = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = resume_tx.send(());
    });
    assert_eq!(
        pts(decoder.frame_at(rt(23))),
        Some(23),
        "the caller's own request finishes instead of answering None"
    );
    ungate.join().unwrap();

    let (mock, _) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    for frame in (0..40).rev() {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            Some(frame),
            "frame {frame}"
        );
        if frame > 0 {
            decoder.request(rt(frame - 1), false);
        }
    }

    // A walk into the first GOP leaves no earlier GOP to pre-walk, so the pool
    // job ends with the request: nothing stays in flight to swallow a later
    // request for the same frame.
    let (mock, _) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(5))), Some(5));
    decoder.request(rt(3), false);
    decoder.idle();
    assert!(
        decoder.shared.lock().serving.is_none(),
        "a finished request is no longer in flight"
    );
    assert_eq!(pts(decoder.frame_at(rt(3))), Some(3));
}
