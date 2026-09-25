//! Mixer behaviour: span resolution, gain, pan, fades, and the streaming
//! contract the export loop relies on.

use super::*;
use crate::audio_dsp::SpanWarp;
use aria_core::AudioReader;
use aria_models::Rational;
use std::f32::consts::PI;

struct SineReader {
    pos: i64,
    rate: u32,
    freq: f32,
}

impl AudioReader for SineReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        for f in 0..frames {
            let t = (self.pos + f as i64) as f32 / self.rate as f32;
            let sample = (2.0 * PI * self.freq * t).sin();
            for ch in 0..CHANNELS {
                out[f * CHANNELS + ch] = sample;
            }
        }
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

#[test]
fn ticks_to_samples_is_exact_for_common_rates() {
    assert_eq!(ticks_to_samples(24, 24, 1), 48_000);
    assert_eq!(ticks_to_samples(1, 24, 1), 2_000);
    assert_eq!(ticks_to_samples(30_000, 30_000, 1_001), 1_001 * 48_000);
}

#[test]
fn sample_boundaries_partition_the_stream() {
    assert_eq!(sample_boundary(0, 24, 1), 0);
    assert_eq!(sample_boundary(1, 24, 1), 2_000);
    assert_eq!(sample_boundary(48, 24, 1), 96_000);
    let mut prev = 0;
    for n in 1..=100 {
        let b = sample_boundary(n, 30_000, 1_001);
        assert!(b > prev, "boundaries advance");
        prev = b;
    }
}

#[test]
fn silent_project_has_no_mixer() {
    let project = Project::new("test", Rational::FPS_24);
    assert!(ExportAudioMixer::for_project(&project).unwrap().is_none());
}

#[test]
fn flat_speed_warp_maps_endpoints() {
    let warp = SpanWarp::FlatSpeed { num: 2, den: 1 };
    assert!((warped_source_frame(&warp, 0, 2000) - 0.0).abs() < 1e-6);
    assert!((warped_source_frame(&warp, 2000, 2000) - 4000.0).abs() < 1e-6);
}

#[test]
fn curved_source_bounds_sample_only_interval_endpoints() {
    let mut span = Span {
        path: PathBuf::new(),
        start: 0,
        end: 48_000,
        source_start: 0,
        source_len: 96_000,
        reversed: false,
        warp: SpanWarp::Curved {
            curve: Param::Constant(2.0),
            source_len: 96_000,
            curve_total: 2.0,
        },
        preserve_pitch: false,
        denoise: false,
        volume: Param::Constant(1.0),
        pan: Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        reader: None,
        exhausted: false,
        bus: 0,
    };
    let (forward_min, forward_max, forward_evaluations) =
        monotonic_source_bounds(&span, 0, 48_000, 48_000);
    assert_eq!(forward_evaluations, 2);
    assert!(forward_min <= forward_max);

    span.reversed = true;
    let (reverse_min, reverse_max, reverse_evaluations) =
        monotonic_source_bounds(&span, 0, 48_000, 48_000);
    assert_eq!(reverse_evaluations, 2);
    assert!(reverse_min <= reverse_max);
}

#[test]
fn warm_overwritten_scratch_is_not_zero_filled() {
    let mut scratch = vec![7.0; 8];
    let allocation = scratch.as_ptr();
    size_overwritten_scratch(&mut scratch, 8);
    assert_eq!(scratch.as_ptr(), allocation);
    assert_eq!(scratch, vec![7.0; 8]);

    size_overwritten_scratch(&mut scratch, 10);
    assert_eq!(&scratch[..8], &[7.0; 8]);
    assert_eq!(&scratch[8..], &[0.0; 2]);
}

