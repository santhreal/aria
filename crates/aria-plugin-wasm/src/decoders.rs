//! Decoders a component registers through `register-decoder`: the host side of
//! the world's `decoder-*` exports.

use std::path::Path;
use std::sync::Arc;

use aria_plugin::{
    CallError, ColorSpace, DecodedPlanes, DecoderCalls, MediaProbe, PixelFormat, Plane,
    PluginContext, Rational, StreamInfo,
};

use crate::bindings;
use crate::host::WasmPluginShared;

pub(crate) struct WasmDecoder {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl WasmDecoder {
    pub(crate) fn new(shared: Arc<WasmPluginShared>, ctx: PluginContext, id: String) -> Self {
        Self { shared, ctx, id }
    }
}

fn stream_info(info: &bindings::VideoInfo) -> StreamInfo {
    StreamInfo {
        width: info.width,
        height: info.height,
        format: match info.format {
            bindings::FrameFormat::Nv12 => PixelFormat::Nv12,
            bindings::FrameFormat::Yuv420p => PixelFormat::Yuv420p,
        },
        color: match info.color {
            bindings::FrameColor::Bt709 => ColorSpace::BT709,
            bindings::FrameColor::Bt601 => ColorSpace::BT601,
            bindings::FrameColor::Bt2020Pq => ColorSpace::BT2020_PQ,
            bindings::FrameColor::Srgb => ColorSpace::SRGB,
        },
        frame_rate: Rational::new(info.frame_rate_num, info.frame_rate_den),
        time_base: Rational::new(info.time_base_num, info.time_base_den),
        duration: info.duration,
    }
}

impl DecoderCalls for WasmDecoder {
    fn probe(&self, path: &Path) -> Result<MediaProbe, CallError> {
        let probe = self.shared.call_decoder_probe(&self.ctx, &self.id, path)?;
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
        let opened = self
            .shared
            .call_decoder_open_video(&self.ctx, &self.id, path)?;
        Ok((opened.instance, stream_info(&opened.info)))
    }

    fn seek(&self, path: &Path, instance: u64, pts: i64) -> Result<(), CallError> {
        self.shared
            .call_decoder_seek(&self.ctx, path, instance, pts)
    }

    fn next_frame(
        &self,
        path: &Path,
        instance: u64,
        stream: &StreamInfo,
    ) -> Result<Option<DecodedPlanes>, CallError> {
        let Some(frame) = self
            .shared
            .call_decoder_next_frame(&self.ctx, path, instance)?
        else {
            return Ok(None);
        };
        let planes = frame
            .planes
            .into_iter()
            .enumerate()
            .map(|(index, data)| {
                let (row_bytes, rows) = stream.plane_shape(index);
                if data.len() != row_bytes * rows {
                    return Err(CallError::Failed(format!(
                        "plane {index} holds {} bytes; {rows} packed rows of {row_bytes} bytes are {}",
                        data.len(),
                        row_bytes * rows
                    )));
                }
                Ok(Plane::new(data, row_bytes, rows))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(DecodedPlanes {
            pts: frame.pts,
            planes,
        }))
    }

    fn open_audio(&self, path: &Path, sample_rate: u32, channels: u16) -> Result<u64, CallError> {
        self.shared
            .call_decoder_open_audio(&self.ctx, &self.id, path, sample_rate, channels)
    }

    fn read_audio(&self, path: &Path, instance: u64, out: &mut [f32]) -> Result<usize, CallError> {
        let max = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let samples = self
            .shared
            .call_decoder_read_audio(&self.ctx, path, instance, max)?;
        let Some(dest) = out.get_mut(..samples.len()) else {
            return Err(CallError::Failed(format!(
                "decoder-read-audio returned {} samples for {} asked",
                samples.len(),
                out.len()
            )));
        };
        dest.copy_from_slice(&samples);
        Ok(samples.len())
    }

    fn seek_audio(&self, path: &Path, instance: u64, frame: i64) -> Result<(), CallError> {
        self.shared
            .call_decoder_seek_audio(&self.ctx, path, instance, frame)
    }

    fn close(&self, path: &Path, instance: u64) {
        self.shared.call_decoder_close(&self.ctx, path, instance);
    }
}
