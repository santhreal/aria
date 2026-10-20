//! Decoder and encoder callbacks: probing, opening, and reading decoder
//! streams, and opening, feeding, and finishing encoder sessions.

use parking_lot::Mutex;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_plugin_abi::{
    AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaInstanceResult, AriaResult, AriaStr,
    AriaVideoFrameOut, AriaVideoInfo,
};

use crate::{
    AudioStream, DecodeFailure, Decoder, EncoderAudio, EncoderConfig, EncoderFrame, FramePlane,
    Plugin, VideoStream,
};

use super::{PluginRuntime, panic_message};

/// A stream a registered decoder opened, locked on its own so streams decode
/// in parallel.
pub(super) enum DecoderInstance {
    Video(Arc<Mutex<Box<dyn VideoStream>>>),
    Audio(Arc<Mutex<Box<dyn AudioStream>>>),
}

fn failure_result(failure: DecodeFailure) -> AriaResult {
    match failure {
        DecodeFailure::Unsupported(reason) => AriaResult::unsupported(reason),
        DecodeFailure::Failed(reason) => AriaResult::err(reason),
    }
}

fn failure_instance(failure: DecodeFailure) -> AriaInstanceResult {
    match failure {
        DecodeFailure::Unsupported(reason) => AriaInstanceResult::unsupported(reason),
        DecodeFailure::Failed(reason) => AriaInstanceResult::err(reason),
    }
}

