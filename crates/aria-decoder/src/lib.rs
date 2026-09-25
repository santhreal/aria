//! Platform-native video decoding for Aria.
//!
//! Aria never ships its own AVC/HEVC decoder (royalties + performance);
//! instead each platform's hardware codec is wrapped behind
//! [`aria_core::VideoDecoder`], so the decode *control* (open, seek, frame
//! selection) is shared Rust while only the codec call is native.
//!
//! - **Apple** (`AvfDecoder`): `AVAssetReader` does demux + VideoToolbox
//!   decode in one, handing back IOSurface-backed `CVPixelBuffer`s. Those map
//!   into a `wgpu` texture with no copy on the GPU path, or read back to planes
//!   on the CPU path.
//! - **Windows** (`WmfDecoder`): Media Foundation's Source Reader demuxes +
//!   decodes — hardware (DXVA) when a D3D11 device is available, software
//!   otherwise — handing back NV12 planes on the CPU path or shared D3D11
//!   textures on the GPU path.
//! - **Android** (`MediaCodecDecoder`): NDK `AMediaExtractor` demuxes and
//!   `AMediaCodec` hardware-decodes, handing back NV12/I420 planes. There is no
//!   zero-copy `AHardwareBuffer` path, and [`decoder_capability`] says so, so a
//!   caller picks CPU planes instead of opening a file to find out.
//! - **Linux** ([`GstDecoder`]): GStreamer `uridecodebin` demuxes and
//!   decodes (VAAPI / NVDEC / V4L2 when the host plugins provide them,
//!   software otherwise). `OutputMode::Gpu` tries a DMA-BUF appsink
//!   (LINEAR NV12); tiled or missing DMABuf falls back to CPU NV12.
//!   Aria never ships a codec; missing plugins return
//!   [`DecodeError::Unsupported`].
//!
//! All backends share one `frame_at` seek policy (the `seek` module):
//! sequential and near-forward targets roll forward from the last emitted
//! frame instead of seeking, so playback and forward scrubbing never pay a
//! per-frame seek + GOP re-decode.

#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_vendor = "apple")]
pub use apple::AvfDecoder;
#[cfg(target_vendor = "apple")]
mod apple_audio;
#[cfg(target_vendor = "apple")]
pub use apple_audio::AvfAudioReader;

#[cfg(target_os = "windows")]
mod wmf;
#[cfg(target_os = "windows")]
pub use wmf::WmfDecoder;
#[cfg(target_os = "windows")]
mod wmf_audio;
#[cfg(target_os = "windows")]
pub use wmf_audio::WmfAudioReader;
/// Fragmented-MP4 duration recovery, for demuxers that report none
/// (Media Foundation today; usable by any backend with the same gap).
#[cfg(target_os = "windows")]
mod mp4_duration;
/// Shared D3D11 device + DXGI device manager backing hardware decode.
#[cfg(target_os = "windows")]
mod wmf_d3d;
/// Shared NV12 texture pool + fence: the decoder side of zero-copy output.
#[cfg(target_os = "windows")]
mod wmf_gpu;
/// Slot-acquisition policy for the shared NV12 pool. Also compiled under
/// `cfg(test)` off Windows so the exhaustion bound is tested on every host.
#[cfg(any(target_os = "windows", test))]
mod wmf_slot;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{MediaCodecAudioReader, MediaCodecDecoder};

#[cfg(target_os = "linux")]
mod gst;
#[cfg(target_os = "linux")]
pub use gst::GstDecoder;
#[cfg(target_os = "linux")]
mod gst_audio;
#[cfg(target_os = "linux")]
mod gst_gpu;
#[cfg(target_os = "linux")]
pub use gst_audio::GstAudioReader;

/// Shared `frame_at` roll-forward policy used by every backend.
#[cfg(any(
    target_vendor = "apple",
    target_os = "windows",
    target_os = "android",
    target_os = "linux"
))]
mod seek;

mod pixel_map;

mod probe;
pub use probe::{MediaProbe, probe};

mod peaks;
pub use peaks::{AudioPeaks, audio_peaks, audio_peaks_per_second};

mod pcm;
pub use pcm::{ReadMonoPcmError, read_mono_pcm_with_cancel};

pub mod image;
#[cfg(target_vendor = "apple")]
mod image_apple;
pub use image::{
    ImageFormat, ImageFormatSpec, ImageInfo, decode_image, decode_image_bytes, every_image_format,
    image_extensions, probe_image, sniff_bytes, sniff_image,
};

mod animation;
pub use animation::{
    AnimationFrame, MAX_ANIMATION_DIMENSION, MAX_ANIMATION_FILE_BYTES, MAX_ANIMATION_FRAMES,
    decode_animation, decode_animation_file,
};

mod lottie;
pub use lottie::{LOTTIE_MAX_DIMENSION, LOTTIE_MAX_SAMPLE_FPS, LottieAnimation};

use std::path::Path;

use aria_core::{AudioReader, DecodeError, VideoDecoder};

