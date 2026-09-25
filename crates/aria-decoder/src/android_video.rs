//! The video decoder itself: [`MediaCodecDecoder`], its feed-input /
//! drain-output pump, and the codec-format refinement that settles geometry
//! and colour before the first frame.
//!
//! Declared from `android.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

/// A pull-based decoder over one video file, backed by MediaExtractor +
/// MediaCodec.
pub struct MediaCodecDecoder {
    extractor: *mut AMediaExtractor,
    codec: *mut AMediaCodec,
    info: SourceInfo,
    /// Keeps the source fd open for the extractor's lifetime (see
    /// [`set_data_source_fd`]). Dropped after the native teardown in `Drop`.
    _source: File,
    /// Y-plane row pitch in bytes and number of luma rows in the output buffer
    /// (the codec pads both for alignment); refined from the output format.
    stride: usize,
    slice_height: usize,
    /// We've queued the end-of-stream marker into the codec's input.
    input_done: bool,
    /// We've drained the end-of-stream marker out of the codec's output.
    eos: bool,
    /// PTS of the last emitted frame — the [`crate::seek`] roll-forward anchor.
    /// Cleared on [`VideoDecoder::seek`] (decode position moved).
    last_pts: Option<RationalTime>,
    /// Observed seek/decode costs driving the adaptive roll window.
    seek_stats: crate::seek::SeekStats,
}

// SAFETY: `MediaCodecDecoder` exclusively owns its native `AMediaExtractor` and `AMediaCodec`
// handles along with the underlying file descriptor. The NDK MediaCodec and MediaExtractor APIs
// do not require thread affinity and are safe to move between threads when accessed exclusively
// through `&mut self` (no internal synchronization / `&self` sharing across threads is exposed,
// satisfying `Send` without implementing `Sync`).
unsafe impl Send for MediaCodecDecoder {}

impl MediaCodecDecoder {
    /// Open `path` and probe its first video track.
    ///
    /// Refuses [`OutputMode::Gpu`], which this backend has no surface path for.
    /// [`crate::decoder_capability`] answers the same question without a file.
    pub fn open(path: &Path, mode: OutputMode) -> Result<Self, DecodeError> {
        let capability = crate::decoder_capability();
        if !capability.supports(mode) {
            return Err(DecodeError::unsupported(crate::unsupported_mode(
                capability, mode,
            )));
        }
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
        let source = set_data_source_fd(extractor, path)?;

        // Find the first video track and keep its format.
        let (track_index, track_format, mime) = find_video_track(extractor)?;
        guard.format = track_format;

        // SAFETY: `extractor` is non-null and `track_index` was validated by `find_video_track` (< track_count).
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

        // ByteBuffer mode: null surface => decoded planes land in system memory.
        // SAFETY: `codec` is a newly created `AMediaCodec` handle and `track_format` is a valid
        // `AMediaFormat` pointer. Null surface/crypto pointers configure the codec in ByteBuffer mode.
        let config_status = unsafe {
            AMediaCodec_configure(codec, track_format, ptr::null_mut(), ptr::null_mut(), 0)
        };
        if !media_ok(config_status) {
            return Err(DecodeError::Decode("AMediaCodec_configure failed".into()));
        }
        // SAFETY: `codec` is configured and transitioned to executing state by `AMediaCodec_start`.
        let start_status = unsafe { AMediaCodec_start(codec) };
        if !media_ok(start_status) {
            return Err(DecodeError::Decode("AMediaCodec_start failed".into()));
        }
        // SAFETY: `track_format` is a valid non-null `AMediaFormat` pointer per `find_video_track`.
        let info = unsafe { probe_container(track_format) }?;
        let (stride, slice_height) = (info.coded_size.0 as usize, info.coded_size.1 as usize);

        // Track format is copied by `configure`; release our reference.
        // SAFETY: `track_format` is an owned `AMediaFormat` allocated by `getTrackFormat` and no
        // longer needed after `configure`.
        unsafe { AMediaFormat_delete(track_format) };
        guard.format = ptr::null_mut();
        let decoder = Self {
            extractor,
            codec,
            info,
            _source: source,
            stride,
            slice_height,
            input_done: false,
            eos: false,
            last_pts: None,
            seek_stats: Default::default(),
        };
        // Ownership transferred to `decoder`; don't let the guard tear it down.
        core::mem::forget(guard);
        Ok(decoder)
    }