/// Clearing the pitch lock is varispeed: 2× speed reads the source twice as
/// fast and its pitch rides along.
#[test]
fn warped_mix_reads_double_source_for_2x_speed() {
    let mut mixer = ExportAudioMixer::of_spans(vec![Span {
        path: PathBuf::from("/dev/null"),
        start: 0,
        end: 1000,
        source_start: 0,
        source_len: 2000,
        reversed: false,
        warp: SpanWarp::FlatSpeed { num: 2, den: 1 },
        preserve_pitch: false,
        denoise: false,
        volume: Param::Constant(1.0),
        pan: Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        reader: Some(Box::new(SineReader {
            pos: 0,
            rate: EXPORT_AUDIO_RATE,
            freq: 440.0,
        })),
        exhausted: false,
        bus: 0,
    }]);
    let mut out = vec![0.0; 500 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    assert!(out.iter().any(|&s| s.abs() > 0.01));
    let reader_pos = mixer.spans[0].reader.as_ref().unwrap().position().unwrap();
    assert!(
        reader_pos >= 998,
        "2× speed should advance source ~twice as fast, got {reader_pos}"
    );
}

/// Flat stereo buffer: left = 0.5, right = −0.25 at every frame.
struct FlatStereoReader {
    pos: i64,
    left: f32,
    right: f32,
}

impl AudioReader for FlatStereoReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        for f in 0..frames {
            out[f * CHANNELS] = self.left;
            out[f * CHANNELS + 1] = self.right;
        }
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

fn mixer_with_span(span: Span) -> ExportAudioMixer {
    ExportAudioMixer::of_spans(vec![span])
}

fn flat_span(pan: Param<f32>, left: f32, right: f32) -> Span {
    Span {
        path: PathBuf::from("/dev/null"),
        start: 0,
        end: 1000,
        source_start: 0,
        source_len: 1000,
        reversed: false,
        warp: SpanWarp::Linear,
        preserve_pitch: true,
        denoise: false,
        volume: Param::Constant(1.0),
        pan,
        fade_in: 0,
        fade_out: 0,
        reader: Some(Box::new(FlatStereoReader {
            pos: 0,
            left,
            right,
        })),
        exhausted: false,
        bus: 0,
    }
}

#[test]
fn pan_zero_is_bit_exact_passthrough() {
    let src_l = 0.5_f32;
    let src_r = -0.25_f32;
    let mut mixer = mixer_with_span(flat_span(Param::Constant(0.0), src_l, src_r));
    let mut out = vec![0.0; 64 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for frame in 0..64 {
        assert_eq!(out[frame * CHANNELS], src_l);
        assert_eq!(out[frame * CHANNELS + 1], src_r);
    }
}

#[test]
fn full_left_pan_zeroes_right_and_boosts_left_by_sqrt2() {
    let src_l = 0.5_f32;
    let src_r = -0.25_f32;
    let mut mixer = mixer_with_span(flat_span(Param::Constant(-1.0), src_l, src_r));
    let mut out = vec![0.0; 16 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    let expect_l = src_l * SQRT_2;
    for frame in 0..16 {
        assert!(
            (out[frame * CHANNELS] - expect_l).abs() < 1e-6,
            "left {frame}: got {} want {expect_l}",
            out[frame * CHANNELS]
        );
        assert_eq!(out[frame * CHANNELS + 1], 0.0);
    }
}

#[test]
fn full_right_pan_zeroes_left_and_boosts_right_by_sqrt2() {
    let src_l = 0.5_f32;
    let src_r = -0.25_f32;
    let mut mixer = mixer_with_span(flat_span(Param::Constant(1.0), src_l, src_r));
    let mut out = vec![0.0; 16 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    let expect_r = src_r * SQRT_2;
    for frame in 0..16 {
        // cos(π/2) is a tiny float residue, not exact 0.
        assert!(out[frame * CHANNELS].abs() < 1e-6);
        assert!((out[frame * CHANNELS + 1] - expect_r).abs() < 1e-6);
    }
}

#[test]
fn keyframed_pan_sweeps_from_left_to_right() {
    use aria_models::{Easing, Keyframe};
    let pan = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: -1.0,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 100,
                value: 1.0,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    // Mono-like source (identical L/R) so pan distributes rather than
    // just balancing unequal channels.
    let mut mixer = mixer_with_span(flat_span(pan, 0.5, 0.5));
    let mut t0 = vec![0.0; CHANNELS];
    mixer.mix_into(0, &mut t0).unwrap();
    assert!((t0[0] - 0.5 * SQRT_2).abs() < 1e-5, "t0 left");
    assert!(t0[1].abs() < 1e-6, "t0 right silent");

    let mut t1 = vec![0.0; CHANNELS];
    mixer.mix_into(100, &mut t1).unwrap();
    assert!(t1[0].abs() < 1e-6, "t1 left silent");
    assert!((t1[1] - 0.5 * SQRT_2).abs() < 1e-5, "t1 right");
}

#[test]
fn stereo_pan_is_balance_not_true_stereo_pan() {
    // Unequal L/R: hard-left keeps the left sample (×√2) and drops right;
    // it does *not* fold right into left (true stereo pan would).
    let src_l = 0.4_f32;
    let src_r = 0.8_f32;
    let mut mixer = mixer_with_span(flat_span(Param::Constant(-1.0), src_l, src_r));
    let mut out = vec![0.0; 4 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    assert!((out[0] - src_l * SQRT_2).abs() < 1e-6);
    assert_eq!(out[1], 0.0);
    // Right energy is gone, not redistributed.
    assert!((out[0] - src_r * SQRT_2).abs() > 0.1);
}

#[test]
fn pan_channel_gains_are_unity_at_center() {
    let (l, r) = pan_channel_gains(0.0);
    assert!((l - 1.0).abs() < 1e-6);
    assert!((r - 1.0).abs() < 1e-6);
    let (l, r) = pan_channel_gains(-1.0);
    assert!((l - SQRT_2).abs() < 1e-6);
    assert!(r.abs() < 1e-6);
    let (l, r) = pan_channel_gains(1.0);
    assert!(l.abs() < 1e-6);
    assert!((r - SQRT_2).abs() < 1e-6);
}

/// A source whose frame `n` reads back as `n / RAMP_SCALE`, so the walk through
/// the source is recoverable from the output. The divisor keeps every value
/// inside full scale: `mix_into` clamps its output to [-1, 1], so a fixture
/// that ramps past 1.0 would be testing the clamp instead of the walk.
const RAMP_SCALE: f32 = 32.0;

struct RampReader {
    pos: i64,
}

impl AudioReader for RampReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        for f in 0..frames {
            let sample = (self.pos + f as i64) as f32 / RAMP_SCALE;
            out[f * CHANNELS] = sample;
            out[f * CHANNELS + 1] = sample;
        }
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

fn ramp_span(reversed: bool, len: i64, warp: SpanWarp, source_len: i64) -> Span {
    Span {
        path: PathBuf::from("/dev/null"),
        start: 0,
        end: len,
        source_start: 0,
        source_len,
        reversed,
        warp,
        // The mapping tests below assert exact varispeed samples; the
        // stretch path has its own tests.
        preserve_pitch: false,
        denoise: false,
        volume: Param::Constant(1.0),
        pan: Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        reader: Some(Box::new(RampReader { pos: 0 })),
        exhausted: false,
        bus: 0,
    }
}

#[test]
fn reversed_linear_span_plays_source_window_backward() {
    let len = 8;
    let mut mixer = mixer_with_span(ramp_span(true, len, SpanWarp::Linear, len));
    let mut out = vec![0.0; (len as usize) * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for frame in 0..len as usize {
        let expect = (len - 1 - frame as i64) as f32 / RAMP_SCALE;
        assert!(
            (out[frame * CHANNELS] - expect).abs() < 1e-4,
            "frame {frame}: got {} want {expect}",
            out[frame * CHANNELS]
        );
    }
}

#[test]
fn forward_linear_span_plays_source_window_forward() {
    let len = 8;
    let mut mixer = mixer_with_span(ramp_span(false, len, SpanWarp::Linear, len));
    let mut out = vec![0.0; (len as usize) * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for frame in 0..len as usize {
        let expect = frame as f32 / RAMP_SCALE;
        assert!(
            (out[frame * CHANNELS] - expect).abs() < 1e-4,
            "frame {frame}: got {} want {expect}",
            out[frame * CHANNELS]
        );
    }
}

#[test]
fn reversed_double_speed_walks_source_backward_twice_as_fast() {
    let out_len = 8;
    let source_len = 16;
    let mut mixer = mixer_with_span(ramp_span(
        true,
        out_len,
        SpanWarp::FlatSpeed { num: 2, den: 1 },
        source_len,
    ));
    let mut out = vec![0.0; (out_len as usize) * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for frame in 0..out_len as usize {
        let expect = (source_len - 1 - 2 * frame as i64) as f32 / RAMP_SCALE;
        assert!(
            (out[frame * CHANNELS] - expect).abs() < 1e-4,
            "frame {frame}: got {} want {expect}",
            out[frame * CHANNELS]
        );
    }
}

#[test]
fn reversed_clip_is_included_in_project_mixer() {
    use aria_models::{MediaSource, Project, TimeRange, TrackKind};
    let mut project = Project::new("rev", Rational::FPS_24);
    let track = project.add_track(TrackKind::Audio, "A1");
    let media = project.add_media(MediaSource::new(
        "/dev/null",
        0,
        0,
        Rational::FPS_24,
        240,
        true,
    ));
    let source = TimeRange::at_rate(0, 48, Rational::FPS_24);
    let start = aria_models::RationalTime::new(0, Rational::FPS_24);
    let clip = project.add_clip(track, media, source, start).unwrap();
    project
        .set_clip_speed(clip, Rational::new(1, 1), true)
        .unwrap();
    assert!(ExportAudioMixer::for_project(&project).unwrap().is_some());
}
