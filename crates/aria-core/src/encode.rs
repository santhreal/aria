//! The encode contract (export seam).
//!
//! The mirror image of [`crate::decode`]: the compositor renders the timeline
//! to frames, and a native encoder muxes them to a file. Like decode, the codec
//! call is platform-native (VideoToolbox / MediaCodec / Media Foundation, or a
//! software fallback) while control stays in Rust behind [`VideoEncoder`]. This
//! is the "composite every frame `0..duration` → mux to mp4" path that the
//! engine, the future Rust exporter, and the Python bindings all build on.
//!
//! This is deliberately a thin seam: frames in, file out, codec behind the
//! trait. What the caller chooses is the codec and the bitrate; everything else
//! about the encode is the backend's business.

use crate::audio::AudioEncoderConfig;
use crate::color::ColorSpace;
use crate::frame::VideoFrame;
use crate::time::{Rational, RationalTime};

/// An encode failure, backend-agnostic across native and software encoders.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// Creating the encoder or output file failed.
    #[error("failed to start encoder: {0}")]
    Start(String),
    /// Encoding or muxing a frame failed.
    #[error("encode failed: {0}")]
    Encode(String),
    /// The requested output format / codec / pixel format isn't supported.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl EncodeError {
    pub fn unsupported(msg: impl Into<String>) -> Self {
        EncodeError::Unsupported(msg.into())
    }
}

/// A video codec an export can ask for.
///
/// Deliberately short: a codec is only listed once every shipped backend can
/// either write it or refuse it by name. A backend that cannot write one
/// returns [`EncodeError::Unsupported`] rather than quietly writing another —
/// a file that is not the codec that was asked for is worse than a refusal.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoCodec {
    /// H.264 / AVC. Every backend writes this.
    #[default]
    H264,
    /// H.265 / HEVC. Smaller at the same quality, less widely playable.
    H265,
}

impl VideoCodec {
    /// Every codec, in the order a picker should offer them.
    pub const ALL: [VideoCodec; 2] = [VideoCodec::H264, VideoCodec::H265];

    /// The stable key used in config files and command lines.
    pub fn key(self) -> &'static str {
        match self {
            VideoCodec::H264 => "h264",
            VideoCodec::H265 => "h265",
        }
    }

    /// How the codec is named in a picker.
    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265",
        }
    }

    /// Parse a [`key`](Self::key). `hevc` and `avc` are accepted as aliases,
    /// since that is what the platform encoders call them.
    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "h264" | "avc" => Some(VideoCodec::H264),
            "h265" | "hevc" => Some(VideoCodec::H265),
            _ => None,
        }
    }
}

/// Output configuration for an encode session.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderConfig {
    /// Output frame size in pixels.
    pub size: (u32, u32),
    /// Output frame rate.
    pub frame_rate: Rational,
    /// Colorimetry to tag the output with.
    pub color: ColorSpace,
    /// Audio track configuration. `None` ⇒ a video-only file (the encoder adds
    /// no audio track and [`push_audio`](VideoEncoder::push_audio) is unused).
    pub audio: Option<AudioEncoderConfig>,
    /// Maximum keyframe (sync-point) spacing in frames. `None` leaves the
    /// codec's default GOP policy (typically seconds-long for delivery).
    /// Preview proxies set a short interval (~15) so scrub seeks decode at
    /// most a few frames to reach any target.
    #[cfg_attr(feature = "serde", serde(default))]
    pub keyframe_interval: Option<u32>,
    /// Video codec to write.
    #[cfg_attr(feature = "serde", serde(default))]
    pub video_codec: VideoCodec,
    /// Target average video bitrate in kilobits per second. `None` leaves the
    /// encoder's own default, which is quality-driven rather than rate-capped.
    ///
    /// Kilobits rather than bits because that is the unit every backend's rate
    /// property takes, and a bitrate expressed to the bit is a false precision.
    #[cfg_attr(feature = "serde", serde(default))]
    pub bitrate_kbps: Option<u32>,
}

impl EncoderConfig {
    /// A video-only encode config (no audio track).
    pub fn new(size: (u32, u32), frame_rate: Rational, color: ColorSpace) -> Self {
        Self {
            size,
            frame_rate,
            color,
            audio: None,
            keyframe_interval: None,
            video_codec: VideoCodec::default(),
            bitrate_kbps: None,
        }
    }

    /// Add an audio track to this config.
    pub fn with_audio(mut self, audio: AudioEncoderConfig) -> Self {
        self.audio = Some(audio);
        self
    }

    /// Cap the keyframe (sync-point) spacing at `frames`.
    pub fn with_keyframe_interval(mut self, frames: u32) -> Self {
        self.keyframe_interval = Some(frames);
        self
    }

