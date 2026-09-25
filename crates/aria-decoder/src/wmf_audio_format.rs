//! Media Foundation audio format negotiation and the sample arithmetic.
//!
//! Split out of [`super`] to stay under the file ceiling. A pure move: the two
//! halves divide along the line that was already there — the reader in the
//! parent owns COM objects and stream state, everything here is stateless. The
//! six entry points the reader calls are `pub(super)`; the three media-type
//! builders are private because only negotiation uses them.
//!
//! `use super::*` carries the parent's `windows` imports and `SampleFormat`, so
//! neither the import block nor the enum is duplicated.

use super::*;

/// Reader-creation attributes: enable the converter/resampler pipeline.
pub(super) fn reader_attributes() -> Result<IMFAttributes, windows::core::Error> {
    let mut attributes: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut attributes, 1) }?;
    let attributes = attributes.expect("MFCreateAttributes succeeded");
    unsafe {
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
    }
    Ok(attributes)
}

/// Negotiate the reader's audio output type, preferring exact-format
/// passthrough. Returns the (rate, channels, sample format) it will deliver.
pub(super) fn negotiate_output(
    reader: &IMFSourceReader,
    stream: u32,
    out_rate: u32,
    channels: u16,
) -> Result<(u32, usize, SampleFormat), DecodeError> {
    // 1. Float32 at exactly the mixer's rate/channels (resampler inserted).
    let exact = exact_float_type(out_rate, channels).map_err(open_err)?;
    if unsafe { reader.SetCurrentMediaType(stream, None, &exact) }.is_ok() {
        return read_back_output(reader, stream);
    }

    // 2. Any Float32 layout (rate/channels chosen by the source).
    if let Ok(partial) = partial_type(&MFAudioFormat_Float)
        && unsafe { reader.SetCurrentMediaType(stream, None, &partial) }.is_ok()
    {
        return read_back_output(reader, stream);
    }

    // 3. Any 16-bit PCM layout (converted to f32 at ingest).
    if let Ok(partial) = partial_type(&MFAudioFormat_PCM)
        && unsafe { reader.SetCurrentMediaType(stream, None, &partial) }.is_ok()
    {
        return read_back_output(reader, stream);
    }

    Err(DecodeError::unsupported(
        "could not negotiate PCM output for the audio track",
    ))
}

/// A complete Float32 PCM media type at `rate`/`channels`.
fn exact_float_type(
    rate: u32,
    channels: u16,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, windows::core::Error> {
    let block_align = u32::from(channels) * 4;
    let media_type = unsafe { MFCreateMediaType() }?;
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
        media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
        media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))?;
        media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32)?;
        media_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)?;
        media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * block_align)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
    }
    Ok(media_type)
}

/// A partial media type (major + subtype); the reader completes the layout.
fn partial_type(
    subtype: &GUID,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, windows::core::Error> {
    let media_type = unsafe { MFCreateMediaType() }?;
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype)?;
    }
    Ok(media_type)
}

