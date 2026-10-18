//! Seek planning and keyframe selection tests.

use super::*;
use aria_core::{
    ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, Rect, Rotation, SourceInfo,
};

const R30: Rational = Rational::FPS_30;
/// The old fixed window, for tests exercising the exact boundary math.
const ONE_SEC_US: i64 = 1_000_000;

fn rt(value: i64, rate: Rational) -> RationalTime {
    RationalTime::new(value, rate)
}

#[test]
fn no_last_pts_never_rolls() {
    assert!(!should_roll_forward(None, rt(5, R30), R30, ONE_SEC_US));
}

#[test]
fn backward_and_repeated_targets_never_roll() {
    let last = Some(rt(10, R30));
    assert!(!should_roll_forward(last, rt(9, R30), R30, ONE_SEC_US));
    assert!(!should_roll_forward(last, rt(10, R30), R30, ONE_SEC_US));
}

#[test]
fn rolls_within_the_window() {
    let last = Some(rt(10, R30));
    // One frame ahead — the sequential-playback case.
    assert!(should_roll_forward(last, rt(11, R30), R30, ONE_SEC_US));
    // Exactly one second ahead is still inside the (inclusive) window.
    assert!(should_roll_forward(last, rt(40, R30), R30, ONE_SEC_US));
    // Just past the window falls back to a seek.
    assert!(!should_roll_forward(last, rt(41, R30), R30, ONE_SEC_US));
}

#[test]
fn window_comparison_is_exact_across_rates() {
    // Last frame on an NTSC stream time base, target at plain 30 fps.
    let last = Some(rt(0, Rational::FPS_29_97));
    // 29/30 s ahead: inside the window.
    assert!(should_roll_forward(
        last,
        rt(29, R30),
        Rational::FPS_29_97,
        ONE_SEC_US
    ));
    // 31/30 s ahead: outside.
    assert!(!should_roll_forward(
        last,
        rt(31, R30),
        Rational::FPS_29_97,
        ONE_SEC_US
    ));
    // Stream-tick time bases (large numerators) stay exact too.
    let last_ticks = Some(rt(90_000, Rational::new(90_000, 1))); // t = 1 s
    assert!(should_roll_forward(
        last_ticks,
        rt(60, R30),
        R30,
        ONE_SEC_US
    )); // 2 s
    assert!(!should_roll_forward(
        last_ticks,
        rt(61, R30),
        R30,
        ONE_SEC_US
    )); // 2 s + 1 frame
}

#[test]
fn invalid_rates_never_roll() {
    let last = Some(rt(10, Rational::new(0, 1)));
    assert!(!should_roll_forward(last, rt(11, R30), R30, ONE_SEC_US));
    assert!(!should_roll_forward(
        Some(rt(10, R30)),
        rt(11, Rational::new(30, 0)),
        R30,
        ONE_SEC_US
    ));
}

/// The Media Foundation shape: PTS on a fine integer tick base (100-ns)
/// that *truncates* the rational frame times of an NTSC stream.
#[test]
fn truncated_tick_pts_counts_as_its_own_frame() {
    const HNS: Rational = Rational::new(10_000_000, 1);
    const NTSC: Rational = Rational::FPS_29_97;
    // Frame 1 at 30000/1001 fps is 333_666.6̅ hns; MF delivers 333_666.
    let pts = rt(333_666, HNS);
    let target = rt(1, NTSC);
    // The truncated frame satisfies its own (slightly later) target...
    assert!(frame_covers(pts, target, NTSC));
    // ...but not the next frame's target.
    assert!(!frame_covers(pts, rt(2, NTSC), NTSC));
    // Requesting the frame just emitted (within a tick) is not a roll
    // (the decoder is already past it)...
    assert!(!should_roll_forward(Some(pts), target, NTSC, ONE_SEC_US));
    // ...while the *next* frame rolls instead of seeking.
    assert!(should_roll_forward(
        Some(pts),
        rt(2, NTSC),
        NTSC,
        ONE_SEC_US
    ));
}

/// On a frame-granular tick base (PTS ticks == frames), the slack must
/// never swallow a whole frame: exact behavior is preserved.
#[test]
fn slack_never_absorbs_a_real_frame_on_coarse_tick_bases() {
    assert!(!frame_covers(rt(9, R30), rt(10, R30), R30));
    assert!(frame_covers(rt(10, R30), rt(10, R30), R30));
    // One frame ahead still rolls (the slack is capped below one frame).
    assert!(should_roll_forward(
        Some(rt(10, R30)),
        rt(11, R30),
        R30,
        ONE_SEC_US
    ));
}