impl<P: Plugin> PluginRuntime<P> {
    fn decoder(&self, id: &str) -> Result<Arc<dyn Decoder>, DecodeFailure> {
        self.decoders
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| DecodeFailure::Failed(format!("unknown decoder: {id}")))
    }

    fn video_stream(&self, instance: u64) -> Result<Arc<Mutex<Box<dyn VideoStream>>>, String> {
        match self.decoder_instances.lock().get(&instance) {
            Some(DecoderInstance::Video(stream)) => Ok(Arc::clone(stream)),
            _ => Err(format!("unknown video stream {instance}")),
        }
    }

    fn audio_stream(&self, instance: u64) -> Result<Arc<Mutex<Box<dyn AudioStream>>>, String> {
        match self.decoder_instances.lock().get(&instance) {
            Some(DecoderInstance::Audio(stream)) => Ok(Arc::clone(stream)),
            _ => Err(format!("unknown audio stream {instance}")),
        }
    }

    fn open_instance(&self, instance: DecoderInstance) -> u64 {
        let id = self.decoder_next_id.fetch_add(1, Ordering::Relaxed);
        self.decoder_instances.lock().insert(id, instance);
        id
    }

    /// Open an encoder session for `encoder_id` writing `config_ptr` to `path`.
    ///
    /// # Safety
    ///
    /// `config_ptr` must be non-null and point to a valid [`AriaEncoderConfig`] for the
    /// duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_open(
        &self,
        host_ctx: *mut c_void,
        encoder_id: AriaStr,
        path: AriaStr,
        config_ptr: *const AriaEncoderConfig,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if config_ptr.is_null() {
                return Err("encoder_open received null config".to_owned());
            }
            // SAFETY: `config_ptr` is valid for the duration of `encoder_open`.
            let raw_cfg = unsafe { &*config_ptr };
            let config = EncoderConfig {
                width: raw_cfg.width,
                height: raw_cfg.height,
                fps_num: raw_cfg.fps_num,
                fps_den: raw_cfg.fps_den,
                video_bitrate_kbps: raw_cfg.video_bitrate_kbps,
                audio_bitrate_kbps: raw_cfg.audio_bitrate_kbps,
            };
            let host = self.host_for(host_ctx)?;
            let encoder_id = Self::str_arg(encoder_id, "encoder id")?;
            let path_str = Self::str_arg(path, "encoder path")?;
            let registered = self.encoders.lock().get(encoder_id).cloned();
            let session = match registered {
                Some(encoder) => encoder.open(&host, Path::new(path_str), &config)?,
                None => {
                    self.plugin
                        .encoder_open(&host, encoder_id, Path::new(path_str), &config)?
                }
            };
            let id = self.encoder_next_id.fetch_add(1, Ordering::Relaxed);
            self.encoder_instances.lock().insert(id, session);
            Ok::<u64, String>(id)
        })) {
            Ok(Ok(id)) => AriaInstanceResult::ok(id),
            Ok(Err(err)) => AriaInstanceResult::err(err),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Push one RGBA video frame to encoder `instance`.
    ///
    /// # Safety
    ///
    /// `frame_ptr` must be non-null and point to a valid [`AriaEncoderFrame`] whose
    /// `rgba` pointer covers `rgba_len` bytes for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_push_frame(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame_ptr: *const AriaEncoderFrame,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if frame_ptr.is_null() {
                return Err("encoder_push_frame received null frame".to_owned());
            }
            // SAFETY: `frame_ptr` is valid for the duration of `encoder_push_frame`.
            let raw = unsafe { &*frame_ptr };
            let rgba = if raw.rgba_len == 0 || raw.rgba.is_null() {
                &[][..]
            } else {
                // SAFETY: `raw.rgba` points to `raw.rgba_len` valid bytes.
                unsafe { std::slice::from_raw_parts(raw.rgba, raw.rgba_len) }
            };
            let frame = EncoderFrame {
                rgba,
                width: raw.width,
                height: raw.height,
                stride: raw.stride,
                pts_value: raw.pts_value,
                pts_rate: raw.pts_rate,
            };
            let mut map = self.encoder_instances.lock();
            let session = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.push_frame(&frame)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Push one interleaved `f32` audio block to encoder `instance`.
    ///
    /// # Safety
    ///
    /// `audio_ptr` must be non-null and point to a valid [`AriaEncoderAudio`] whose
    /// `samples` pointer covers `samples_len` `f32` elements for the duration of the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_encoder_push_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        audio_ptr: *const AriaEncoderAudio,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            if audio_ptr.is_null() {
                return Err("encoder_push_audio received null audio".to_owned());
            }
            // SAFETY: `audio_ptr` is valid for the duration of `encoder_push_audio`.
            let raw = unsafe { &*audio_ptr };
            let samples = if raw.samples_len == 0 || raw.samples.is_null() {
                &[][..]
            } else {
                // SAFETY: `raw.samples` points to `raw.samples_len` valid f32 samples.
                unsafe { std::slice::from_raw_parts(raw.samples, raw.samples_len) }
            };
            let audio = EncoderAudio {
                samples,
                sample_rate: raw.sample_rate,
                channels: raw.channels,
                pts_value: raw.pts_value,
                pts_rate: raw.pts_rate,
            };
            let mut map = self.encoder_instances.lock();
            let session = map
                .get_mut(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.push_audio(&audio)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Finalize encoder `instance` and remove it from the active table.
    pub fn handle_encoder_finish(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let session = self
                .encoder_instances
                .lock()
                .remove(&instance)
                .ok_or_else(|| format!("unknown encoder instance {instance}"))?;
            session.finish()
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Drop encoder `instance` without finishing (for example on cancellation).
    pub fn handle_encoder_drop(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let removed = self.encoder_instances.lock().remove(&instance);
            drop(removed);
            Ok::<(), String>(())
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Describe `path` with decoder `decoder_id`, as probe JSON.
    pub fn handle_decoder_probe(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let probe = decoder.probe(&host, Path::new(path))?;
            serde_json::to_string(&probe).map_err(|err| DecodeFailure::Failed(err.to_string()))
        })) {
            Ok(Ok(json)) => AriaResult::ok(json),
            Ok(Err(failure)) => failure_result(failure),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Open the video stream of `path` with decoder `decoder_id`, writing its
    /// description into `info`.
    ///
    /// # Safety
    ///
    /// `info` must be null or point to an [`AriaVideoInfo`] writable for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_open_video(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        info: *mut AriaVideoInfo,
    ) -> AriaInstanceResult {
        if info.is_null() {
            return AriaInstanceResult::err("decoder_open_video received a null info");
        }
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let (stream_info, stream) = decoder.open_video(&host, Path::new(path))?;
            let instance = self.open_instance(DecoderInstance::Video(Arc::new(Mutex::new(stream))));
            Ok::<_, DecodeFailure>((instance, stream_info.to_raw()))
        })) {
            Ok(Ok((instance, raw))) => {
                // SAFETY: `info` is non-null and the caller guarantees it is writable.
                unsafe { info.write(raw) };
                AriaInstanceResult::ok(instance)
            }
            Ok(Err(failure)) => failure_instance(failure),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Seek video stream `instance` to the sync frame at or before `pts`.
    pub fn handle_decoder_seek(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        pts: i64,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            self.video_stream(instance)?.lock().seek(pts)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Fill the planes of `frame` with the next frame of video stream `instance`.
    ///
    /// # Safety
    ///
    /// `frame` must be null or point to an [`AriaVideoFrameOut`] writable for
    /// the call, whose `planes` hold `planes_len` planes, each `data` covering
    /// `len` writable bytes.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_next_frame(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame: *mut AriaVideoFrameOut,
    ) -> AriaResult {
        if frame.is_null() {
            return AriaResult::err("decoder_next_frame received a null frame");
        }
        // SAFETY: `frame` is non-null and the caller guarantees it is writable.
        let out = unsafe { &mut *frame };
        let raw_planes = if out.planes.is_null() || out.planes_len == 0 {
            &[][..]
        } else {
            // SAFETY: `out.planes` holds `out.planes_len` planes for the call.
            unsafe { std::slice::from_raw_parts(out.planes, out.planes_len) }
        };
        let mut planes: Vec<FramePlane<'_>> = raw_planes
            .iter()
            .map(|plane| FramePlane {
                data: if plane.data.is_null() || plane.len == 0 {
                    &mut []
                } else {
                    // SAFETY: each plane's `data` covers `len` writable bytes for the call,
                    // and no two planes overlap.
                    unsafe { std::slice::from_raw_parts_mut(plane.data, plane.len) }
                },
                stride: plane.stride as usize,
                rows: plane.rows as usize,
            })
            .collect();
        match catch_unwind(AssertUnwindSafe(|| {
            self.video_stream(instance)?.lock().next_frame(&mut planes)
        })) {
            Ok(Ok(Some(pts))) => {
                out.pts = pts;
                out.produced = 1;
                AriaResult::ok_empty()
            }
            Ok(Ok(None)) => {
                out.produced = 0;
                AriaResult::ok_empty()
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Open the audio of `path` with decoder `decoder_id`, resampled to
    /// `sample_rate` and `channels`.
    pub fn handle_decoder_open_audio(
        &self,
        host_ctx: *mut c_void,
        decoder_id: AriaStr,
        path: AriaStr,
        sample_rate: u32,
        channels: u32,
    ) -> AriaInstanceResult {
        match catch_unwind(AssertUnwindSafe(|| {
            let host = self.host_for(host_ctx)?;
            let decoder = self.decoder(Self::str_arg(decoder_id, "decoder id")?)?;
            let path = Self::str_arg(path, "decoder path")?;
            let channels = u16::try_from(channels)
                .map_err(|_| DecodeFailure::Failed(format!("{channels} channels is too many")))?;
            let stream = decoder.open_audio(&host, Path::new(path), sample_rate, channels)?;
            Ok::<_, DecodeFailure>(
                self.open_instance(DecoderInstance::Audio(Arc::new(Mutex::new(stream)))),
            )
        })) {
            Ok(Ok(instance)) => AriaInstanceResult::ok(instance),
            Ok(Err(failure)) => failure_instance(failure),
            Err(payload) => AriaInstanceResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Read interleaved samples of audio stream `instance` into `out`.
    ///
    /// # Safety
    ///
    /// `out` must cover `out_len` writable samples and `written` must be null
    /// or writable for the call.
    #[allow(unsafe_code)]
    pub unsafe fn handle_decoder_read_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        out: *mut f32,
        out_len: usize,
        written: *mut usize,
    ) -> AriaResult {
        if written.is_null() {
            return AriaResult::err("decoder_read_audio received a null count");
        }
        let samples = if out.is_null() || out_len == 0 {
            &mut [][..]
        } else {
            // SAFETY: `out` covers `out_len` writable samples for the call.
            unsafe { std::slice::from_raw_parts_mut(out, out_len) }
        };
        match catch_unwind(AssertUnwindSafe(|| {
            let read = self.audio_stream(instance)?.lock().read(samples)?;
            if read > out_len {
                return Err(format!("read {read} samples into {out_len}"));
            }
            Ok(read)
        })) {
            Ok(Ok(read)) => {
                // SAFETY: `written` is non-null and the caller guarantees it is writable.
                unsafe { written.write(read) };
                AriaResult::ok_empty()
            }
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Position the next read of audio stream `instance` at sample frame `frame`.
    pub fn handle_decoder_seek_audio(
        &self,
        _host_ctx: *mut c_void,
        instance: u64,
        frame: i64,
    ) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            self.audio_stream(instance)?.lock().seek(frame)
        })) {
            Ok(Ok(())) => AriaResult::ok_empty(),
            Ok(Err(err)) => AriaResult::err(err),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }

    /// Drop the video or audio stream `instance`.
    pub fn handle_decoder_close(&self, _host_ctx: *mut c_void, instance: u64) -> AriaResult {
        match catch_unwind(AssertUnwindSafe(|| {
            drop(self.decoder_instances.lock().remove(&instance));
        })) {
            Ok(()) => AriaResult::ok_empty(),
            Err(payload) => AriaResult::panic(panic_message(payload.as_ref())),
        }
    }
}
