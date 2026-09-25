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