    /// Feed one input buffer from the extractor into the codec, if a buffer is
    /// free and the source isn't exhausted.
    fn pump_input(&mut self) {
        if self.input_done {
            return;
        }
        // SAFETY: `self.codec` is non-null and in executing state. Timeout is non-blocking (0 us).
        let in_index = unsafe { AMediaCodec_dequeueInputBuffer(self.codec, INPUT_TIMEOUT_US) };
        if in_index < 0 {
            return; // no free input buffer right now
        }
        let index = in_index as usize;
        let mut capacity: usize = 0;
        // SAFETY: `self.codec` is executing, `index` was dequeued from `AMediaCodec_dequeueInputBuffer`,
        // and `&mut capacity` receives the allocated input buffer byte size.
        let buffer = unsafe { AMediaCodec_getInputBuffer(self.codec, index, &mut capacity) };
        if buffer.is_null() {
            return;
        }

        // SAFETY: `self.extractor` has a selected video track; `buffer` points to `capacity` writable bytes.
        let read = unsafe { AMediaExtractor_readSampleData(self.extractor, buffer, capacity) };
        if read < 0 {
            // End of stream: queue an empty buffer flagged EOS to flush the codec.
            // SAFETY: `index` is the currently dequeued input buffer; queues EOS marker.
            unsafe {
                AMediaCodec_queueInputBuffer(
                    self.codec,
                    index,
                    0,
                    0,
                    0,
                    AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM,
                );
            }
            self.input_done = true;
        } else {
            // SAFETY: `self.extractor` is positioned at a valid video sample (`read >= 0`).
            let pts_us = unsafe { AMediaExtractor_getSampleTime(self.extractor) }.max(0) as u64;
            // SAFETY: `read as usize <= capacity` bytes were populated in `buffer`; queues
            // buffer into `self.codec` and advances `self.extractor` to next sample.
            unsafe {
                AMediaCodec_queueInputBuffer(self.codec, index, 0, read as usize, pts_us, 0);
                AMediaExtractor_advance(self.extractor);
            }
        }
    }

    /// Copy a decoded output buffer into a CPU [`VideoFrame`].
    fn read_cpu_frame(
        &self,
        buffer: *const u8,
        capacity: usize,
        info: &AMediaCodecBufferInfo,
        pts: RationalTime,
    ) -> Result<VideoFrame, DecodeError> {
        if info.offset < 0 || info.size < 0 {
            return Err(DecodeError::Decode("negative buffer offset or size".into()));
        }
        let offset = info.offset as usize;
        let size = info.size as usize;
        if offset.checked_add(size).is_none_or(|end| end > capacity) {
            return Err(DecodeError::Decode(format!(
                "buffer range [{}..{}] exceeds buffer capacity {}",
                offset,
                offset.saturating_add(size),
                capacity
            )));
        }
        // SAFETY: `buffer` points to `capacity` bytes from the codec; `offset + size <= capacity`,
        // so `buffer.add(offset)` is in-bounds and points to at least `size` readable bytes.
        let base = unsafe { buffer.add(offset) };
        let available = size;
        // SAFETY: `base` points to `available` readable bytes within the dequeued output buffer.
        let planes = unsafe {
            read_planes(
                base,
                available,
                self.info.pixel_format,
                self.stride,
                self.slice_height,
            )
        }?;
        let visible = Rect::from_size(self.info.display_size.0, self.info.display_size.1);
        Ok(VideoFrame::new(
            pts,
            self.info.pixel_format,
            self.info.color,
            self.info.coded_size,
            visible,
            self.info.rotation,
            FrameData::Cpu(CpuImage::new(planes)),
        ))
    }
}

