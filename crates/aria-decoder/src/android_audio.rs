//! The audio reader: [`MediaCodecAudioReader`] decodes a whole audio track to
//! interleaved `f32` up front, then mixes and resamples it on demand.
//!
//! Declared from `android.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

// `android.media.AudioFormat` ENCODING_* values (KEY_PCM_ENCODING).
const ENCODING_PCM_16BIT: i32 = 2;
const ENCODING_PCM_FLOAT: i32 = 4;

/// Safety net on the audio decode pump (whole stream, so a larger budget than
/// the per-frame video pump).
const MAX_AUDIO_PUMP_ITERS: usize = 1_000_000;

/// A pull-based audio decoder over one file: MediaExtractor + MediaCodec decode
/// the whole audio track to PCM up front, which is then converted to
/// interleaved `f32`, mixed to `channels`, and linearly resampled to `out_rate`
/// on demand.
///
/// Decoding eagerly (rather than streaming) keeps seeking trivial — random
/// access into an in-memory buffer — at the cost of holding the decoded track;
/// fine for export, the only consumer in this pass.
pub struct MediaCodecAudioReader {
    /// Whole track as interleaved `f32` at `channels`, still at the *source*
    /// sample rate (resampled to `out_rate` lazily in [`read`](Self::read)).
    samples: Vec<f32>,
    channels: usize,
    src_rate: u32,
    out_rate: u32,
    /// Source-frame count (`samples.len() / channels`).
    src_frames: i64,
    /// Output-frame position of the next [`read`](Self::read).
    position: Option<i64>,
}

// SAFETY: `MediaCodecAudioReader` contains only owned Rust heap buffers (`Vec<f32>`)
// and primitive numeric state. No raw pointers, file descriptors, or native NDK handles
// outlive `open()`. Decoded samples are immutable after construction and mutation is
// restricted to exclusive `&mut self` access on the calling thread, satisfying `Send`.
unsafe impl Send for MediaCodecAudioReader {}

impl MediaCodecAudioReader {
    /// Decode the first audio track of `path` to interleaved `f32`, mixed to
    /// `channels` and (lazily) resampled to `out_rate`.
    pub fn open(path: &Path, out_rate: u32, channels: u16) -> Result<Self, DecodeError> {
        if out_rate == 0 || channels == 0 {
            return Err(DecodeError::unsupported("zero audio rate or channels"));
        }
        let channels = channels as usize;

        // SAFETY: `AMediaExtractor_new()` allocates a new native extractor instance or returns null on OOM.
        let extractor = unsafe { AMediaExtractor_new() };
        if extractor.is_null() {
            return Err(DecodeError::Open(
                "AMediaExtractor_new returned null".into(),
            ));
        }
        let mut guard = PartialCleanup {
            extractor,
            codec: ptr::null_mut(),
            format: ptr::null_mut(),
        };
        // Holds the source fd open through decode; dropped at the end of `open`,
        // after `guard` tears the extractor down.
        let _source = set_data_source_fd(extractor, path)?;

        let (track_index, track_format, mime) = find_audio_track(extractor)?;
        guard.format = track_format;

        // SAFETY: `track_format` is a non-null, valid `AMediaFormat` pointer returned by `find_audio_track`.
        // `AMEDIAFORMAT_KEY_SAMPLE_RATE` is an NDK static null-terminated C string constant.
        let container_rate = unsafe { format_i32(track_format, AMEDIAFORMAT_KEY_SAMPLE_RATE) }
            .filter(|r| *r > 0)
            .unwrap_or(out_rate as i32) as u32;
        // SAFETY: `track_format` is valid; `AMEDIAFORMAT_KEY_CHANNEL_COUNT` is an NDK static C string.
        let container_channels =
            unsafe { format_i32(track_format, AMEDIAFORMAT_KEY_CHANNEL_COUNT) }
                .filter(|c| *c > 0)
                .unwrap_or(channels as i32);

        // SAFETY: `extractor` is non-null and `track_index` was validated by `find_audio_track` (< track_count).
        let status = unsafe { AMediaExtractor_selectTrack(extractor, track_index) };
        if !media_ok(status) {
            return Err(DecodeError::Open(
                "AMediaExtractor_selectTrack failed".into(),
            ));
        }
        let c_mime = CString::new(mime.clone())
            .map_err(|_| DecodeError::Unsupported(format!("MIME type has a NUL byte: {mime}")))?;
        // SAFETY: `c_mime.as_ptr()` points to a valid null-terminated C string for the duration of this call.
        let codec = unsafe { AMediaCodec_createDecoderByType(c_mime.as_ptr()) };
        if codec.is_null() {
            return Err(DecodeError::Unsupported(format!(
                "no MediaCodec decoder for {mime}"
            )));
        }
        guard.codec = codec;

        // SAFETY: `codec` is a newly created `AMediaCodec` handle and `track_format` is a valid
        // `AMediaFormat` pointer. Null surface/crypto pointers configure the codec in ByteBuffer mode.
        let config_status = unsafe {
            AMediaCodec_configure(codec, track_format, ptr::null_mut(), ptr::null_mut(), 0)
        };
        if !media_ok(config_status) {
            return Err(DecodeError::Decode(
                "AMediaCodec_configure (audio) failed".into(),
            ));
        }
        // SAFETY: `codec` is configured and transitioned to the executing state by `AMediaCodec_start`.
        let start_status = unsafe { AMediaCodec_start(codec) };
        if !media_ok(start_status) {
            return Err(DecodeError::Decode(
                "AMediaCodec_start (audio) failed".into(),
            ));
        }

        // Track format is copied by `configure`; release our reference.
        // SAFETY: `track_format` is an owned `AMediaFormat` allocated by `getTrackFormat` and no
        // longer needed after `configure`.
        unsafe { AMediaFormat_delete(track_format) };
        guard.format = ptr::null_mut();
        let (interleaved, src_rate, src_channels) = decode_audio_track(
            extractor,
            codec,
            container_rate,
            container_channels,
            channels,
        )?;

        // `guard`'s Drop tears down the codec + extractor now that we're done.
        drop(guard);

        let src_frames = (interleaved.len() / channels) as i64;
        let _ = src_channels; // already mixed to `channels`
        Ok(Self {
            samples: interleaved,
            channels,
            src_rate,
            out_rate,
            src_frames,
            position: Some(0),
        })
    }

