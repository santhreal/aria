//! Pitch is the thing the lock claims to hold, so it is what gets measured.
//!
//! The class of defect closed here: `preserve_pitch` was stored, shown as a
//! control, and defaulted to on while the mixer was varispeed only, so the
//! control said the opposite of what the file did. These assert the audible
//! property (how many times the waveform crosses zero per second) rather
//! than which code path ran, so an implementation swap that regresses pitch
//! fails here.
//!
//! Not covered: stretch quality. Nothing in a test can tell warble from
//! clean overlap-add.

use super::*;
use aria_core::AudioReader;

/// A 440 Hz tone whose phase is a function of absolute position, so a seek
/// lands on the same waveform every time. Anything else would make the
/// block-independence test meaningless.
struct Tone {
    pos: i64,
    freq: f32,
}

impl AudioReader for Tone {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        for frame in 0..frames {
            let t = (self.pos + frame as i64) as f32 / EXPORT_AUDIO_RATE as f32;
            let sample = (std::f32::consts::TAU * self.freq * t).sin();
            for ch in 0..CHANNELS {
                out[frame * CHANNELS + ch] = sample;
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

/// Dominant frequency of the left channel, by zero crossings.
fn crossings_per_second(out: &[f32]) -> f32 {
    let frames = out.len() / CHANNELS;
    let mut crossings = 0_u32;
    for frame in 1..frames {
        let previous = out[(frame - 1) * CHANNELS];
        let current = out[frame * CHANNELS];
        if (previous < 0.0) != (current < 0.0) {
            crossings += 1;
        }
    }
    crossings as f32 * EXPORT_AUDIO_RATE as f32 / frames as f32 / 2.0
}

fn sine_span(speed: i32, preserve_pitch: bool, len: i64) -> Span {
    Span {
        path: std::path::PathBuf::from("/dev/null"),
        start: 0,
        end: len,
        source_start: 0,
        source_len: len * speed as i64,
        reversed: false,
        warp: SpanWarp::FlatSpeed { num: speed, den: 1 },
        preserve_pitch,
        denoise: false,
        volume: aria_models::Param::Constant(1.0),
        pan: aria_models::Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        crossfade_in: None,
        crossfade_out: None,
        reader: Some(Box::new(Tone {
            pos: 0,
            freq: 440.0,
        })),
        exhausted: false,
        bus: 0,
    }
}

fn mix(span: Span, frames: usize) -> Vec<f32> {
    let mut mixer = ExportAudioMixer::of_spans(vec![span]);
    let mut out = vec![0.0; frames * CHANNELS];
    mixer.mix_into(0, &mut out).expect("mix");
    out
}

#[test]
fn a_pitch_locked_double_speed_clip_keeps_its_pitch() {
    let out = mix(sine_span(2, true, 24_000), 8_192);
    let measured = crossings_per_second(&out);
    assert!(
        (measured - 440.0).abs() < 40.0,
        "a 440 Hz source at 2× with pitch locked measured {measured} Hz"
    );
}

#[test]
fn an_unlocked_double_speed_clip_rides_the_rate() {
    let out = mix(sine_span(2, false, 24_000), 8_192);
    let measured = crossings_per_second(&out);
    assert!(
        (measured - 880.0).abs() < 60.0,
        "a 440 Hz source at 2× varispeed measured {measured} Hz"
    );
}

#[test]
fn a_pitch_locked_half_speed_clip_keeps_its_pitch() {
    let mut span = sine_span(1, true, 24_000);
    span.warp = SpanWarp::FlatSpeed { num: 1, den: 2 };
    let out = mix(span, 8_192);
    let measured = crossings_per_second(&out);
    assert!(
        (measured - 440.0).abs() < 40.0,
        "a 440 Hz source at half speed with pitch locked measured {measured} Hz"
    );
}

#[test]
fn unity_speed_is_untouched_by_the_lock() {
    let locked = mix(sine_span(1, true, 24_000), 4_096);
    let unlocked = mix(sine_span(1, false, 24_000), 4_096);
    assert_eq!(
        locked, unlocked,
        "nothing is stretched at unity speed, so the lock must be inert"
    );
}

/// Every block re-seeks the source, so a stretched sample must not depend on
/// how the export happened to be blocked. Without a deterministic alignment
/// search this drifts, and two exports of one project differ.
#[test]
fn a_stretched_block_does_not_depend_on_the_block_size() {
    let whole = mix(sine_span(2, true, 24_000), 4_096);
    let mut mixer = ExportAudioMixer::of_spans(vec![sine_span(2, true, 24_000)]);
    let mut halves = vec![0.0; 4_096 * CHANNELS];
    let (first, second) = halves.split_at_mut(2_048 * CHANNELS);
    mixer.mix_into(0, first).expect("first half");
    mixer.mix_into(2_048, second).expect("second half");
    for (index, (a, b)) in whole.iter().zip(halves.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-5,
            "sample {index} differs by block size: {a} vs {b}"
        );
    }
}

fn tone_span(freq: f32, len: i64) -> Span {
    Span {
        path: std::path::PathBuf::from("/dev/null"),
        start: 0,
        end: len,
        source_start: 0,
        source_len: len,
        reversed: false,
        warp: SpanWarp::Linear,
        preserve_pitch: true,
        denoise: false,
        volume: aria_models::Param::Constant(1.0),
        pan: aria_models::Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        crossfade_in: None,
        crossfade_out: None,
        reader: Some(Box::new(Tone { pos: 0, freq })),
        exhausted: false,
        bus: 0,
    }
}

fn render_shuttle_blocks(
    span: Span,
    anchor_sample: i64,
    speed_num: i32,
    speed_den: i32,
    total_out_frames: usize,
) -> Vec<f32> {
    let mut mixer = ExportAudioMixer::of_spans(vec![span]);
    let mut out = vec![0.0; total_out_frames * CHANNELS];
    let block_frames = 1_024usize;
    let mut stream_frame = 0usize;
    while stream_frame < total_out_frames {
        let take = (total_out_frames - stream_frame).min(block_frames);
        let slice = &mut out[stream_frame * CHANNELS..(stream_frame + take) * CHANNELS];
        mixer
            .mix_shuttle_into(
                anchor_sample,
                stream_frame as i64,
                speed_num,
                speed_den,
                slice,
            )
            .expect("shuttle mix");
        stream_frame += take;
    }
    out
}

#[test]
fn shuttle_varispeed_holds_1khz_pitch_and_duration_up_to_4x_in_both_directions() {
    let source_frames = 48_000_i64;
    // (speed_num, speed_den, expected_out_frames)
    let cases = [
        (200, 100, 24_000usize),
        (50, 100, 96_000usize),
        (-100, 100, 48_000usize),
        (-400, 100, 12_000usize),
        (110, 100, 43_636usize),
        (-25, 100, 192_000usize),
    ];
    for (speed_num, speed_den, expected_frames) in cases {
        let anchor = if speed_num < 0 { source_frames } else { 0 };
        // Render past the expected end so the span boundary is observable.
        let tail_frames = 4_096usize;
        let out = render_shuttle_blocks(
            tone_span(1_000.0, source_frames),
            anchor,
            speed_num,
            speed_den,
            expected_frames + tail_frames,
        );
        // Trim 1024-frame edge pads when measuring steady-state pitch.
        let steady_start = 1_024 * CHANNELS;
        let steady_end = (expected_frames - 1_024) * CHANNELS;
        let measured = crossings_per_second(&out[steady_start..steady_end]);
        let rel_err = (measured - 1_000.0).abs() / 1_000.0;
        assert!(
            rel_err <= 0.02,
            "speed {speed_num}/{speed_den}: 1 kHz measured {measured} Hz ({:.2}% error, > 2%)",
            rel_err * 100.0
        );
        // Active span covers expected_frames (within one OLA frame at the edge)
        // and goes silent after the source span ends.
        let last_active_frame = (0..expected_frames + tail_frames)
            .rev()
            .find(|&f| out[f * CHANNELS].abs() > 1e-3)
            .expect("audible output");
        assert!(
            last_active_frame.abs_diff(expected_frames) <= OLA_FRAME,
            "speed {speed_num}/{speed_den}: active span ended at {last_active_frame}, expected {expected_frames}"
        );
    }
}

#[test]
fn shuttle_reverse_output_is_forward_render_played_backward_within_wsola_tolerance() {
    let source_frames = 48_000_i64;
    let mut fwd_span = tone_span(1_000.0, source_frames);
    fwd_span.fade_in = 12_000;
    fwd_span.fade_out = 24_000;
    let mut rev_span = tone_span(1_000.0, source_frames);
    rev_span.fade_in = 12_000;
    rev_span.fade_out = 24_000;

    let fwd_1x = render_shuttle_blocks(fwd_span, 0, 100, 100, source_frames as usize);
    let rev_1x = render_shuttle_blocks(rev_span, source_frames, -100, 100, source_frames as usize);
    for frame in 0..source_frames as usize {
        let fwd_l = fwd_1x[(source_frames as usize - 1 - frame) * CHANNELS];
        let rev_l = rev_1x[frame * CHANNELS];
        assert!(
            (fwd_l - rev_l).abs() < 1e-4,
            "-1x frame {frame} differed from reversed +1x: {rev_l} vs {fwd_l}"
        );
    }

    // At 2x vs -2x with an asymmetric fade (fade_in = 6000, fade_out = 24000),
    // playing the forward render backward matches the reverse render's window
    // RMS envelope and 1 kHz pitch within WSOLA tolerance.
    let mut fwd_2x_span = tone_span(1_000.0, source_frames);
    fwd_2x_span.fade_in = 6_000;
    fwd_2x_span.fade_out = 24_000;
    let mut rev_2x_span = tone_span(1_000.0, source_frames);
    rev_2x_span.fade_in = 6_000;
    rev_2x_span.fade_out = 24_000;
    let out_frames = 24_000usize;
    let fwd_2x = render_shuttle_blocks(fwd_2x_span, 0, 200, 100, out_frames);
    let rev_2x = render_shuttle_blocks(rev_2x_span, source_frames, -200, 100, out_frames);
    let mut fwd_2x_reversed = Vec::with_capacity(fwd_2x.len());
    for frame in (0..out_frames).rev() {
        fwd_2x_reversed.push(fwd_2x[frame * CHANNELS]);
        fwd_2x_reversed.push(fwd_2x[frame * CHANNELS + 1]);
    }
    let window = 2_000usize;
    for chunk in 1..(out_frames / window - 1) {
        let start = chunk * window * CHANNELS;
        let end = (chunk + 1) * window * CHANNELS;
        let rms = |slice: &[f32]| -> f32 {
            (slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32).sqrt()
        };
        let rev_rms = rms(&rev_2x[start..end]);
        let fwd_rev_rms = rms(&fwd_2x_reversed[start..end]);
        assert!(
            (rev_rms - fwd_rev_rms).abs() < 0.08,
            "chunk {chunk} RMS mismatch: rev={rev_rms} vs fwd_reversed={fwd_rev_rms}"
        );
    }
}

#[test]
fn shuttle_above_4x_outputs_silence() {
    let source_frames = 48_000_i64;
    for (speed_num, speed_den) in [(401, 100), (800, 100), (-401, 100), (-800, 100), (0, 100)] {
        let anchor = if speed_num < 0 { source_frames } else { 0 };
        let out = render_shuttle_blocks(
            tone_span(1_000.0, source_frames),
            anchor,
            speed_num,
            speed_den,
            2_048,
        );
        assert!(
            out.iter().all(|&s| s == 0.0),
            "speed {speed_num}/{speed_den} must be silent above 4x or at 0"
        );
    }
}
