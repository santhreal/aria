//! WHY: closes the class of a frame whose container clock rounds its time
//! below its own rational target. Matroska and WebM store milliseconds, so
//! frame 71 of a 24 fps WebM (2958.3̅ ms) decodes at 2958 ms; under a one-tick
//! slack on the decoder's nanosecond clock that frame did not cover its own
//! target, `frame_at(i)` returned frame `i + 1`, and the last frame returned
//! nothing. Every [`NAMED_RATES`] rate is held against every container clock
//! the backends read (Matroska and FLV milliseconds, MPEG-TS 90 kHz, Media
//! Foundation 100 ns, MediaCodec microseconds), with each frame time rounded
//! down, to nearest, and up, at the predicates every walk goes through and
//! through [`frame_at_rolling`] on a decoder that emits those times.
//!
//! What this does not catch: a variable-rate stream whose frames sit closer
//! than half the stated frame period.

use super::*;
use aria_core::{
    ColorSpace, CpuImage, FrameData, NAMED_RATES, PixelFormat, Plane, Rect, Rotation, SourceInfo,
};

/// The rate GStreamer stamps buffers on.
const NS: Rational = Rational::new(1_000_000_000, 1);
/// Ticks a second of each container clock a backend reads.
const CLOCKS: [i64; 4] = [1_000, 90_000, 10_000_000, 1_000_000];
const ONE_SEC_US: i64 = 1_000_000;

#[derive(Clone, Copy, Debug)]
enum Round {
    Down,
    Nearest,
    Up,
}

const ROUNDS: [Round; 3] = [Round::Down, Round::Nearest, Round::Up];

/// Frame `frame` of `rate` as a container clock of `ticks` a second stores
/// it, read back in nanoseconds as GStreamer reports it.
fn stored(frame: i64, rate: Rational, ticks: i64, round: Round) -> RationalTime {
    let exact = i128::from(frame) * i128::from(rate.den) * i128::from(ticks);
    let per = i128::from(rate.num);
    let stamp = match round {
        Round::Down => exact.div_euclid(per),
        Round::Nearest => (2 * exact + per).div_euclid(2 * per),
        Round::Up => (exact + per - 1).div_euclid(per),
    };
    let ns = stamp * 1_000_000_000 / i128::from(ticks);
    RationalTime::new(i64::try_from(ns).expect("a short stream fits"), NS)
}

/// Frames in two seconds of `rate`, rounded up.
fn two_seconds(rate: Rational) -> i64 {
    let (num, den) = (i64::from(rate.num), i64::from(rate.den));
    (2 * num + den - 1) / den
}

#[test]
fn a_stored_frame_time_names_its_own_frame_on_every_clock() {
    for (name, rate) in NAMED_RATES {
        for ticks in CLOCKS {
            for round in ROUNDS {
                let case = format!("{name} fps, {ticks} ticks a second, {round:?}");
                for frame in 0..two_seconds(rate) {
                    let (at, next) = (
                        stored(frame, rate, ticks, round),
                        stored(frame + 1, rate, ticks, round),
                    );
                    let (target, after) = (
                        RationalTime::new(frame, rate),
                        RationalTime::new(frame + 1, rate),
                    );
                    assert!(
                        frame_covers(at, target, rate),
                        "{case}: frame {frame} at {at:?} misses its target"
                    );
                    assert!(
                        !frame_covers(at, after, rate),
                        "{case}: frame {frame} covers the next target"
                    );
                    assert!(
                        !should_roll_forward(Some(at), target, rate, ONE_SEC_US),
                        "{case}: frame {frame} rolls past itself"
                    );
                    assert!(
                        should_roll_forward(Some(at), after, rate, ONE_SEC_US),
                        "{case}: frame {frame} seeks for the next frame"
                    );
                    assert!(
                        is_after(next, at, rate),
                        "{case}: frame {} is not after frame {frame}",
                        frame + 1
                    );
                    assert!(
                        !is_after(target, at, rate),
                        "{case}: frame {frame}'s target is a later frame"
                    );
                }
            }
        }
    }
}

/// A decoder over `times`, a keyframe every `gop` frames, that seeks to the
/// last keyframe at or before its target as a key-unit seek does.
struct ClockDecoder {
    info: SourceInfo,
    times: Vec<RationalTime>,
    gop: usize,
    cursor: usize,
    last_pts: Option<RationalTime>,
    stats: SeekStats,
}

impl ClockDecoder {
    fn new(rate: Rational, times: Vec<RationalTime>, gop: usize) -> Self {
        Self {
            info: SourceInfo {
                coded_size: (2, 2),
                display_size: (2, 2),
                rotation: Rotation::None,
                pixel_format: PixelFormat::Nv12,
                color: ColorSpace::BT709,
                frame_rate: rate,
                time_base: NS,
                duration: None,
            },
            times,
            gop,
            cursor: 0,
            last_pts: None,
            stats: SeekStats::default(),
        }
    }
}

impl VideoDecoder for ClockDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let landed = self
            .times
            .iter()
            .rposition(|t| t.compare(target) != Ordering::Greater)
            .unwrap_or(0);
        self.cursor = landed - landed % self.gop;
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let Some(&pts) = self.times.get(self.cursor) else {
            return Ok(None);
        };
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

/// Every frame decodes at its own time, the last one included, walking
/// forward as playback does and backward as a scrub does.
#[test]
fn every_stored_frame_decodes_at_its_own_time() {
    for (name, rate) in NAMED_RATES {
        for ticks in CLOCKS {
            for round in ROUNDS {
                let case = format!("{name} fps, {ticks} ticks a second, {round:?}");
                let count = two_seconds(rate);
                let times: Vec<_> = (0..count).map(|f| stored(f, rate, ticks, round)).collect();
                let mut decoder = ClockDecoder::new(rate, times.clone(), 12);
                let forward = 0..count;
                for frame in forward.clone().chain(forward.rev()) {
                    let got = decoder
                        .frame_at(RationalTime::new(frame, rate))
                        .expect("the mock decodes")
                        .unwrap_or_else(|| panic!("{case}: frame {frame} decoded nothing"));
                    assert_eq!(got.pts, times[frame as usize], "{case}: frame {frame}");
                }
            }
        }
    }
}