    /// Interleaved `f32` for source frame `frame` (clamped to range), linearly
    /// interpolating between the two nearest source frames.
    fn sample_frame(&self, frame: f64, out: &mut [f32]) {
        let ch = self.channels;
        if self.src_frames == 0 {
            out.iter_mut().for_each(|s| *s = 0.0);
            return;
        }
        let i0 = frame.floor();
        let frac = (frame - i0) as f32;
        let i0 = (i0 as i64).clamp(0, self.src_frames - 1) as usize;
        let i1 = (i0 + 1).min(self.src_frames as usize - 1);
        let base0 = i0 * ch;
        let base1 = i1 * ch;
        for (c, slot) in out.iter_mut().enumerate().take(ch) {
            let a = self.samples[base0 + c];
            let b = self.samples[base1 + c];
            *slot = a + (b - a) * frac;
        }
    }
}

impl AudioReader for MediaCodecAudioReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let ch = self.channels;
        debug_assert_eq!(out.len() % ch, 0, "buffer not a frame multiple");
        let want = out.len() / ch;
        let pos = self.position.unwrap_or(0);
        let ratio = self.src_rate as f64 / self.out_rate as f64;

        let mut produced = 0;
        while produced < want {
            let out_frame = pos + produced as i64;
            let src_pos = out_frame as f64 * ratio;
            if src_pos >= self.src_frames as f64 {
                break; // past end of source
            }
            let dst = &mut out[produced * ch..(produced + 1) * ch];
            self.sample_frame(src_pos, dst);
            produced += 1;
        }

        self.position = Some(pos + produced as i64);
        Ok(produced)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        // Random access into the decoded buffer: just move the cursor.
        self.position = Some(frame.max(0));
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        self.position
    }
}

