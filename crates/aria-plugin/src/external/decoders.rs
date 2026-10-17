//! Decoders a native plugin registers through `register_decoder`: the host
//! side of the `decoder_*` entries of [`aria_plugin_abi::AriaPluginV1`].

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

use aria_core::{ColorSpace, PixelFormat, Plane, Rational};
use aria_decoder::MediaProbe;
use aria_plugin_abi::{
    ARIA_COLOR_BT601, ARIA_COLOR_BT709, ARIA_COLOR_BT2020_PQ, ARIA_COLOR_SRGB, ARIA_FORMAT_NV12,
    ARIA_FORMAT_YUV420P, ARIA_STATUS_OK, ARIA_STATUS_PANIC, ARIA_STATUS_UNSUPPORTED,
    AriaFramePlane, AriaInstanceResult, AriaResult, AriaStr, AriaVideoFrameOut, AriaVideoInfo,
};
use serde::Deserialize;

use super::{HostCallContext, NativePluginState};
use crate::context::PluginContext;
use crate::decoders::DecoderSpec;
use crate::foreign_decoder::{CallError, DecodedPlanes, DecoderCalls, ForeignDecoder, StreamInfo};

#[derive(Deserialize)]
struct RawDecoderSpec {
    id: String,
    name: String,
    extensions: Vec<String>,
    #[serde(default)]
    priority: i32,
}

/// The `decoder_probe` value: [`MediaProbe`] as JSON.
#[derive(Deserialize)]
struct RawProbe {
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    frame_rate_num: i32,
    frame_rate_den: i32,
    #[serde(default)]
    frame_count: i64,
    #[serde(default)]
    has_audio: bool,
    #[serde(default)]
    is_image: bool,
    #[serde(default)]
    video_codec: Option<String>,
    #[serde(default)]
    audio_codec: Option<String>,
}

/// Register the decoder `spec_json` states for the plugin `ctx` is mounting.
pub(super) fn register(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    let raw: RawDecoderSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid decoder spec JSON: {err}"))?;
    let decoder = NativeDecoder {
        id: raw.id.clone(),
        state: Arc::clone(ctx.state),
        ctx: ctx.scoped_ctx.clone(),
    };
    ctx.scoped_ctx
        .register_decoder(
            DecoderSpec {
                id: raw.id,
                name: raw.name,
                extensions: raw.extensions,
                priority: raw.priority,
            },
            ForeignDecoder::new(decoder),
        )
        .map(drop)
        .map_err(|err| err.to_string())
}

struct NativeDecoder {
    id: String,
    state: Arc<NativePluginState>,
    ctx: PluginContext,
}

impl NativeDecoder {
    /// Run `call` with a host context for the plugin, or refuse when the
    /// plugin has faulted.
    fn call<R>(&self, call: impl FnOnce(*mut c_void) -> R) -> Result<R, CallError> {
        if self.state.is_failed() {
            return Err(CallError::Failed(format!(
                "plugin `{}` has faulted",
                self.state.id
            )));
        }
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.ctx,
            unscoped_ctx: self.ctx.scoped(None),
            in_mount: false,
        };
        Ok(call((&raw mut call_ctx).cast::<c_void>()))
    }

    /// The outcome of a call that returned `status` with `text`. A panic
    /// fails the plugin for the session.
    fn outcome(&self, status: u32, text: String) -> Result<String, CallError> {
        match status {
            ARIA_STATUS_OK => Ok(text),
            ARIA_STATUS_UNSUPPORTED => Err(CallError::Unsupported(text)),
            ARIA_STATUS_PANIC => {
                let reason = format!("decoder {} panicked: {text}", self.id);
                self.state.mark_faulted(&self.ctx, reason.clone());
                Err(CallError::Failed(reason))
            }
            _ => Err(CallError::Failed(text)),
        }
    }

    fn result(&self, result: AriaResult) -> Result<String, CallError> {
        let text = self.state.take_guest_string(result.value);
        self.outcome(result.status, text)
    }

    fn instance(&self, result: AriaInstanceResult) -> Result<u64, CallError> {
        let text = self.state.take_guest_string(result.error);
        self.outcome(result.status, text).map(|_| result.instance)
    }
}

fn stream_info(raw: &AriaVideoInfo) -> Result<StreamInfo, String> {
    let format = match raw.format {
        ARIA_FORMAT_NV12 => PixelFormat::Nv12,
        ARIA_FORMAT_YUV420P => PixelFormat::Yuv420p,
        other => {
            return Err(format!(
                "the stream format {other} is not an ARIA_FORMAT_* value"
            ));
        }
    };
    let color = match raw.color {
        ARIA_COLOR_BT709 => ColorSpace::BT709,
        ARIA_COLOR_BT601 => ColorSpace::BT601,
        ARIA_COLOR_BT2020_PQ => ColorSpace::BT2020_PQ,
        ARIA_COLOR_SRGB => ColorSpace::SRGB,
        other => {
            return Err(format!(
                "the stream color {other} is not an ARIA_COLOR_* value"
            ));
        }
    };
    Ok(StreamInfo {
        width: raw.width,
        height: raw.height,
        format,
        color,
        frame_rate: Rational::new(raw.frame_rate_num, raw.frame_rate_den),
        time_base: Rational::new(raw.time_base_num, raw.time_base_den),
        duration: (raw.duration >= 0).then_some(raw.duration),
    })
}

