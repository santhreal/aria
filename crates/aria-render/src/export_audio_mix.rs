//! Per-span sample maths for the export mixer: opening a span's reader,
//! interpolating inside a decoded window, mapping output frames back to source
//! offsets, and the exact tick↔sample conversion the whole export loop shares.
//!
//! Moved out of `export_audio.rs` to stay under the file ceiling; nothing here
//! changed. `pan_channel_gains` stayed behind because it is the only reader of
//! the parent's pan constants, and splitting it from them would have moved the
//! constants too.

// Routed through the parent rather than re-imported from the crates: the parent
// still names every one of these, and one import list for the module pair is
// what keeps a move from quietly orphaning a binding.
use super::{
    AudioReader, CHANNELS, DecodeError, DenoiseReader, EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE,
    Span, audio_gain_at, pan_channel_gains, warped_source_frame,
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
    // Center pan + unit volume + no fades: bit-exact passthrough of today's
    // pre-pan mix (no √2 scaling at rest).
    let unity = span.volume.constant() == Some(1.0)
        && span.pan.constant() == Some(0.0)
        && span.fade_in == 0
        && span.fade_out == 0;
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
            let gain = audio_gain_at(pos, span_len, &span.volume, span.fade_in, span.fade_out);
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
