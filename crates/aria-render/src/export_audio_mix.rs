//! Per-span sample maths for the export mixer: opening a span's reader,
//! interpolating inside a decoded window, mapping output frames back to source
//! offsets, and the exact tick↔sample conversion the whole export loop shares.
//!
//! `pan_channel_gains` is defined in `export_audio.rs` alongside the pan
//! constants.

// Routed through the parent rather than re-imported from the crates: the parent
// still names every one of these, and one import list for the module pair is
// what keeps a move from quietly orphaning a binding.
use super::{
    AudioReader, CHANNELS, DecodeError, DenoiseReader, EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE,
    OLA_FRAME, OLA_HOP, OLA_PAD, OLA_WARMUP, Span, audio_gain_at, ola_aligned_offset, ola_frames,
    ola_window, pan_channel_gains, warped_source_frame,
};

pub(crate) fn open_span_reader(span: &Span) -> Result<Box<dyn AudioReader>, DecodeError> {
    let reader =
        aria_decoder::open_audio_reader(&span.path, EXPORT_AUDIO_RATE, EXPORT_AUDIO_CHANNELS)
            .map_err(|err| audio_err("open audio source", &span.path, err))?;
    if span.denoise {
        Ok(Box::new(DenoiseReader::new(reader, CHANNELS)))
    } else {
        Ok(reader)
    }
}

/// One interpolated channel sample from the decoded window.
///
/// `pos` is an absolute source-window offset; `floor` is the offset the window
/// starts at. Anything outside the window is silence rather than the nearest
/// edge sample: holding an edge would add a DC step, and the frame is
/// overlap-added, so silence reads as the fade it is.
pub(crate) fn sample_at(window: &[f32], got: usize, floor: i64, pos: f64, ch: usize) -> f32 {
    let rel = pos - floor as f64;
    if rel < 0.0 {
        return 0.0;
    }
    let base = rel.floor() as usize;
    if base >= got {
        return 0.0;
    }
    let frac = (rel - base as f64) as f32;
    let a = window[base * CHANNELS + ch];
    let b = if base + 1 < got {
        window[(base + 1) * CHANNELS + ch]
    } else {
        a
    };
    a + (b - a) * frac
}

/// Channel sum at `pos`, for the alignment search. Correlating the mono mix
/// keeps a stereo pair aligned to each other instead of drifting per channel.
pub(crate) fn mono_at(window: &[f32], got: usize, floor: i64, pos: f64) -> f32 {
    (0..CHANNELS)
        .map(|ch| sample_at(window, got, floor, pos, ch))
        .sum()
}

/// Source-window offset (output sample frames) for clip-relative output
/// sample `rel_out`. Reverse mirrors the video `source_time_at` walk.
pub(crate) fn span_source_offset(span: &Span, rel_out: i64, span_len: i64) -> f64 {
    let forward = warped_source_frame(&span.warp, rel_out, span_len);
    if !span.reversed {
        return forward;
    }
    let last = (span.source_len - 1).max(0) as f64;
    (last - forward).clamp(0.0, last)
}

/// Source sample range `[src_floor, src_ceil)` needed to WSOLA-stretch output
/// frames `[rel_start, rel_end)`.
pub(crate) fn ola_source_bounds(
    rel_start: i64,
    rel_end: i64,
    anchor_at: &mut dyn FnMut(i64) -> f64,
) -> (i64, i64) {
    let (first_frame, last_frame) = ola_frames(rel_start, rel_end);
    let warmup_frame = (first_frame - OLA_WARMUP).max(0);
    let mut src_min = f64::MAX;
    let mut src_max = f64::MIN;
    for k in warmup_frame..=last_frame {
        let anchor = anchor_at(k * OLA_HOP as i64);
        src_min = src_min.min(anchor);
        src_max = src_max.max(anchor);
    }
    src_min -= OLA_PAD as f64;
    src_max += OLA_PAD as f64;
    let src_floor = (src_min.floor() as i64).max(0);
    let src_ceil = (src_max.ceil() as i64 + 1).max(src_floor);
    (src_floor, src_ceil)
}

/// WSOLA overlap-add synthesis of `[rel_start, rel_end)` from decoded stereo
/// `window` starting at `src_floor` with `got` frames.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ola_synthesize(
    window: &[f32],
    got: usize,
    src_floor: i64,
    rel_start: i64,
    rel_end: i64,
    direction: i64,
    anchor_at: &mut dyn FnMut(i64) -> f64,
    weight_scratch: &mut Vec<f32>,
    out: &mut [f32],
) {
    let out_frames = (rel_end - rel_start).max(0) as usize;
    out[..out_frames * CHANNELS].fill(0.0);
    weight_scratch.resize(out_frames, 0.0);
    weight_scratch.fill(0.0);
    let (first_frame, last_frame) = ola_frames(rel_start, rel_end);
    let warmup_frame = (first_frame - OLA_WARMUP).max(0);
    let mut previous = None;
    for k in warmup_frame..=last_frame {
        let anchor = anchor_at(k * OLA_HOP as i64);
        let aligned = ola_aligned_offset(anchor, previous, direction, &mut |pos| {
            mono_at(window, got, src_floor, pos)
        });
        previous = Some(aligned);
        if k < first_frame {
            continue;
        }
        let base_out = k * OLA_HOP as i64;
        for i in 0..OLA_FRAME {
            let rel = base_out + i as i64;
            if rel < rel_start || rel >= rel_end {
                continue;
            }
            let weight = ola_window(i);
            let pos = aligned + (direction * i as i64) as f64;
            let out_idx = (rel - rel_start) as usize;
            weight_scratch[out_idx] += weight;
            for ch in 0..CHANNELS {
                out[out_idx * CHANNELS + ch] += weight * sample_at(window, got, src_floor, pos, ch);
            }
        }
    }
    // Divide by the weight that landed so single-frame edges at span boundaries
    // hold full level instead of fading by half a Hann window.
    for (out_idx, &weight) in weight_scratch.iter().enumerate() {
        if weight < 1e-4 {
            for ch in 0..CHANNELS {
                out[out_idx * CHANNELS + ch] = 0.0;
            }
            continue;
        }
        let inv = 1.0 / weight;
        for ch in 0..CHANNELS {
            let s = &mut out[out_idx * CHANNELS + ch];
            *s *= inv;
            if s.is_subnormal() || !s.is_finite() {
                *s = 0.0;
            }
        }
    }
}