    /// Write `codec` instead of the default H.264.
    pub fn with_codec(mut self, codec: VideoCodec) -> Self {
        self.video_codec = codec;
        self
    }

    /// Target `kbps` kilobits per second of video.
    pub fn with_bitrate_kbps(mut self, kbps: u32) -> Self {
        self.bitrate_kbps = Some(kbps);
        self
    }

    /// Validate the output configuration (dimensions even and >= 2, valid frame rate, non-zero bitrate/keyframe interval).
    pub fn validate(&self) -> Result<(), EncodeError> {
        let (w, h) = self.size;
        if w < 2 || h < 2 || w % 2 != 0 || h % 2 != 0 {
            return Err(EncodeError::Unsupported(format!(
                "invalid dimensions {w}x{h}: dimensions must be even and >= 2"
            )));
        }
        if !self.frame_rate.is_valid() || self.frame_rate.num <= 0 || self.frame_rate.den <= 0 {
            return Err(EncodeError::Unsupported(format!(
                "invalid frame rate {:?}: frame rate must be positive and valid",
                self.frame_rate
            )));
        }
        if let Some(bitrate) = self.bitrate_kbps {
            if bitrate == 0 {
                return Err(EncodeError::Unsupported(
                    "bitrate_kbps must be greater than zero".into(),
                ));
            }
        }
        if let Some(interval) = self.keyframe_interval {
            if interval == 0 {
                return Err(EncodeError::Unsupported(
                    "keyframe_interval must be greater than zero".into(),
                ));
            }
        }
        if let Some(audio) = &self.audio {
            audio
                .validate()
                .map_err(|msg| EncodeError::Unsupported(msg.to_string()))?;
        }
        Ok(())
    }
}

/// What an export was *asked* for, one field per choice an operator can make.
///
/// Every field is an override: `None` (and [`ExportFormat::default`]) means
/// "whatever the project already is", which is why this type carries no
/// project-derived value of its own. It is what a command, a CLI invocation and
/// an export sheet all send; resolving it against a project — and refusing a
/// choice this machine's encoder cannot honour — happens in exactly one place,
/// `aria_render::ExportSettings::resolved`.
///
/// Serialized as part of `ProjectCommand::Export`, so an absent field must stay
/// absent: an older caller's `{"path": …}` has to keep meaning "the project's
/// own output", not "H.264 at some default size".
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportFormat {
    /// Output size in pixels. `None` renders at the project canvas.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub size: Option<(u32, u32)>,
    /// Frame sampling rate. `None` uses the timeline's own rate.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub frame_rate: Option<Rational>,
    /// Video codec. `None` writes H.264, which every backend can write.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub video_codec: Option<VideoCodec>,
    /// Target average video bitrate in kilobits per second. `None` leaves the
    /// encoder's own quality-driven rate control.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub bitrate_kbps: Option<u32>,
    /// Maximum keyframe spacing in frames. `None` leaves the codec's default.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub keyframe_interval: Option<u32>,
    /// Whether the file carries the audio mix. `None` includes it when the
    /// project has any, which is what the project itself implies.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub audio: Option<bool>,
    /// Half-open `[from, to)` span of the timeline to render. `None` renders all
    /// of it. The output always starts at zero either way.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub range: Option<(RationalTime, RationalTime)>,
}

impl ExportFormat {
    /// The project's own output, unmodified: what an export with no choices
    /// made writes.
    pub const PROJECT: Self = Self {
        size: None,
        frame_rate: None,
        video_codec: None,
        bitrate_kbps: None,
        keyframe_interval: None,
        audio: None,
        range: None,
    };

    /// Whether this asks for anything other than the project's own output.
    ///
    /// A status line uses it to say "as the project" instead of listing seven
    /// values that are all the default.
    pub fn is_project_default(&self) -> bool {
        *self == Self::PROJECT
    }
}

/// The encoder an export session opened, and why each one tried before it was
/// not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderChoice {
    /// The encoder: a GStreamer element such as `nvh264enc`, a platform
    /// framework such as `AVAssetWriter`, or a plugin encoder's id.
    pub encoder: std::borrow::Cow<'static, str>,
    /// Whether it encodes on a GPU or media engine (`Some(true)`) or on the CPU
    /// (`Some(false)`); `None` when the platform framework selects the codec
    /// itself and does not report which one it took.
    pub hardware: Option<bool>,
    /// Why each encoder tried before [`Self::encoder`] did not open, in order.
    pub passed_over: Vec<String>,
}

