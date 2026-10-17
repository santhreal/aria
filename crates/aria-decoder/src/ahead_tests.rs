//! Decode-ahead contracts: an [`AheadDecoder`] returns the frames its wrapped
//! decoder returns, decodes each frame once, holds at most [`AHEAD_FRAMES`]
//! ahead, walks each GOP once when stepping backward, does its pool work on
//! the decode threads, and ends instead of hanging when the wrapped decoder
//! fails, panics, or is dropped mid-fill.
//!
//! Not covered: hardware decoders (the mock stands in for every backend) and
//! the pool's thread count.

use std::sync::atomic::{AtomicUsize, Ordering as Atomic};
use std::time::{Duration, Instant};

use aria_core::{
    ColorSpace, CpuImage, GpuSurface, GpuSurfaceKind, PixelFormat, Plane, Rect, Rotation,
};

use super::*;
use crate::seek::{SeekStats, frame_at_nearest_rolling, frame_at_rolling};

const R30: Rational = Rational::FPS_30;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R30)
}

/// What a [`Mock`] did, readable after the mock moved into an
/// [`AheadDecoder`].
#[derive(Default)]
struct Counts {
    decoded: AtomicUsize,
    seeks: AtomicUsize,
    dropped: AtomicUsize,
    threads: Mutex<Vec<String>>,
}

impl Counts {
    fn decoded(&self) -> usize {
        self.decoded.load(Atomic::SeqCst)
    }

    fn seeks(&self) -> usize {
        self.seeks.load(Atomic::SeqCst)
    }

    fn dropped(&self) -> usize {
        self.dropped.load(Atomic::SeqCst)
    }

    fn threads(&self) -> Vec<String> {
        self.threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Panic,
    Error,
}

/// `count` frames at 30 fps with a keyframe every `gop` frames, wired to the
/// shared roll-forward policy like the real backends.
struct Mock {
    info: SourceInfo,
    cursor: i64,
    count: i64,
    gop: i64,
    last_pts: Option<RationalTime>,
    stats: SeekStats,
    counts: Arc<Counts>,
    fault: Option<(i64, Fault)>,
    /// Sync samples that cannot start a decode (open GOPs): a seek to one
    /// lands on the next sync sample instead, past its target.
    open: Vec<i64>,
}

impl Mock {
    fn new(count: i64, gop: i64) -> (Self, Arc<Counts>) {
        let counts = Arc::new(Counts::default());
        let mock = Self {
            info: SourceInfo {
                coded_size: (2, 2),
                display_size: (2, 2),
                rotation: Rotation::None,
                pixel_format: PixelFormat::Nv12,
                color: ColorSpace::BT709,
                frame_rate: R30,
                time_base: R30,
                duration: Some(rt(count)),
            },
            cursor: 0,
            count,
            gop,
            last_pts: None,
            stats: SeekStats::default(),
            counts: Arc::clone(&counts),
            fault: None,
            open: Vec::new(),
        };
        (mock, counts)
    }

    fn with_open_gops(mut self, open: &[i64]) -> Self {
        self.open = open.to_vec();
        self
    }

    fn failing_at(mut self, frame: i64, fault: Fault) -> Self {
        self.fault = Some((frame, fault));
        self
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.counts.dropped.fetch_add(1, Atomic::SeqCst);
    }
}

fn cpu_frame(pts: i64, bytes: usize) -> VideoFrame {
    VideoFrame::new(
        rt(pts),
        PixelFormat::Nv12,
        ColorSpace::BT709,
        (2, 2),
        Rect::from_size(2, 2),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![Plane::new(vec![0u8; bytes], 2, 2)])),
    )
}

fn gpu_frame(pts: i64) -> VideoFrame {
    VideoFrame::new(
        rt(pts),
        PixelFormat::Nv12,
        ColorSpace::BT709,
        (2, 2),
        Rect::from_size(2, 2),
        Rotation::None,
        FrameData::Gpu(GpuSurface::new(GpuSurfaceKind::DmaBuf, 0, None)),
    )
}

impl VideoDecoder for Mock {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let frame = target.value.clamp(0, self.count);
        let mut sync = frame - frame % self.gop;
        while self.open.contains(&sync) {
            sync += self.gop;
        }
        self.cursor = sync;
        self.last_pts = None;
        self.counts.seeks.fetch_add(1, Atomic::SeqCst);
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.cursor >= self.count {
            return Ok(None);
        }
        match self.fault {
            Some((at, Fault::Panic)) if at == self.cursor => panic!("mock decoder fault"),
            Some((at, Fault::Error)) if at == self.cursor => {
                return Err(DecodeError::Decode(format!("mock fault at frame {at}")));
            }
            Some(_) | None => {}
        }
        let name = std::thread::current().name().unwrap_or("").to_owned();
        self.counts
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name);
        let pts = rt(self.cursor);
        self.cursor += 1;
        self.last_pts = Some(pts);
        self.counts.decoded.fetch_add(1, Atomic::SeqCst);
        Ok(Some(cpu_frame(pts.value, 6)))
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last = self.last_pts;
        let mut stats = self.stats;
        let result = frame_at_rolling(self, last, &mut stats, target);
        self.stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last = self.last_pts;
        let mut stats = self.stats;
        let result = frame_at_nearest_rolling(self, last, &mut stats, target);
        self.stats = stats;
        result
    }
}