/// What the platform's decoder backend can deliver, answerable without opening
/// a file.
///
/// A caller picks its [`OutputMode`] from this instead of asking for one and
/// reading the refusal: an Android build reports no GPU surfaces up front, so
/// nothing has to open a file to learn that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderCapability {
    /// Backend this build dispatches to, for logs and error text.
    pub backend: &'static str,
    /// Whether [`OutputMode::Gpu`] hands back GPU surfaces. A backend may still
    /// fall back to CPU planes per frame when the host lacks the hardware path.
    pub gpu_surfaces: bool,
    /// Whether [`OutputMode::Cpu`] is available. `false` means this platform has
    /// no decoder at all.
    pub cpu_planes: bool,
}

impl DecoderCapability {
    /// Whether `mode` can be opened.
    pub fn supports(self, mode: OutputMode) -> bool {
        match mode {
            OutputMode::Gpu => self.gpu_surfaces,
            OutputMode::Cpu => self.cpu_planes,
        }
    }

    /// The closest mode to `wanted` this backend can deliver, or `None` when the
    /// platform has no decoder.
    pub fn best(self, wanted: OutputMode) -> Option<OutputMode> {
        if self.supports(wanted) {
            return Some(wanted);
        }
        // GPU is the only mode a backend can lack while still decoding, so the
        // one remaining candidate is CPU planes.
        self.cpu_planes.then_some(OutputMode::Cpu)
    }
}

/// Which decoder backend this build dispatches to and what it can deliver.
pub fn decoder_capability() -> DecoderCapability {
    #[cfg(target_vendor = "apple")]
    {
        DecoderCapability {
            backend: "avfoundation",
            gpu_surfaces: true,
            cpu_planes: true,
        }
    }
    #[cfg(target_os = "windows")]
    {
        DecoderCapability {
            backend: "media foundation",
            gpu_surfaces: true,
            cpu_planes: true,
        }
    }
    #[cfg(target_os = "android")]
    {
        // MediaCodec runs in ByteBuffer mode here: no `AImageReader` surface, so
        // no `AHardwareBuffer` to hand the renderer.
        DecoderCapability {
            backend: "mediacodec",
            gpu_surfaces: false,
            cpu_planes: true,
        }
    }
    #[cfg(target_os = "linux")]
    {
        DecoderCapability {
            backend: "gstreamer",
            gpu_surfaces: true,
            cpu_planes: true,
        }
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "windows",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        DecoderCapability {
            backend: "none",
            gpu_surfaces: false,
            cpu_planes: false,
        }
    }
}