impl EncoderChoice {
    /// An encoder that was the only one tried, on the processor `hardware`
    /// states.
    pub fn only(
        encoder: impl Into<std::borrow::Cow<'static, str>>,
        hardware: Option<bool>,
    ) -> Self {
        Self {
            encoder: encoder.into(),
            hardware,
            passed_over: Vec::new(),
        }
    }
}

impl std::fmt::Display for EncoderChoice {
    /// `encoder <name> (<processor>)`, then one `skipped <reason>` line for
    /// each encoder passed over.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let processor = match self.hardware {
            Some(true) => "hardware",
            Some(false) => "software",
            None => "selected by the platform",
        };
        write!(f, "encoder {} ({processor})", self.encoder)?;
        for reason in &self.passed_over {
            write!(f, "\nskipped {reason}")?;
        }
        Ok(())
    }
}

/// A push-based video encoder.
///
/// Push composited frames in presentation order, then [`finish`](VideoEncoder::finish)
/// to flush and finalize the container. `Send` so the encoder can run on an
/// export worker thread.
pub trait VideoEncoder: Send {
    /// Encode and mux one frame. Frames are expected in presentation order.
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError>;

    /// Encode and mux one block of interleaved `f32` audio at `pts`, in
    /// presentation order alongside the video [`push`](VideoEncoder::push)es.
    ///
    /// `samples` is `frames * channels` long, matching the
    /// [`AudioEncoderConfig`] the encoder was opened with. Only meaningful when
    /// the config carried `Some(audio)`; the default implementation errors, so
    /// audio-free encoders (and the PNG sink) need not implement it.
    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let _ = (samples, pts);
        Err(EncodeError::unsupported(
            "this encoder has no audio track (config.audio was None)",
        ))
    }

    /// Flush buffered frames and finalize the output. The encoder must not be
    /// used after this returns.
    fn finish(&mut self) -> Result<(), EncodeError>;

    /// The encoder this session opened, and why the ones tried before it were
    /// not used.
    fn choice(&self) -> EncoderChoice;
}

/// A push-based audio-only encoder.
///
/// The sound half of the export seam. A [`VideoEncoder`] writes a file with a
/// picture in it, and every platform backend requires the frame size to open
/// one at all; a sound intermediate has no picture, so it is written through
/// its own sink rather than through a video encoder whose `push` refuses.
///
/// Push interleaved `f32` blocks in presentation order, then
/// [`finish`](AudioEncoder::finish) to finalize the container. `Send` so it can
/// run on an export worker thread, same as [`VideoEncoder`].
pub trait AudioEncoder: Send {
    /// Encode and write one block of interleaved `f32` audio at `pts`.
    ///
    /// `samples` is `frames * channels` long, matching the
    /// [`AudioEncoderConfig`] the sink was opened with.
    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError>;

    /// Flush buffered samples and finalize the output. The sink must not be
    /// used after this returns.
    fn finish(&mut self) -> Result<(), EncodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_config_roundtrips_fields() {
        let cfg = EncoderConfig::new((1920, 1080), Rational::FPS_30, ColorSpace::BT709);
        assert_eq!(cfg.size, (1920, 1080));
        assert_eq!(cfg.frame_rate, Rational::FPS_30);
        assert_eq!(cfg.color, ColorSpace::BT709);
    }

    #[test]
    fn encode_error_unsupported_helper() {
        let e = EncodeError::unsupported("prores not wired up");
        assert_eq!(e.to_string(), "unsupported: prores not wired up");
    }

    #[test]
    fn encoder_config_validate_catches_invalid_configurations() {
        let valid = EncoderConfig::new((1920, 1080), Rational::FPS_30, ColorSpace::BT709);
        assert!(valid.validate().is_ok());

        // Odd dimensions
        let odd_w = EncoderConfig::new((1921, 1080), Rational::FPS_30, ColorSpace::BT709);
        assert!(odd_w.validate().is_err());
        let odd_h = EncoderConfig::new((1920, 1081), Rational::FPS_30, ColorSpace::BT709);
        assert!(odd_h.validate().is_err());

        // Less than 2
        let zero_dim = EncoderConfig::new((0, 1080), Rational::FPS_30, ColorSpace::BT709);
        assert!(zero_dim.validate().is_err());

        // Zero bitrate
        let zero_bitrate = valid.clone().with_bitrate_kbps(0);
        assert!(zero_bitrate.validate().is_err());

        // Invalid frame rate
        let invalid_rate = EncoderConfig::new((1920, 1080), Rational::new(0, 1), ColorSpace::BT709);
        assert!(invalid_rate.validate().is_err());

        // Zero keyframe interval
        let zero_gop = valid.with_keyframe_interval(0);
        assert!(zero_gop.validate().is_err());
    }
}
