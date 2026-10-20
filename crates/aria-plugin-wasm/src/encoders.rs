//! Encoders a component registers through `register-encoder`: the host side of
//! the world's `encoder-*` exports.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_plugin::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, EncodeError, Encoder, EncoderConfig, EncoderFrame,
    PluginContext, RationalTime, VideoEncoder, VideoFrame, encoder_pts,
};

use crate::bindings;
use crate::host::WasmPluginShared;

pub(crate) struct WasmEncoder {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl WasmEncoder {
    pub(crate) fn new(shared: Arc<WasmPluginShared>, ctx: PluginContext, id: String) -> Self {
        Self { shared, ctx, id }
    }
}

impl Encoder for WasmEncoder {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let (width, height) = config.size;
        let (audio_sample_rate, audio_channels) = config.audio.map_or(
            (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS)),
            |audio| (audio.sample_rate, u32::from(audio.channels)),
        );
        let wit_cfg = bindings::EncoderConfig {
            width,
            height,
            fps_num: i64::from(config.frame_rate.num),
            fps_den: config.frame_rate.den,
            video_bitrate_kbps: config.bitrate_kbps.unwrap_or(0),
            audio_bitrate_kbps: 0,
        };
        let instance = self
            .shared
            .call_encoder_open(&self.ctx, &self.id, path, &wit_cfg)
            .map_err(EncodeError::Start)?;
        Ok(Box::new(WasmEncoderSession {
            shared: Arc::clone(&self.shared),
            ctx: self.ctx.clone(),
            path: path.to_path_buf(),
            id: self.id.clone(),
            instance,
            audio_sample_rate,
            audio_channels,
            finished: false,
        }))
    }
}

/// The `encoder-frame` the guest receives for `frame`.
fn wit_frame(frame: &VideoFrame) -> Result<bindings::EncoderFrame, EncodeError> {
    let frame = EncoderFrame::new(frame)?;
    Ok(bindings::EncoderFrame {
        rgba: frame.rgba.to_vec(),
        width: frame.width,
        height: frame.height,
        stride: frame.stride,
        pts_value: frame.pts.value,
        pts_rate: frame.pts.rate.num,
    })
}

/// The `encoder-audio` the guest receives for one interleaved block at `pts`.
fn wit_audio(
    samples: &[f32],
    sample_rate: u32,
    channels: u32,
    pts: RationalTime,
) -> Result<bindings::EncoderAudio, EncodeError> {
    let pts = encoder_pts(pts)?;
    Ok(bindings::EncoderAudio {
        samples: samples.to_vec(),
        sample_rate,
        channels,
        pts_value: pts.value,
        pts_rate: pts.rate.num,
    })
}

struct WasmEncoderSession {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    path: PathBuf,
    id: String,
    instance: u64,
    audio_sample_rate: u32,
    audio_channels: u32,
    finished: bool,
}

impl VideoEncoder for WasmEncoderSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let wit_frame = wit_frame(frame)?;
        self.shared
            .call_encoder_push_frame(&self.ctx, &self.path, self.instance, &wit_frame)
            .map_err(EncodeError::Encode)
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let wit_audio = wit_audio(samples, self.audio_sample_rate, self.audio_channels, pts)?;
        self.shared
            .call_encoder_push_audio(&self.ctx, &self.path, self.instance, &wit_audio)
            .map_err(EncodeError::Encode)
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        self.finished = true;
        self.shared
            .call_encoder_finish(&self.ctx, &self.path, self.instance)
            .map_err(EncodeError::Encode)
    }

    fn choice(&self) -> aria_plugin::EncoderChoice {
        aria_plugin::EncoderChoice::only(self.id.clone(), None)
    }
}

impl Drop for WasmEncoderSession {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.shared.call_encoder_drop(&self.ctx, self.instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use aria_core::{
        ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, Rect, Rotation,
    };

    use super::*;

    fn rgba_frame(pts: RationalTime, format: PixelFormat) -> VideoFrame {
        let plane = Plane::new((0..32).collect(), 16, 2);
        VideoFrame::new(
            pts,
            format,
            ColorSpace::BT709,
            (4, 2),
            Rect::from_size(4, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![plane])),
        )
    }

    /// A guest reads `pts-value / pts-rate` seconds, so a frame at a
    /// fractional timeline rate arrives as whole-rate ticks: frame 3 at
    /// 30000/1001 starts 3003/30000 s in, not 3/30000 s.
    #[test]
    fn frames_and_audio_reach_the_guest_at_their_instant() {
        for (value, rate, ticks) in [
            (0, Rational::FPS_24, 0),
            (48, Rational::FPS_24, 48),
            (3, Rational::FPS_29_97, 3003),
            (7, Rational::FPS_23_976, 7007),
            (2, Rational::FPS_59_94, 2002),
        ] {
            let frame = wit_frame(&rgba_frame(
                RationalTime::new(value, rate),
                PixelFormat::Rgba8,
            ))
            .expect("RGBA frame");
            assert_eq!(
                (frame.pts_value, frame.pts_rate),
                (ticks, rate.num),
                "{value} at {rate:?}"
            );
            assert_eq!((frame.width, frame.height, frame.stride), (4, 2, 16));
            assert_eq!(frame.rgba, (0..32).collect::<Vec<u8>>());
        }
        for (pts, ticks) in [
            (
                RationalTime::new(96_000, Rational::new(48_000, 1)),
                (96_000, 48_000),
            ),
            (RationalTime::new(5, Rational::FPS_29_97), (5005, 30_000)),
        ] {
            let audio = wit_audio(&[0.5, -0.5], 48_000, 2, pts).expect("audio block");
            assert_eq!((audio.pts_value, audio.pts_rate), ticks, "{pts:?}");
            assert_eq!((audio.sample_rate, audio.channels), (48_000, 2));
            assert_eq!(audio.samples, [0.5, -0.5]);
        }
    }

    #[test]
    fn a_frame_that_is_not_rgba8_is_refused() {
        let err = wit_frame(&rgba_frame(
            RationalTime::zero(Rational::FPS_24),
            PixelFormat::Nv12,
        ))
        .expect_err("NV12 frame");
        assert!(matches!(err, EncodeError::Unsupported(_)), "{err:?}");
    }
}