impl DecoderCalls for NativeDecoder {
    fn probe(&self, path: &Path) -> Result<MediaProbe, CallError> {
        let path = path.to_string_lossy();
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx`, the id, and the path outlive `decoder_probe`.
            unsafe {
                (self.state.table.decoder_probe)(
                    host_ctx,
                    AriaStr::from_str(&self.id),
                    AriaStr::from_str(&path),
                )
            }
        })?;
        let json = self.result(raw)?;
        let probe: RawProbe = serde_json::from_str(&json)
            .map_err(|err| CallError::Failed(format!("invalid probe JSON: {err}")))?;
        Ok(MediaProbe {
            width: probe.width,
            height: probe.height,
            frame_rate: Rational::new(probe.frame_rate_num, probe.frame_rate_den),
            frame_count: probe.frame_count,
            has_audio: probe.has_audio,
            is_image: probe.is_image,
            video_codec: probe.video_codec,
            audio_codec: probe.audio_codec,
        })
    }

    fn open_video(&self, path: &Path) -> Result<(u64, StreamInfo), CallError> {
        let path_text = path.to_string_lossy();
        let mut info = AriaVideoInfo::default();
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx`, the id, the path, and `info` outlive `decoder_open_video`.
            unsafe {
                (self.state.table.decoder_open_video)(
                    host_ctx,
                    AriaStr::from_str(&self.id),
                    AriaStr::from_str(&path_text),
                    &raw mut info,
                )
            }
        })?;
        let instance = self.instance(raw)?;
        match stream_info(&info) {
            Ok(stream) => Ok((instance, stream)),
            Err(reason) => {
                self.close(path, instance);
                Err(CallError::Failed(reason))
            }
        }
    }

    fn seek(&self, _path: &Path, instance: u64, pts: i64) -> Result<(), CallError> {
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx` outlives `decoder_seek`.
            unsafe { (self.state.table.decoder_seek)(host_ctx, instance, pts) }
        })?;
        self.result(raw).map(drop)
    }

    fn next_frame(
        &self,
        _path: &Path,
        instance: u64,
        stream: &StreamInfo,
    ) -> Result<Option<DecodedPlanes>, CallError> {
        let shapes: Vec<(usize, usize)> = (0..stream.format.plane_count())
            .map(|index| stream.plane_shape(index))
            .collect();
        let mut buffers: Vec<Vec<u8>> = shapes
            .iter()
            .map(|&(row_bytes, rows)| vec![0; row_bytes * rows])
            .collect();
        let mut planes: Vec<AriaFramePlane> = buffers
            .iter_mut()
            .zip(&shapes)
            .map(|(buffer, &(row_bytes, rows))| AriaFramePlane {
                data: buffer.as_mut_ptr(),
                len: buffer.len(),
                // Both fit: `StreamInfo` sides are at most 16384.
                stride: row_bytes as u32,
                rows: rows as u32,
            })
            .collect();
        let mut frame = AriaVideoFrameOut {
            planes: planes.as_mut_ptr(),
            planes_len: planes.len(),
            pts: 0,
            produced: 0,
        };
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx`, `frame`, and the planes and buffers it points to outlive
            // `decoder_next_frame`; each plane's `len` is its buffer's length.
            unsafe { (self.state.table.decoder_next_frame)(host_ctx, instance, &raw mut frame) }
        })?;
        self.result(raw)?;
        if frame.produced == 0 {
            return Ok(None);
        }
        Ok(Some(DecodedPlanes {
            pts: frame.pts,
            planes: buffers
                .into_iter()
                .zip(shapes)
                .map(|(data, (row_bytes, rows))| Plane::new(data, row_bytes, rows))
                .collect(),
        }))
    }

    fn open_audio(&self, path: &Path, sample_rate: u32, channels: u16) -> Result<u64, CallError> {
        let path = path.to_string_lossy();
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx`, the id, and the path outlive `decoder_open_audio`.
            unsafe {
                (self.state.table.decoder_open_audio)(
                    host_ctx,
                    AriaStr::from_str(&self.id),
                    AriaStr::from_str(&path),
                    sample_rate,
                    u32::from(channels),
                )
            }
        })?;
        self.instance(raw)
    }

    fn read_audio(&self, _path: &Path, instance: u64, out: &mut [f32]) -> Result<usize, CallError> {
        let mut written = 0usize;
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx`, `out`, and `written` outlive `decoder_read_audio`, and
            // `out` holds `out.len()` samples.
            unsafe {
                (self.state.table.decoder_read_audio)(
                    host_ctx,
                    instance,
                    out.as_mut_ptr(),
                    out.len(),
                    &raw mut written,
                )
            }
        })?;
        self.result(raw).map(|_| written)
    }

    fn seek_audio(&self, _path: &Path, instance: u64, frame: i64) -> Result<(), CallError> {
        let raw = self.call(|host_ctx| {
            // SAFETY: `host_ctx` outlives `decoder_seek_audio`.
            unsafe { (self.state.table.decoder_seek_audio)(host_ctx, instance, frame) }
        })?;
        self.result(raw).map(drop)
    }

    fn close(&self, _path: &Path, instance: u64) {
        if let Ok(raw) = self.call(|host_ctx| {
            // SAFETY: `host_ctx` outlives `decoder_close`.
            unsafe { (self.state.table.decoder_close)(host_ctx, instance) }
        }) {
            // A refusal to close leaves nothing for the host to do.
            let _ = self.result(raw);
        }
    }
}