impl VideoDecoder for MediaCodecDecoder {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        let seek_us = to_us(target);
        // SAFETY: `self.extractor` is non-null, valid, and configured with a video track.
        // `seek_us` is a non-negative microsecond target; `PREVIOUS_SYNC` seeks to sync frame.
        let seek_status = unsafe {
            AMediaExtractor_seekTo(
                self.extractor,
                seek_us,
                SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC,
            )
        };
        if !media_ok(seek_status) {
            return Err(DecodeError::Seek(format!("seekTo {seek_us}us failed")));
        }
        // Drop the codec's in-flight buffers so the next `next_frame` decodes
        // forward from the new keyframe.
        // SAFETY: `self.codec` is in executing state; `flush` resets decoder buffer pipelines.
        let flush_status = unsafe { AMediaCodec_flush(self.codec) };
        if !media_ok(flush_status) {
            return Err(DecodeError::Seek("AMediaCodec_flush failed".into()));
        }
        self.input_done = false;
        self.eos = false;
        self.last_pts = None;
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.eos {
            return Ok(None);
        }

        let mut info = AMediaCodecBufferInfo {
            offset: 0,
            size: 0,
            presentationTimeUs: 0,
            flags: 0,
        };

        for _ in 0..MAX_PUMP_ITERS {
            self.pump_input();

            // SAFETY: `self.codec` is executing; `&mut info` is a valid, aligned stack pointer to `AMediaCodecBufferInfo`.
            let out_index = unsafe {
                AMediaCodec_dequeueOutputBuffer(self.codec, &mut info, OUTPUT_TIMEOUT_US)
            };

            if out_index >= 0 {
                let index = out_index as usize;
                let is_eos = info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
                let is_config = info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG != 0;
                if is_eos {
                    self.eos = true;
                }
                // Codec-config (CSD) and empty buffers aren't displayable frames.
                if is_config || info.size == 0 {
                    // SAFETY: `self.codec` is executing and `index` was dequeued from `AMediaCodec_dequeueOutputBuffer`.
                    // `false` releases the buffer back to the codec pool without surface rendering.
                    unsafe { AMediaCodec_releaseOutputBuffer(self.codec, index, false) };
                    if self.eos {
                        return Ok(None);
                    }
                    continue;
                }

                let mut capacity: usize = 0;
                // SAFETY: `self.codec` is executing, `index` is a dequeued output buffer index,
                // and `&mut capacity` receives the allocated buffer size in bytes.
                let buffer =
                    unsafe { AMediaCodec_getOutputBuffer(self.codec, index, &mut capacity) };
                if buffer.is_null() {
                    // SAFETY: releases dequeued output buffer on error to prevent codec pool starvation.
                    unsafe { AMediaCodec_releaseOutputBuffer(self.codec, index, false) };
                    return Err(DecodeError::Decode("null output buffer".into()));
                }
                let pts = RationalTime::new(info.presentationTimeUs, self.info.time_base);
                let frame = self.read_cpu_frame(buffer, capacity, &info, pts);
                // Release before propagating any read error so we never leak a
                // dequeued buffer back to the codec.
                // SAFETY: returns buffer back to codec pool.
                unsafe { AMediaCodec_releaseOutputBuffer(self.codec, index, false) };
                let frame = frame?;
                self.last_pts = Some(pts);
                return Ok(Some(frame));
            }

            let out_index = out_index as i32;
            if out_index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED {
                // SAFETY: `self.codec` is executing and signaled `AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED`.
                let out_format = unsafe { AMediaCodec_getOutputFormat(self.codec) };
                if !out_format.is_null() {
                    let refined = self.refine_from_output(out_format);
                    // SAFETY: `out_format` was allocated by `AMediaCodec_getOutputFormat` and must be deleted.
                    unsafe { AMediaFormat_delete(out_format) };
                    refined?;
                }
            } else if out_index == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED {
                // Deprecated no-op on the NDK path; keep pumping.
            } else {
                // INFO_TRY_AGAIN_LATER: nothing ready yet.
                if self.input_done && self.eos {
                    return Ok(None);
                }
            }
        }