/// Scan the extractor's tracks and return the first `audio/*` one: its index,
/// its (caller-owned) format, and its MIME type.
fn find_audio_track(
    extractor: *mut AMediaExtractor,
) -> Result<(usize, *mut AMediaFormat, String), DecodeError> {
    // SAFETY: `extractor` is a non-null, valid `AMediaExtractor` handle with an active data source.
    let track_count = unsafe { AMediaExtractor_getTrackCount(extractor) };
    for index in 0..track_count {
        // SAFETY: `extractor` is initialized with a data source and `index` is within `0..track_count`.
        let format = unsafe { AMediaExtractor_getTrackFormat(extractor, index) };
        if format.is_null() {
            continue;
        }
        // SAFETY: `format` is a non-null `AMediaFormat` pointer returned by `getTrackFormat`;
        // `AMEDIAFORMAT_KEY_MIME` is an NDK-provided static null-terminated C string constant.
        let mime = unsafe { format_string(format, AMEDIAFORMAT_KEY_MIME) };
        match mime {
            Some(mime) if mime.starts_with("audio/") => return Ok((index, format, mime)),
            // SAFETY: `format` is non-null and owned; we delete non-audio track formats to avoid leaks.
            _ => unsafe {
                AMediaFormat_delete(format);
            },
        }
    }
    Err(DecodeError::Unsupported("source has no audio track".into()))
}

/// Pump the codec to end-of-stream, returning the whole track as interleaved
/// `f32` mixed to `out_channels`, plus the authoritative source rate/channels
/// (read from the codec's settled output format).
fn decode_audio_track(
    extractor: *mut AMediaExtractor,
    codec: *mut AMediaCodec,
    fallback_rate: u32,
    fallback_channels: i32,
    out_channels: usize,
) -> Result<(Vec<f32>, u32, usize), DecodeError> {
    let mut input_done = false;
    let mut src_rate = fallback_rate;
    let mut src_channels = fallback_channels.max(1) as usize;
    let mut encoding = ENCODING_PCM_16BIT;
    let mut mixed: Vec<f32> = Vec::new();

    let mut info = AMediaCodecBufferInfo {
        offset: 0,
        size: 0,
        presentationTimeUs: 0,
        flags: 0,
    };

    for _ in 0..MAX_AUDIO_PUMP_ITERS {
        if !input_done {
            // SAFETY: `codec` is non-null and in executing state. Timeout is non-blocking (0 us).
            let in_index = unsafe { AMediaCodec_dequeueInputBuffer(codec, INPUT_TIMEOUT_US) };
            if in_index >= 0 {
                let index = in_index as usize;
                let mut capacity: usize = 0;
                // SAFETY: `codec` is executing, `index` was dequeued from `AMediaCodec_dequeueInputBuffer`,
                // and `&mut capacity` receives the allocated input buffer byte size.
                let buffer = unsafe { AMediaCodec_getInputBuffer(codec, index, &mut capacity) };
                if !buffer.is_null() {
                    // SAFETY: `extractor` has a selected audio track; `buffer` points to `capacity` writable bytes.
                    let read =
                        unsafe { AMediaExtractor_readSampleData(extractor, buffer, capacity) };
                    if read < 0 {
                        // SAFETY: `index` is the currently dequeued input buffer; queues EOS marker.
                        unsafe {
                            AMediaCodec_queueInputBuffer(
                                codec,
                                index,
                                0,
                                0,
                                0,
                                AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                            );
                        }
                        input_done = true;
                    } else {
                        // SAFETY: `extractor` is positioned at a valid audio sample (`read >= 0`).
                        let pts = unsafe { AMediaExtractor_getSampleTime(extractor) }.max(0) as u64;
                        // SAFETY: `read as usize <= capacity` bytes were populated in `buffer`; queues
                        // buffer into `codec` and advances `extractor` to next sample.
                        unsafe {
                            AMediaCodec_queueInputBuffer(codec, index, 0, read as usize, pts, 0);
                            AMediaExtractor_advance(extractor);
                        }
                    }
                }
            }
        }

        // SAFETY: `codec` is executing; `&mut info` is a valid, aligned stack pointer to `AMediaCodecBufferInfo`.
        let out_index =
            unsafe { AMediaCodec_dequeueOutputBuffer(codec, &mut info, OUTPUT_TIMEOUT_US) };

        if out_index >= 0 {
            let index = out_index as usize;
            let is_eos = info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
            let is_config = info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG != 0;

            if !is_config && info.size > 0 && info.offset >= 0 {
                let size = info.size as usize;
                let offset = info.offset as usize;
                let mut capacity: usize = 0;
                // SAFETY: `codec` is executing, `index` is a dequeued output buffer index,
                // and `&mut capacity` is a valid stack pointer receiving the buffer size.
                let buffer = unsafe { AMediaCodec_getOutputBuffer(codec, index, &mut capacity) };
                if !buffer.is_null() && offset.checked_add(size).is_some_and(|end| end <= capacity)
                {
                    // SAFETY: `buffer` points to `capacity` bytes from the codec; `offset + size <= capacity`,
                    // so `buffer.add(offset)` is in-bounds and points to at least `size` readable bytes.
                    let base = unsafe { buffer.add(offset) };
                    append_pcm(base, size, encoding, src_channels, out_channels, &mut mixed);
                }
            }
            // SAFETY: `codec` is executing and `index` was dequeued from `AMediaCodec_dequeueOutputBuffer`.
            // `false` releases the buffer back to the codec pool without surface rendering.
            unsafe { AMediaCodec_releaseOutputBuffer(codec, index, false) };
            if is_eos {
                break;
            }
            continue;
        }

        let out_index = out_index as i32;
        if out_index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED {
            // SAFETY: `codec` is executing and signaled `AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED`.
            let out_format = unsafe { AMediaCodec_getOutputFormat(codec) };
            if !out_format.is_null() {
                // SAFETY: `out_format` is a non-null, valid `AMediaFormat` pointer returned by `getOutputFormat`.
                if let Some(r) = unsafe { format_i32(out_format, AMEDIAFORMAT_KEY_SAMPLE_RATE) }
                    .filter(|r| *r > 0)
                {
                    src_rate = r as u32;
                }
                // SAFETY: `out_format` is a valid `AMediaFormat` pointer.
                if let Some(c) = unsafe { format_i32(out_format, AMEDIAFORMAT_KEY_CHANNEL_COUNT) }
                    .filter(|c| *c > 0)
                {
                    src_channels = c as usize;
                }
                // SAFETY: `out_format` is a valid `AMediaFormat` pointer.
                if let Some(e) = unsafe { format_i32(out_format, AMEDIAFORMAT_KEY_PCM_ENCODING) } {
                    encoding = e;
                }
                // SAFETY: `out_format` was allocated by `AMediaCodec_getOutputFormat` and must be deleted.
                unsafe { AMediaFormat_delete(out_format) };
            }
        } else if out_index == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED {
            // Deprecated no-op on the NDK path; keep pumping.
        } else if input_done {
            // INFO_TRY_AGAIN_LATER after EOS was queued: give the codec a few
            // more spins, then assume the stream produced nothing further.
            // (The EOS output flag breaks the loop in the `>= 0` arm.)
        }
    }

    Ok((mixed, src_rate, src_channels))
}