fn ahead(mock: Mock) -> AheadDecoder {
    AheadDecoder::new(Box::new(mock))
}

fn pts(result: Result<Option<VideoFrame>, DecodeError>) -> Option<i64> {
    result
        .unwrap_or_else(|error| panic!("decode failed: {error}"))
        .map(|frame| frame.pts.value)
}

/// Wait for `done`, failing after five seconds: what a stalled decoder looks
/// like in this suite.
fn within_deadline(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "{what} did not happen within 5 s"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn sequential_frames_match_the_wrapped_decoder_and_decode_once() {
    let (bare, _) = Mock::new(60, 10);
    let mut bare = bare;
    let (mock, counts) = Mock::new(60, 10);
    let mut decoder = ahead(mock);
    for frame in 0..60 {
        assert_eq!(
            pts(decoder.frame_at(rt(frame))),
            pts(bare.frame_at(rt(frame))),
            "frame {frame}"
        );
    }
    assert_eq!(pts(decoder.frame_at(rt(60))), None, "past the end");
    decoder.idle();
    assert_eq!(counts.decoded(), 60, "every frame decoded once");
    assert_eq!(counts.seeks(), 1, "only the cold first frame seeks");
}

#[test]
fn a_fill_stops_at_the_ahead_bound() {
    let (mock, counts) = Mock::new(100, 10);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    decoder.idle();
    assert_eq!(decoder.ahead(), AHEAD_FRAMES);
    assert_eq!(counts.decoded(), 1 + AHEAD_FRAMES);
    // Taking one frame refills one.
    assert_eq!(pts(decoder.frame_at(rt(1))), Some(1));
    decoder.idle();
    assert_eq!(decoder.ahead(), AHEAD_FRAMES);
    assert_eq!(counts.decoded(), 2 + AHEAD_FRAMES);
}

#[test]
fn ahead_frames_decode_on_the_pool() {
    let (mock, counts) = Mock::new(30, 10);
    let mut decoder = ahead(mock);
    decoder.request(rt(4), false);
    decoder.idle();
    let pool = counts.threads();
    assert_eq!(
        pool.len(),
        5 + AHEAD_FRAMES,
        "the request walk plus the fill"
    );
    assert!(
        pool.iter()
            .all(|name| name.starts_with(DECODE_THREAD_PREFIX)),
        "decoded off the pool: {pool:?}"
    );
    assert_eq!(pts(decoder.frame_at(rt(4))), Some(4));
    for frame in 5..5 + AHEAD_FRAMES as i64 {
        assert_eq!(pts(decoder.frame_at(rt(frame))), Some(frame));
    }
    decoder.idle();
    let all = counts.threads();
    assert!(
        all.iter()
            .all(|name| name.starts_with(DECODE_THREAD_PREFIX)),
        "a sequential frame decoded on the caller: {all:?}"
    );
}

#[test]
fn a_skip_forward_drops_the_frames_it_passes() {
    let (mock, counts) = Mock::new(100, 10);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    decoder.idle();
    // Frame 5 is held: the frames before it are passed, not decoded again.
    assert_eq!(pts(decoder.frame_at(rt(5))), Some(5));
    decoder.idle();
    assert_eq!(decoder.ahead(), AHEAD_FRAMES);
    assert_eq!(counts.decoded(), 6 + AHEAD_FRAMES);
    assert_eq!(counts.seeks(), 1);
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
fn nearest_matches_the_wrapped_decoder_and_holds_nothing_ahead() {
    let (bare, _) = Mock::new(60, 10);
    let mut bare = bare;
    let (mock, _) = Mock::new(60, 10);
    let mut decoder = ahead(mock);
    for target in [45, 12, 33, 7] {
        assert_eq!(
            pts(decoder.frame_at_nearest(rt(target))),
            pts(bare.frame_at_nearest(rt(target))),
            "target {target}"
        );
    }
    decoder.idle();
    assert_eq!(decoder.ahead(), 0);
}

#[test]
fn seek_and_next_frame_follow_the_wrapped_decoder() {
    let (bare, _) = Mock::new(40, 10);
    let mut bare = bare;
    let (mock, _) = Mock::new(40, 10);
    let mut decoder = ahead(mock);
    decoder
        .seek(rt(23))
        .unwrap_or_else(|error| panic!("{error}"));
    bare.seek(rt(23)).unwrap_or_else(|error| panic!("{error}"));
    let mut got = Vec::new();
    let mut want = Vec::new();
    while let Some(frame) = pts(decoder.next_frame()) {
        got.push(frame);
    }
    while let Some(frame) = pts(bare.next_frame()) {
        want.push(frame);
    }
    assert_eq!(got, want);
    assert_eq!(got.first(), Some(&20));
    // After a seek, a frame at or past its target comes from the held queue.
    decoder
        .seek(rt(10))
        .unwrap_or_else(|error| panic!("{error}"));
    decoder.idle();
    assert_eq!(decoder.ahead(), AHEAD_FRAMES);
    assert_eq!(pts(decoder.frame_at(rt(12))), Some(12));
}

#[test]
fn an_error_after_held_frames_arrives_after_them() {
    let (mock, _) = Mock::new(40, 10);
    let mut decoder = ahead(mock.failing_at(5, Fault::Error));
    let mut got = Vec::new();
    let error = loop {
        match decoder.next_frame() {
            Ok(Some(frame)) => got.push(frame.pts.value),
            Ok(None) => panic!("the stream ended before its fault"),
            Err(error) => break error,
        }
    };
    assert_eq!(got, [0, 1, 2, 3, 4]);
    assert!(
        error.to_string().contains("mock fault at frame 5"),
        "{error}"
    );
}

#[test]
fn a_panicking_decoder_fails_requests_instead_of_hanging() {
    let (mock, counts) = Mock::new(40, 10);
    let mut decoder = ahead(mock.failing_at(5, Fault::Panic));
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    decoder.idle();
    assert_eq!(counts.dropped(), 1, "the panicked decoder was dropped");
    for frame in 1..5 {
        assert_eq!(pts(decoder.frame_at(rt(frame))), Some(frame));
    }
    let error = decoder
        .frame_at(rt(5))
        .err()
        .unwrap_or_else(|| panic!("a broken decoder returned a frame"));
    assert!(error.to_string().contains("panicked"), "{error}");
    assert!(decoder.seek(rt(0)).is_err());
    decoder.request(rt(6), false);
    decoder.idle();
}

#[test]
fn dropping_mid_fill_releases_the_wrapped_decoder() {
    let (mock, counts) = Mock::new(10_000, 100);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    drop(decoder);
    within_deadline("dropping the wrapped decoder", || counts.dropped() == 1);
    let decoded = counts.decoded();
    assert!(
        decoded <= 1 + AHEAD_FRAMES,
        "decoded {decoded} after the drop"
    );
}

#[test]
fn release_drops_held_frames_and_the_next_request_refills() {
    let (mock, counts) = Mock::new(100, 10);
    let mut decoder = ahead(mock);
    assert_eq!(pts(decoder.frame_at(rt(0))), Some(0));
    decoder.idle();
    decoder.release();
    assert_eq!(decoder.ahead(), 0);
    decoder.idle();
    assert_eq!(
        counts.decoded(),
        1 + AHEAD_FRAMES,
        "a released decoder kept filling"
    );
    assert_eq!(pts(decoder.frame_at(rt(40))), Some(40));
    decoder.idle();
    assert_eq!(decoder.ahead(), AHEAD_FRAMES);
}

#[test]
fn open_ahead_opens_and_decodes_the_first_frame_on_the_pool() {
    let (mock, counts) = Mock::new(30, 10);
    let opening = open_ahead(
        move || Ok(Box::new(mock) as Box<dyn VideoDecoder>),
        rt(3),
        false,
    );
    let mut decoder = opening.wait().unwrap_or_else(|error| panic!("{error}"));
    decoder.idle();
    assert!(
        counts
            .threads()
            .iter()
            .all(|name| name.starts_with(DECODE_THREAD_PREFIX))
    );
    assert_eq!(counts.decoded(), 4 + AHEAD_FRAMES);
    assert_eq!(pts(decoder.frame_at(rt(3))), Some(3));
}

#[test]
fn open_ahead_reports_a_failed_or_panicking_open() {
    let failed = open_ahead(
        || Err(DecodeError::Open("no such file".into())),
        rt(0),
        false,
    );
    let error = failed.wait().err().unwrap_or_else(|| panic!("opened"));
    assert!(error.to_string().contains("no such file"), "{error}");
    let panicked = open_ahead(|| panic!("open fault"), rt(0), false);
    let error = panicked.wait().err().unwrap_or_else(|| panic!("opened"));
    assert!(error.to_string().contains("panicked"), "{error}");
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