pub(crate) fn accumulate_span_samples(
    span: &Span,
    span_rel_start: i64,
    frames: usize,
    out_offset: usize,
    out: &mut [f32],
    samples: &[f32],
) {
    let span_len = span.end - span.start;
    let first = span_rel_start - span.start;
    // Center pan + unit volume + no fades or crossfades: bit-exact passthrough
    // of today's pre-pan mix (no √2 scaling at rest).
    let unity = span.volume.constant() == Some(1.0)
        && span.pan.constant() == Some(0.0)
        && span.fade_in == 0
        && span.fade_out == 0
        && span.crossfade_in.is_none()
        && span.crossfade_out.is_none();
    if unity {
        for (dst, src) in out[out_offset..]
            .iter_mut()
            .zip(&samples[..frames * CHANNELS])
        {
            *dst += *src;
        }
    } else {
        for frame in 0..frames {
            let pos = first + frame as i64;
            let gain = audio_gain_at(pos, span_len, &span.volume, span.fade_in, span.fade_out)
                * span.crossfade_gain(pos);
            let (left, right) = pan_channel_gains(span.pan.sample(pos));
            let base = out_offset + frame * CHANNELS;
            out[base] += samples[frame * CHANNELS] * gain * left;
            out[base + 1] += samples[frame * CHANNELS + 1] * gain * right;
        }
    }
}

pub(crate) fn audio_err(
    what: &str,
    path: &std::path::Path,
    err: impl std::fmt::Display,
) -> DecodeError {
    DecodeError::Decode(format!("{what} {}: {err}", path.display()))
}

/// `value` ticks at `num/den` fps → sample frames at the export rate (exact
/// i128, floored) — the same conversion the desktop mixer uses.
pub(crate) fn ticks_to_samples(value: i64, num: i32, den: i32) -> i64 {
    if num <= 0 || den <= 0 {
        return 0;
    }
    let frames =
        i128::from(value) * i128::from(den) * i128::from(EXPORT_AUDIO_RATE) / i128::from(num);
    frames.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// Sample-frame boundary of output video frame `n` at `out_num/out_den` fps:
/// the export loop pushes audio block `[boundary(n), boundary(n+1))` after video
/// frame `n`, so audio and video cover identical wall-clock spans.
pub fn sample_boundary(n: i64, out_num: i32, out_den: i32) -> i64 {
    ticks_to_samples(n, out_num, out_den)
}
/// Convert output sample frame `sample` at [`EXPORT_AUDIO_RATE`] to sequence ticks at `fps`.
pub(crate) fn samples_to_ticks(sample: i64, fps: aria_models::Rational) -> i64 {
    if fps.num <= 0 || fps.den <= 0 {
        return 0;
    }
    let ticks = i128::from(sample) * i128::from(fps.num)
        / (i128::from(fps.den) * i128::from(EXPORT_AUDIO_RATE));
    ticks.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// Apply a strip's fader and constant-power stereo pan to interleaved stereo `buf`
/// starting at output sample `start_sample` on sequence rate `fps`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_strip_gain_pan(
    buf: &mut [f32],
    start_sample: i64,
    fps: aria_models::Rational,
    muted: bool,
    static_gain: f32,
    static_pan: f32,
    automation: aria_models::AutomationMode,
    volume_envelope: &aria_models::Param<f32>,
    pan_envelope: &aria_models::Param<f32>,
) {
    if muted {
        buf.fill(0.0);
        return;
    }
    let reads_vol = automation.reads_envelope() && volume_envelope.is_animated();
    let reads_pan = automation.reads_envelope() && pan_envelope.is_animated();
    let const_gain = (!reads_vol).then_some(static_gain);
    let const_pan = (!reads_pan).then_some(static_pan);
    if let (Some(gain), Some(pan)) = (const_gain, const_pan) {
        if gain == 1.0 && pan == 0.0 {
            return;
        }
        if pan == 0.0 {
            for sample in buf.iter_mut() {
                *sample *= gain;
            }
            return;
        }
        let (left, right) = pan_channel_gains(pan);
        let l = gain * left;
        let r = gain * right;
        for frame in buf.as_chunks_mut::<CHANNELS>().0 {
            frame[0] *= l;
            frame[1] *= r;
        }
        return;
    }
    for (idx, frame) in buf.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
        let tick = samples_to_ticks(start_sample + idx as i64, fps);
        let gain = if reads_vol {
            volume_envelope.sample(tick)
        } else {
            static_gain
        };
        let pan = if reads_pan {
            pan_envelope.sample(tick)
        } else {
            static_pan
        };
        if pan == 0.0 {
            frame[0] *= gain;
            frame[1] *= gain;
        } else {
            let (left, right) = pan_channel_gains(pan);
            frame[0] *= gain * left;
            frame[1] *= gain * right;
        }
    }
}