        Err(DecodeError::Decode(
            "decode pump exceeded its iteration budget without producing a frame".into(),
        ))
    }

    fn frame_at(&mut self, target: RationalTime) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        // Copied out and written back: `frame_at_rolling` borrows `self` for
        // the decode walk, so it can't also borrow the field.
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }

    fn frame_at_nearest(
        &mut self,
        target: RationalTime,
    ) -> Result<Option<VideoFrame>, DecodeError> {
        let last_pts = self.last_pts;
        let mut stats = self.seek_stats;
        let result = crate::seek::frame_at_nearest_rolling(self, last_pts, &mut stats, target);
        self.seek_stats = stats;
        result
    }
}

impl MediaCodecDecoder {
    /// Update decode geometry, pixel format, and color from the codec's settled
    /// output format (emitted as `INFO_OUTPUT_FORMAT_CHANGED` before frame one).
    fn refine_from_output(&mut self, format: *mut AMediaFormat) -> Result<(), DecodeError> {
        // SAFETY: `format` is a non-null, valid `AMediaFormat` pointer returned by `getOutputFormat`.
        // `AMEDIAFORMAT_KEY_COLOR_FORMAT` is an NDK static null-terminated C string constant.
        if let Some(color_format) = unsafe { format_i32(format, AMEDIAFORMAT_KEY_COLOR_FORMAT) } {
            self.info.pixel_format = color_format_to_pixel(color_format).ok_or_else(|| {
                DecodeError::unsupported(format!("MediaCodec color format {color_format:#x}"))
            })?;
        }
        // SAFETY: `format` is valid; `AMEDIAFORMAT_KEY_WIDTH` is an NDK static C string.
        let width = unsafe { format_i32(format, AMEDIAFORMAT_KEY_WIDTH) }
            .filter(|w| *w > 0)
            .map(|w| w as u32)
            .unwrap_or(self.info.display_size.0);
        // SAFETY: `format` is valid; `AMEDIAFORMAT_KEY_HEIGHT` is an NDK static C string.
        let height = unsafe { format_i32(format, AMEDIAFORMAT_KEY_HEIGHT) }
            .filter(|h| *h > 0)
            .map(|h| h as u32)
            .unwrap_or(self.info.display_size.1);

        // SAFETY: `format` is valid; `AMEDIAFORMAT_KEY_STRIDE` is an NDK static C string.
        let stride = unsafe { format_i32(format, AMEDIAFORMAT_KEY_STRIDE) }
            .filter(|s| *s > 0)
            .map(|s| s as usize)
            .unwrap_or(width as usize);
        // SAFETY: `format` is valid; `AMEDIAFORMAT_KEY_SLICE_HEIGHT` is an NDK static C string.
        let slice_height = unsafe { format_i32(format, AMEDIAFORMAT_KEY_SLICE_HEIGHT) }
            .filter(|s| *s > 0)
            .map(|s| s as usize)
            .unwrap_or(height as usize);

        self.stride = stride.max(width as usize);
        self.slice_height = slice_height.max(height as usize);
        self.info.coded_size = (self.stride as u32, self.slice_height as u32);
        self.info.display_size = (width, height);
        // SAFETY: `format` is a valid non-null AMediaFormat pointer.
        self.info.color = unsafe { color_from_format(format) };
        Ok(())
    }
}

impl Drop for MediaCodecDecoder {
    fn drop(&mut self) {
        if !self.codec.is_null() {
            // SAFETY: `self.codec` is a non-null, owned `AMediaCodec` handle. `AMediaCodec_stop`
            // transitions it out of executing state so `AMediaCodec_delete` can safely release it.
            unsafe {
                AMediaCodec_stop(self.codec);
                AMediaCodec_delete(self.codec);
            }
        }
        if !self.extractor.is_null() {
            // SAFETY: `self.extractor` is a non-null, owned `AMediaExtractor` handle allocated via
            // `AMediaExtractor_new()` and not yet deleted.
            unsafe {
                AMediaExtractor_delete(self.extractor);
            }
        }
    }
}