/// Open the platform-native video decoder for `path`, delivering frames in
/// `mode`.
///
/// Mirrors [`open_audio_reader`]: shared control flow, native codec only.
/// Returns [`DecodeError::Unsupported`] on platforms without a backend, and for
/// a mode the backend cannot deliver — ask [`decoder_capability`] first rather
/// than reading the refusal. Apple, Windows, and Linux honor
/// [`OutputMode::Gpu`] (zero-copy surfaces, with per-frame or per-open CPU
/// fallback when the hardware path is unavailable); Android has CPU planes only.
pub fn open_video_decoder(
    path: &Path,
    mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, DecodeError> {
    let capability = decoder_capability();
    if !capability.supports(mode) {
        return Err(DecodeError::unsupported(unsupported_mode(capability, mode)));
    }
    #[cfg(target_vendor = "apple")]
    {
        Ok(Box::new(AvfDecoder::open(path, mode)?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(WmfDecoder::open(path, mode)?))
    }
    #[cfg(target_os = "android")]
    {
        Ok(Box::new(MediaCodecDecoder::open(path, mode)?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(GstDecoder::open(path, mode)?))
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "windows",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        let _ = (path, mode);
        Err(DecodeError::unsupported(
            "no native video decoder for this platform",
        ))
    }
}

/// Refusal text for a mode the backend cannot deliver, naming the backend and
/// what it does have, so the caller can act on it without guessing.
pub(crate) fn unsupported_mode(capability: DecoderCapability, mode: OutputMode) -> String {
    let wanted = match mode {
        OutputMode::Gpu => "GPU surfaces",
        OutputMode::Cpu => "CPU planes",
    };
    if !capability.cpu_planes && !capability.gpu_surfaces {
        return format!("no native video decoder for this platform, so no {wanted}");
    }
    let has = match (capability.gpu_surfaces, capability.cpu_planes) {
        (true, true) => "GPU surfaces and CPU planes",
        (true, false) => "GPU surfaces only",
        (false, true) => "CPU planes only",
        (false, false) => unreachable!("handled above"),
    };
    format!(
        "the {} decoder delivers {has}, not {wanted}; ask decoder_capability() for a mode it has",
        capability.backend
    )
}

/// Open the platform-native audio reader for `path`, decoding to interleaved
/// `f32` at `out_rate` Hz and `channels` channels (the mixer's format).
///
/// Mirrors the video decoder's platform dispatch; returns
/// [`DecodeError::Unsupported`] on platforms without an audio backend.
pub fn open_audio_reader(
    path: &Path,
    out_rate: u32,
    channels: u16,
) -> Result<Box<dyn AudioReader>, DecodeError> {
    #[cfg(target_vendor = "apple")]
    {
        Ok(Box::new(AvfAudioReader::open(path, out_rate, channels)?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(WmfAudioReader::open(path, out_rate, channels)?))
    }
    #[cfg(target_os = "android")]
    {
        Ok(Box::new(MediaCodecAudioReader::open(
            path, out_rate, channels,
        )?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(GstAudioReader::open(path, out_rate, channels)?))
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "windows",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        let _ = (path, out_rate, channels);
        Err(DecodeError::unsupported(
            "no native audio decoder for this platform",
        ))
    }
}

/// How a decoder should deliver frame pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Zero-copy: hand back the decoder's native GPU surface
    /// ([`aria_core::FrameData::Gpu`]) for the renderer to import. The fast
    /// path for preview/playback.
    #[default]
    Gpu,
    /// Copy decoded pixels into CPU planes ([`aria_core::FrameData::Cpu`]).
    /// The portable path — used for export readback, tests, and platforms
    /// without zero-copy import.
    Cpu,
}

impl OutputMode {
    /// Every delivery mode, fastest first.
    ///
    /// A surface reporting what the backend can deliver walks this instead of
    /// naming the two modes, so a third mode appears in the report by existing.
    pub const ALL: [OutputMode; 2] = [OutputMode::Gpu, OutputMode::Cpu];

    /// Operator-facing name of the mode.
    pub const fn label(self) -> &'static str {
        match self {
            OutputMode::Gpu => "GPU surfaces",
            OutputMode::Cpu => "CPU planes",
        }
    }
}

/// Capability reporting: a caller must be able to pick a mode without opening a
/// file and reading a refusal, and the refusal it does get must name what the
/// backend has. Every mode in [`OutputMode`] is covered, so adding one turns
/// these red until it is reported.
#[cfg(test)]
mod capability_tests {
    use super::*;

    /// The modes under test are the axis itself, so a mode added to
    /// [`OutputMode::ALL`] is covered here without a second list.
    const MODES: [OutputMode; OutputMode::ALL.len()] = OutputMode::ALL;

    #[test]
    fn this_build_reports_a_named_backend_that_can_decode() {
        let capability = decoder_capability();
        assert!(
            !capability.backend.is_empty(),
            "a build must say which backend it dispatches to"
        );
        // Every platform this workspace builds for has CPU planes; a platform
        // without a decoder reports "none" and no modes at all.
        assert_eq!(
            capability.cpu_planes,
            capability.backend != "none",
            "{} reported cpu_planes={}",
            capability.backend,
            capability.cpu_planes
        );
        assert!(
            !(capability.gpu_surfaces && !capability.cpu_planes),
            "a GPU-only decoder has no readback path for export"
        );
    }

    #[test]
    fn best_only_ever_names_a_mode_the_backend_has() {
        for capability in [
            decoder_capability(),
            DecoderCapability {
                backend: "cpu only",
                gpu_surfaces: false,
                cpu_planes: true,
            },
            DecoderCapability {
                backend: "none",
                gpu_surfaces: false,
                cpu_planes: false,
            },
        ] {
            for mode in MODES {
                match capability.best(mode) {
                    Some(picked) => assert!(
                        capability.supports(picked),
                        "{} offered {picked:?} for {mode:?} and cannot deliver it",
                        capability.backend
                    ),
                    None => assert!(
                        !capability.cpu_planes,
                        "{} refused every mode while it can decode",
                        capability.backend
                    ),
                }
            }
        }
    }

    #[test]
    fn a_cpu_only_backend_downgrades_gpu_instead_of_failing() {
        let capability = DecoderCapability {
            backend: "mediacodec",
            gpu_surfaces: false,
            cpu_planes: true,
        };
        assert!(!capability.supports(OutputMode::Gpu));
        assert_eq!(capability.best(OutputMode::Gpu), Some(OutputMode::Cpu));
        assert_eq!(capability.best(OutputMode::Cpu), Some(OutputMode::Cpu));
    }

    #[test]
    fn a_refusal_names_the_backend_and_what_it_has() {
        let capability = DecoderCapability {
            backend: "mediacodec",
            gpu_surfaces: false,
            cpu_planes: true,
        };
        let message = unsupported_mode(capability, OutputMode::Gpu);
        assert!(message.contains("mediacodec"), "{message}");
        assert!(message.contains("CPU planes only"), "{message}");
        assert!(message.contains("GPU surfaces"), "{message}");
        assert!(message.contains("decoder_capability"), "{message}");
    }

    #[test]
    fn a_platform_with_no_decoder_says_so_for_every_mode() {
        let capability = DecoderCapability {
            backend: "none",
            gpu_surfaces: false,
            cpu_planes: false,
        };
        for mode in MODES {
            assert_eq!(capability.best(mode), None);
            let message = unsupported_mode(capability, mode);
            assert!(
                message.contains("no native video decoder"),
                "{mode:?}: {message}"
            );
        }
    }
}