/// Read the negotiated output layout back from the reader.
fn read_back_output(
    reader: &IMFSourceReader,
    stream: u32,
) -> Result<(u32, usize, SampleFormat), DecodeError> {
    let media_type = unsafe { reader.GetCurrentMediaType(stream) }.map_err(open_err)?;
    let rate =
        unsafe { media_type.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.map_err(open_err)?;
    let channels = unsafe { media_type.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.map_err(open_err)?;
    if rate == 0 || channels == 0 {
        return Err(DecodeError::Open(
            "audio output type reports zero rate or channels".into(),
        ));
    }
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.map_err(open_err)?;
    let format = if subtype == MFAudioFormat_Float {
        SampleFormat::F32
    } else if subtype == MFAudioFormat_PCM {
        let bits = unsafe { media_type.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE) }.unwrap_or(16);
        if bits != 16 {
            return Err(DecodeError::Unsupported(format!(
                "unsupported PCM bit depth: {bits}"
            )));
        }
        SampleFormat::I16
    } else {
        return Err(DecodeError::Unsupported(format!(
            "unexpected audio output subtype: {subtype:?}"
        )));
    };
    Ok((rate, channels as usize, format))
}

/// Mix one interleaved source frame into `dst.len()` output channels:
/// identity, mono fan-out, average-downmix to mono, or positional mapping
/// (extra source channels dropped). The resampler DSP handles the standard
/// matrices on the passthrough path; this covers the Rust fallback.
pub(super) fn mix_frame(src: &[f32], dst: &mut [f32]) {
    if src.len() == dst.len() {
        dst.copy_from_slice(src);
    } else if src.len() == 1 {
        dst.fill(src[0]);
    } else if dst.len() == 1 {
        dst[0] = src.iter().sum::<f32>() / src.len() as f32;
    } else {
        for (c, value) in dst.iter_mut().enumerate() {
            *value = src[c.min(src.len() - 1)];
        }
    }
}

/// Source-frame index of a 100-ns timestamp (rounded to nearest).
pub(super) fn src_frame_of_hns(hns: i64, src_rate: u32) -> i64 {
    let num = i128::from(hns.max(0)) * i128::from(src_rate);
    let den = i128::from(HNS_PER_SEC);
    ((num + den / 2) / den) as i64
}

/// Output-frame index equivalent to source frame `s` (rounded to nearest).
pub(super) fn out_frame_of_src(s: i64, src_rate: u32, out_rate: u32) -> i64 {
    let num = i128::from(s) * i128::from(out_rate);
    let den = i128::from(src_rate);
    ((num + den / 2) / den) as i64
}

/// Source position of output frame `pos`: `(floor, frac)` with
/// `source = floor + frac` and `frac ∈ [0, 1)`. Exact in `i128`.
pub(super) fn src_pos_of(pos: i64, src_rate: u32, out_rate: u32) -> (i64, f32) {
    let num = i128::from(pos) * i128::from(src_rate);
    let den = i128::from(out_rate);
    let floor = num.div_euclid(den);
    let frac = num.rem_euclid(den) as f64 / den as f64;
    (floor as i64, frac as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_frame_covers_the_channel_layouts() {
        // Identity.
        let mut out = [0.0; 2];
        mix_frame(&[0.25, -0.5], &mut out);
        assert_eq!(out, [0.25, -0.5]);
        // Mono fan-out.
        let mut out = [0.0; 2];
        mix_frame(&[0.75], &mut out);
        assert_eq!(out, [0.75, 0.75]);
        // Downmix to mono averages.
        let mut out = [0.0; 1];
        mix_frame(&[1.0, 0.0], &mut out);
        assert_eq!(out, [0.5]);
        // Positional map: extra source channels dropped, missing ones cloned.
        let mut out = [0.0; 3];
        mix_frame(&[0.1, 0.2], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.2]);
    }

    #[test]
    fn src_pos_is_exact_for_equal_rates() {
        for pos in [0, 1, 47_999, 48_000] {
            let (floor, frac) = src_pos_of(pos, 48_000, 48_000);
            assert_eq!(floor, pos);
            assert_eq!(frac, 0.0);
        }
    }

    #[test]
    fn src_pos_tracks_ratio_for_unequal_rates() {
        // 44.1k source read at 48k out: output frame 48000 is source 44100.
        let (floor, frac) = src_pos_of(48_000, 44_100, 48_000);
        assert_eq!(floor, 44_100);
        assert_eq!(frac, 0.0);
        // One output frame is 0.91875 source frames.
        let (floor, frac) = src_pos_of(1, 44_100, 48_000);
        assert_eq!(floor, 0);
        assert!((frac - 0.918_75).abs() < 1e-6);
    }

    #[test]
    fn frame_conversions_round_to_nearest() {
        // 1 second of 100-ns ticks at 44.1 kHz.
        assert_eq!(src_frame_of_hns(10_000_000, 44_100), 44_100);
        // Half a sample rounds up.
        assert_eq!(src_frame_of_hns(113, 44_100), 0);
        assert_eq!(src_frame_of_hns(114, 44_100), 1);
        // Source->output round trip at the common 44.1k -> 48k ratio.
        assert_eq!(out_frame_of_src(44_100, 44_100, 48_000), 48_000);
        assert_eq!(out_frame_of_src(22_050, 44_100, 48_000), 24_000);
    }
}