/// A decoder over `count` synthetic 30 fps frames with a keyframe only at
/// zero, mirroring how the real backends wire [`frame_at_rolling`]:
/// `next_frame` records `last_pts`, `seek` clears it and counts.
struct MockDecoder {
    info: SourceInfo,
    cursor: i64,
    count: i64,
    last_pts: Option<RationalTime>,
    stats: SeekStats,
    seeks: usize,
}

impl MockDecoder {
    fn new(count: i64) -> Self {
        Self {
            info: SourceInfo {
                coded_size: (2, 2),
                display_size: (2, 2),
                rotation: Rotation::None,
                pixel_format: PixelFormat::Nv12,
                color: ColorSpace::BT709,
                frame_rate: R30,
                time_base: R30,
                duration: Some(rt(count, R30)),
            },
            cursor: 0,
            count,
            last_pts: None,
            stats: SeekStats::default(),
            seeks: 0,
        }
    }
}

impl VideoDecoder for MockDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, _target: RationalTime) -> Result<(), DecodeError> {
        self.cursor = 0; // only keyframe is frame 0
        self.last_pts = None;
        self.seeks += 1;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.cursor >= self.count {
            return Ok(None);
        }
        let pts = rt(self.cursor, R30);
        self.cursor += 1;
        self.last_pts = Some(pts);
        let y = Plane::new(vec![16u8; 4], 2, 2);
        let uv = Plane::new(vec![128u8; 2], 2, 1);
        Ok(Some(VideoFrame::new(
            pts,
            PixelFormat::Nv12,
            ColorSpace::BT709,
            (2, 2),
            Rect::from_size(2, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![y, uv])),
        )))
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

#[test]
fn sequential_targets_seek_once_and_land_exactly() {
    let mut dec = MockDecoder::new(60);
    for i in (0..30).step_by(3) {
        let f = dec.frame_at(rt(i, R30)).unwrap().unwrap();
        assert_eq!(f.pts.value, i);
    }
    assert_eq!(dec.seeks, 1, "only the first (cold) target may seek");
}