/// Convert one codec output buffer (`encoding` PCM, `src_channels` interleaved)
/// to interleaved `f32` mixed to `out_channels`, appending to `mixed`.
///
/// # Safety
/// `base` must point to at least `size` readable, initialized bytes for the duration of this call.
pub(super) fn append_pcm(
    base: *const u8,
    size: usize,
    encoding: i32,
    src_channels: usize,
    out_channels: usize,
    mixed: &mut Vec<f32>,
) {
    let src_channels = src_channels.max(1);
    // Decode the raw PCM into interleaved f32 at the source channel count.
    let src: Vec<f32> = match encoding {
        ENCODING_PCM_FLOAT => {
            let count = size / 4;
            // SAFETY: `base` points to at least `size` readable bytes per caller contract;
            // `count * 4 <= size`, so reading `count * 4` bytes is in bounds.
            let bytes = unsafe { slice::from_raw_parts(base, count * 4) };
            bytes
                .chunks_exact(4)
                .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        }
        // Default to 16-bit signed, the common MediaCodec PCM output.
        _ => {
            let count = size / 2;
            // SAFETY: `base` points to at least `size` readable bytes per caller contract;
            // `count * 2 <= size`, so reading `count * 2` bytes is in bounds.
            let bytes = unsafe { slice::from_raw_parts(base, count * 2) };
            bytes
                .chunks_exact(2)
                .map(|b| i16::from_ne_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect()
        }
    };

    for frame in src.chunks_exact(src_channels) {
        for c in 0..out_channels {
            // Mono source ⇒ duplicate; multichannel ⇒ take matching channels,
            // padding the high channels from channel 0 (MVP downmix).
            let sample = if src_channels == 1 {
                frame[0]
            } else if c < src_channels {
                frame[c]
            } else {
                frame[0]
            };
            mixed.push(sample);
        }
    }
}