#[test]
fn backward_target_falls_back_to_seek() {
    let mut dec = MockDecoder::new(60);
    dec.frame_at(rt(20, R30)).unwrap().unwrap();
    let f = dec.frame_at(rt(5, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 5);
    assert_eq!(dec.seeks, 2);
}

#[test]
fn far_forward_target_falls_back_to_seek() {
    let mut dec = MockDecoder::new(120);
    dec.frame_at(rt(0, R30)).unwrap().unwrap();
    // 40 frames ahead: beyond the window — the cold-start break-even is
    // 30 frames, and the first (instant) mock seek only shrank it.
    let f = dec.frame_at(rt(40, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 40);
    assert_eq!(dec.seeks, 2);
}

#[test]
fn rolling_past_end_of_stream_returns_none() {
    let mut dec = MockDecoder::new(10);
    dec.frame_at(rt(9, R30)).unwrap().unwrap();
    // Within the roll window but past the last frame.
    assert!(dec.frame_at(rt(12, R30)).unwrap().is_none());
    assert_eq!(dec.seeks, 1, "the roll path must not seek");
}

/// A far target snaps: one seek, one decode, and the frame is the sync
/// point at/before the target (frame 0 here — the mock's only keyframe),
/// not the exact target. That single decode replacing the GOP-prefix
/// walk is the whole point of the API.
#[test]
fn nearest_far_target_snaps_to_sync_frame_with_one_decode() {
    let mut dec = MockDecoder::new(120);
    let f = dec.frame_at_nearest(rt(100, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 0, "snap lands on the keyframe, not target");
    assert_eq!(dec.seeks, 1);
    assert_eq!(dec.cursor, 1, "exactly one frame decoded");
}

/// A target just ahead of the last emitted frame stays on the exact
/// roll path: no seek, and the frame is the exact target.
#[test]
fn nearest_target_just_ahead_rolls_exactly() {
    let mut dec = MockDecoder::new(60);
    dec.frame_at(rt(10, R30)).unwrap().unwrap();
    let f = dec.frame_at_nearest(rt(12, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 12, "roll-window targets land exactly");
    assert_eq!(dec.seeks, 1, "the nearest roll path must not seek");
}

/// A nearest jump past the snap break-even snaps even while the exact roll
/// window spans it: a long exact window (expensive GOP walks) says nothing
/// about a snap, which costs one decode. Rolling 45 frames here is the
/// defect: 45 decodes where one seek and one decode answer the call.
#[test]
fn nearest_jump_snaps_inside_a_wide_exact_window() {
    let mut dec = MockDecoder::new(120);
    dec.frame_at(rt(10, R30)).unwrap().unwrap();
    let mut grown = SeekStats::default();
    for _ in 0..20 {
        grown.observe_seek(300.0);
        grown.observe_roll_frame(1.0);
    }
    assert!(
        grown.roll_window_us(R30) > 1_500_000,
        "exact window spans it"
    );
    dec.stats = grown;
    let f = dec.frame_at_nearest(rt(55, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 0, "the jump snaps to the keyframe");
    assert_eq!(dec.seeks, 2, "the jump seeks once");
    assert_eq!(dec.cursor, 1, "the snap decodes one frame");
}

/// A snap feeds the nearest window once a frame cost is measured: after one
/// near-free snap against 10 ms frames, a 5-frame nearest jump snaps again
/// instead of rolling inside the 8-frame prior.
#[test]
fn an_observed_snap_narrows_the_nearest_window() {
    let mut dec = MockDecoder::new(120);
    dec.stats.observe_roll_frame(10.0);
    let first = dec.frame_at_nearest(rt(100, R30)).unwrap().unwrap();
    assert_eq!(first.pts.value, 0);
    let f = dec.frame_at_nearest(rt(5, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 0, "the second jump snaps too");
    assert_eq!(dec.seeks, 2);
}

/// All-keyframe mock whose `seek` lands exactly on the target frame —
/// including *past* EOF, where `next_frame` then yields nothing. That's
/// the AVFoundation shape for a seek beyond the last sample, which the
/// tail-recovery path must step back from.
struct TailMockDecoder {
    inner: MockDecoder,
}

impl TailMockDecoder {
    fn new(count: i64) -> Self {
        Self {
            inner: MockDecoder::new(count),
        }
    }
}

impl VideoDecoder for TailMockDecoder {
    fn info(&self) -> &SourceInfo {
        &self.inner.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        self.inner.cursor = resample(target, R30).value.max(0);
        self.inner.last_pts = None;
        self.inner.seeks += 1;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        self.inner.next_frame()
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last = self.inner.last_pts;
        let mut stats = self.inner.stats;
        let result = frame_at_rolling(self, last, &mut stats, target);
        self.inner.stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last = self.inner.last_pts;
        let mut stats = self.inner.stats;
        let result = frame_at_nearest_rolling(self, last, &mut stats, target);
        self.inner.stats = stats;
        result
    }
}

/// The over-report-by-one case: a stream of frames 0..29 asked for
/// frame 30 (what a pool that trusts the container's count requests at
/// the media tail) must hand back the last real frame, not miss.
#[test]
fn nearest_past_eof_target_steps_back_to_tail_frame() {
    let mut dec = TailMockDecoder::new(30);
    let f = dec.frame_at_nearest(rt(30, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 29, "past-EOF nearest lands on the tail");
}

/// Rolling into EOF (target within the roll window but past the last
/// frame) recovers the tail frame too, not just the cold-seek path.
#[test]
fn nearest_roll_into_eof_recovers_tail_frame() {
    let mut dec = TailMockDecoder::new(10);
    dec.frame_at(rt(9, R30)).unwrap().unwrap();
    let f = dec.frame_at_nearest(rt(11, R30)).unwrap().unwrap();
    assert_eq!(f.pts.value, 9);
}

/// A target nowhere near the tail (beyond the bounded back-steps) is an
/// honest miss, and an empty stream can't loop.
#[test]
fn nearest_far_past_eof_and_empty_streams_still_miss() {
    let mut dec = TailMockDecoder::new(30);
    assert!(
        dec.frame_at_nearest(rt(30 + MAX_EOF_BACKSTEPS + 1, R30))
            .unwrap()
            .is_none()
    );
    let mut empty = TailMockDecoder::new(0);
    assert!(empty.frame_at_nearest(rt(5, R30)).unwrap().is_none());
}
/// Synthetic non-advancing decoder whose timestamps never increase (corrupt stream simulation).
struct NonAdvancingDecoder {
    info: SourceInfo,
    last_pts: Option<RationalTime>,
    stats: SeekStats,
    frames_yielded: u32,
}

impl NonAdvancingDecoder {
    fn new() -> Self {
        Self {
            info: SourceInfo {
                coded_size: (2, 2),
                display_size: (2, 2),
                rotation: Rotation::None,
                frame_rate: R30,
                time_base: Rational::new(30, 1),
                color: ColorSpace::BT709,
                pixel_format: PixelFormat::Nv12,
                duration: None,
            },
            last_pts: None,
            stats: SeekStats::default(),
            frames_yielded: 0,
        }
    }
}

impl VideoDecoder for NonAdvancingDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, _target: RationalTime) -> Result<(), DecodeError> {
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        self.frames_yielded += 1;
        // PTS is forever stuck at frame 0, never covering target > 0
        let pts = rt(0, R30);
        self.last_pts = Some(pts);
        let y = Plane::new(vec![16u8; 4], 2, 2);
        let uv = Plane::new(vec![128u8; 2], 2, 1);
        Ok(Some(VideoFrame::new(
            pts,
            PixelFormat::Nv12,
            ColorSpace::BT709,
            (2, 2),
            Rect::from_size(2, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![y, uv])),
        )))
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

#[test]
fn non_advancing_corrupt_stream_terminates_promptly() {
    let mut dec = NonAdvancingDecoder::new();
    // Asking for a future frame on a decoder whose PTS never advances must not hang in an infinite loop
    let res = dec.frame_at(rt(10, R30)).unwrap();
    assert!(
        res.is_none(),
        "non-advancing stream cannot cover future target"
    );
    assert!(dec.frames_yielded > 0);
    assert!(dec.frames_yielded <= 650, "walk must be strictly bounded");
}

/// A stream with a sync sample every `gop` frames whose `open` sync samples
/// cannot start a decode: a seek onto one emits nothing until the next sync
/// sample, the way a decoder skips the corrupt frames of an open GOP. Decoding
/// through an open sync sample from an earlier one is exact. `first` is the
/// first frame's pts, for a stream that starts after zero.
struct OpenGopDecoder {
    info: SourceInfo,
    gop: i64,
    open: Vec<i64>,
    first: i64,
    cursor: i64,
    count: i64,
    last_pts: Option<RationalTime>,
    seeks: Vec<i64>,
}

impl OpenGopDecoder {
    fn new(count: i64, gop: i64, open: &[i64]) -> Self {
        let mut info = MockDecoder::new(count).info;
        info.frame_rate = R30;
        Self {
            info,
            gop,
            open: open.to_vec(),
            first: 0,
            cursor: 0,
            count,
            last_pts: None,
            seeks: Vec::new(),
        }
    }
}

impl VideoDecoder for OpenGopDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let target = resample(target, R30).value.max(0);
        self.seeks.push(target);
        let mut sync = target - target % self.gop;
        while self.open.contains(&sync) {
            sync += self.gop;
        }
        self.cursor = sync.max(self.first);
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let mut frame = MockDecoder::new(self.count);
        frame.cursor = self.cursor.max(self.first);
        let out = frame.next_frame()?;
        self.cursor = frame.cursor;
        self.last_pts = out.as_ref().map(|frame| frame.pts);
        Ok(out)
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let mut stats = SeekStats::default();
        frame_at_rolling(self, self.last_pts, &mut stats, target)
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let mut stats = SeekStats::default();
        frame_at_nearest_rolling(self, self.last_pts, &mut stats, target)
    }
}

/// A seek that lands past its target backs off to an earlier sync sample and
/// walks forward to the exact frame. Closes the class "an open-GOP seek point
/// returns a later frame for a cold or backward request": before the back-off
/// the first frame at or after the target, the next sync sample, was returned.
/// Does not cover a decoder that emits corrupt frames instead of skipping
/// them; `GstDecoder::next_frame` skips those after a seek.
#[test]
fn a_seek_that_lands_past_its_target_backs_off_to_the_exact_frame() {
    // Sync samples every 60 frames; 120 and 240 cannot start a decode.
    for (target, seeks) in [
        (130, vec![130, 100]),
        (179, vec![179, 149, 119]),
        (120, vec![120, 90]),
        // Lands at the end of the stream: the last GOP is open.
        (250, vec![250, 220]),
        (61, vec![61]),
    ] {
        let mut dec = OpenGopDecoder::new(300, 60, &[120, 240]);
        let frame = dec
            .frame_at(rt(target, R30))
            .unwrap()
            .unwrap_or_else(|| panic!("target {target}: no frame"));
        assert_eq!(frame.pts.value, target, "target {target}");
        assert_eq!(dec.seeks, seeks, "target {target}: seek points");
    }
}

/// Back-off ends: at the start of the stream it accepts the first frame, and
/// a stream where no sync sample before the target can start a decode stops
/// after the longest back-off rather than seeking forever.
#[test]
fn the_back_off_stops_at_the_stream_start_and_at_its_longest_step() {
    let mut late = OpenGopDecoder::new(90, 30, &[]);
    late.first = 2;
    let frame = late.frame_at(rt(0, R30)).unwrap().unwrap();
    assert_eq!(frame.pts.value, 2, "the stream's first frame");
    assert_eq!(late.seeks, vec![0], "nothing earlier to seek to");

    let every: Vec<i64> = (1..200).map(|n| n * 30).collect();
    let mut hopeless = OpenGopDecoder::new(7000, 30, &every);
    let frame = hopeless.frame_at(rt(5000, R30)).unwrap().unwrap();
    // 1, 2, 4, ... 64 seconds back: seven back-offs after the first seek.
    assert_eq!(hopeless.seeks.len(), 8, "{:?}", hopeless.seeks);
    assert_eq!(*hopeless.seeks.last().unwrap(), 5000 - 64 * 30);
    assert!(frame.pts.value > 5000, "nothing exact exists to return");
}
