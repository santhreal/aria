//! aria-encoder: platform-native video encoding behind
//! [`aria_core::VideoEncoder`], and a sound-only sink behind
//! [`aria_core::AudioEncoder`].
//!
//! The mirror of `aria-decoder`: the renderer composites the timeline to
//! frames, and a native encoder muxes them to a file. Like decode, the codec
//! call is platform-native (VideoToolbox on Apple, Media Foundation on Windows,
//! MediaCodec on Android) while the *control* — frames in, file out — stays in
//! Rust behind the trait, so desktop, mobile, and the Python bindings share one
//! export path.
//!
//! The **Apple** backend (`AvfEncoder`) uses `AVAssetWriter` (VideoToolbox
//! H.264 + AAC, mp4 mux); the **Windows** backend (`WmfEncoder`) uses Media
//! Foundation's `IMFSinkWriter` (H.264 + AAC, mp4 mux); the **Android**
//! backend (`MediaCodecEncoder`) uses MediaCodec (H.264 + AAC) with
//! `AMediaMuxer`. Linux ([`GstEncoder`]) uses GStreamer (`x264enc` / `x265enc`
//! / VAAPI / NVENC + AAC + `mp4mux`), and is the only backend that writes
//! H.265 or honours a requested bitrate — see [`capability`]. Other platforms
//! return [`EncodeError::Unsupported`] from [`open_encoder`] until their
//! backends land.

use std::path::Path;

use aria_core::{
    AudioEncoder, AudioEncoderConfig, EncodeError, EncoderConfig, VideoCodec, VideoEncoder,
};

#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_vendor = "apple")]
pub use apple::AvfEncoder;

#[cfg(target_os = "windows")]
mod wmf;
#[cfg(target_os = "windows")]
pub use wmf::WmfEncoder;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::MediaCodecEncoder;

#[cfg(target_os = "linux")]
mod gst;
#[cfg(target_os = "linux")]
pub use gst::GstEncoder;

// RGBA→YUV conversion lives outside the platform cfgs so it's unit-testable
// on any host; the Android (I420) and Windows (NV12) backends consume it.
mod yuv;

// RGBA↔BGRA swizzle lives outside the platform cfgs so it is unit-testable and
// benchmarkable on any host; the Apple backend consumes it.
mod swizzle;
pub use swizzle::{copy_bgra, swizzle_rgba_to_bgra};
// The sound-only sink. Pure Rust, so it is compiled on every platform and is
// not behind a backend cfg: a machine with no video encoder still writes audio.
mod wav;
pub use wav::WavEncoder;

/// Open the platform's native encoder for `path` with `config`, returning it
/// behind the [`VideoEncoder`] trait so the export loop is platform-agnostic.
///
/// The container is inferred from the path extension by the backend (`.mp4` on
/// Apple and Windows). Any existing file at `path` is overwritten.
///
/// A codec or a bitrate this backend cannot write is refused here rather than
/// in each backend, so no platform can quietly write a different file from the
/// one that was asked for. [`capability`] is what a caller checks first.
pub fn open_encoder(
    path: &Path,
    config: EncoderConfig,
) -> Result<Box<dyn VideoEncoder>, EncodeError> {
    let cap = capability();
    if !cap.codecs.contains(&config.video_codec) {
        return Err(EncodeError::unsupported(format!(
            "{} cannot write {} here",
            cap.backend,
            config.video_codec.label()
        )));
    }
    if config.bitrate_kbps.is_some() && !cap.bitrate {
        return Err(EncodeError::unsupported(format!(
            "{} does not take a target bitrate",
            cap.backend
        )));
    }
    #[cfg(target_vendor = "apple")]
    {
        Ok(Box::new(AvfEncoder::open(path, config)?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(WmfEncoder::open(path, config)?))
    }
    #[cfg(target_os = "android")]
    {
        Ok(Box::new(MediaCodecEncoder::open(path, config)?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(GstEncoder::open(path, config)?))
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "windows",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        let _ = (path, config);
        Err(EncodeError::unsupported(
            "no native video encoder for this platform yet",
        ))
    }
}

/// Open the sound-only sink for `path` with `config`, returning it behind the
/// [`AudioEncoder`] trait.
///
/// The counterpart to [`open_encoder`] for a file with no picture in it, which
/// no platform video backend can write. See [`AUDIO_CAPABILITY`] for what it
/// produces. Any existing file at `path` is overwritten.
pub fn open_audio_encoder(
    path: &Path,
    config: AudioEncoderConfig,
) -> Result<Box<dyn AudioEncoder>, EncodeError> {
    Ok(Box::new(WavEncoder::create(path, config)?))
}

/// What a sound-only export writes here.
///
/// The audio counterpart to [`EncoderCapability`], and separate from it because
/// the two answer different questions: that one describes a file with a picture
/// and can be unavailable, this one is written in Rust and so is the same
/// everywhere. A shell reads the container and codec from here rather than
/// naming them itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioCapability {
    /// Container the sink writes.
    pub container: &'static str,
    /// How the samples are coded in it.
    pub codec: &'static str,
    /// The file extension the container is named with, without a dot.
    pub extension: &'static str,
}

impl AudioCapability {
    /// How the capability reads in a sheet: `WAV · 16-bit PCM`.
    pub fn label(&self) -> String {
        format!("{} · {}", self.container, self.codec)
    }
}

/// The capability of the sink [`open_audio_encoder`] opens. Available on every
/// platform, which is why it is a value rather than a probe.
pub const AUDIO_CAPABILITY: AudioCapability = AudioCapability {
    container: "WAV",
    codec: "16-bit PCM",
    extension: "wav",
};

/// What this platform's export path writes.
///
/// One owner of the answer to "what does Export produce here", because the
/// export sheet used to state `MP4 · H.264` as a compiled-in label: on a
/// platform with no backend that label was a lie, and on Linux it named neither
/// the muxer nor the audio codec. A shell reads this instead of describing the
/// encoder from memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderCapability {
    /// The backend doing the encoding, in the words its own docs use.
    pub backend: &'static str,
    /// Container the backend muxes into.
    pub container: &'static str,
    /// The file extension that container is named with, without a dot. What a
    /// caller naming an output file reads, so the extension and the muxer cannot
    /// disagree.
    pub extension: &'static str,
    /// Video codec it writes by default.
    pub video: &'static str,
    /// Audio codec it writes, or `None` when it writes no audio track.
    pub audio: Option<&'static str>,
    /// Whether [`open_encoder`] can succeed on this platform at all.
    pub available: bool,
    /// The codecs this backend can be asked for. A codec outside this list is
    /// refused by [`open_encoder`] rather than silently swapped for another.
    pub codecs: &'static [VideoCodec],
    /// Whether a [`EncoderConfig::bitrate_kbps`] is honoured. When it is not,
    /// asking for one is refused: a file at the encoder's own rate is not the
    /// file that was asked for.
    pub bitrate: bool,
}

impl EncoderCapability {
    /// How the capability reads in an export sheet: `MP4 · H.264 / AAC`, or a
    /// sentence when this platform has no encoder.
    pub fn label(&self) -> String {
        self.label_for(self.codecs.first().copied().unwrap_or_default())
    }

    /// The same line for a chosen `codec`, so a sheet with a codec picker
    /// describes the file it is about to write rather than the default one.
    ///
    /// One owner of the shape of that line: a sheet never assembles it from the
    /// fields itself.
    pub fn label_for(&self, codec: VideoCodec) -> String {
        if !self.available {
            return "No encoder on this platform".to_string();
        }
        match self.audio {
            Some(audio) => format!("{} · {} / {}", self.container, codec.label(), audio),
            None => format!("{} · {}, no audio", self.container, codec.label()),
        }
    }
}

/// The capability of the encoder [`open_encoder`] would open here.
pub fn capability() -> EncoderCapability {
    #[cfg(target_vendor = "apple")]
    {
        EncoderCapability {
            backend: "VideoToolbox",
            container: "MP4",
            extension: "mp4",
            video: "H.264",
            audio: Some("AAC"),
            available: true,
            codecs: &[VideoCodec::H264],
            bitrate: false,
        }
    }
    #[cfg(target_os = "windows")]
    {
        EncoderCapability {
            backend: "Media Foundation",
            container: "MP4",
            extension: "mp4",
            video: "H.264",
            audio: Some("AAC"),
            available: true,
            codecs: &[VideoCodec::H264],
            bitrate: false,
        }
    }
    #[cfg(target_os = "android")]
    {
        EncoderCapability {
            backend: "MediaCodec",
            container: "MP4",
            extension: "mp4",
            video: "H.264",
            audio: Some("AAC"),
            available: true,
            codecs: &[VideoCodec::H264],
            bitrate: false,
        }
    }
    #[cfg(target_os = "linux")]
    {
        EncoderCapability {
            backend: "GStreamer",
            container: "MP4",
            extension: "mp4",
            video: "H.264",
            audio: Some("AAC"),
            available: true,
            codecs: &[VideoCodec::H264, VideoCodec::H265],
            bitrate: true,
        }
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "windows",
        target_os = "android",
        target_os = "linux"
    )))]
    {
        EncoderCapability {
            backend: "none",
            container: "none",
            extension: "",
            video: "none",
            audio: None,
            available: false,
            codecs: &[],
            bitrate: false,
        }
    }
}

/// The codecs a picker should offer here: what the backend can be asked for,
/// narrowed to what this machine can actually do.
///
/// On Linux the codec plugins are installed separately from GStreamer itself,
/// so the compiled-in list is filtered by probing for each codec's elements. A
/// picker offering a codec the pipeline would then refuse is worse than a
/// picker with one entry.
pub fn available_codecs() -> Vec<VideoCodec> {
    let cap = capability();
    #[cfg(target_os = "linux")]
    {
        return cap
            .codecs
            .iter()
            .copied()
            .filter(|codec| gst::can_encode(*codec))
            .collect();
    }
    #[cfg(not(target_os = "linux"))]
    {
        cap.codecs.to_vec()
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    /// The label is what an export sheet shows, so it has to name the container
    /// and the codec, and it must not claim a codec on a platform that has no
    /// encoder at all.
    #[test]
    fn the_capability_describes_this_platform() {
        let cap = capability();
        let label = cap.label();
        if cap.available {
            assert!(label.contains(cap.container), "{label} hides the container");
            assert!(label.contains(cap.video), "{label} hides the codec");
            assert_ne!(cap.backend, "none");
            match cap.audio {
                Some(audio) => assert!(label.contains(audio)),
                None => assert!(label.contains("no audio")),
            }
        } else {
            assert_eq!(label, "No encoder on this platform");
            assert_eq!(cap.audio, None);
        }
    }

    /// Availability is a fact about the build, not a guess: every platform the
    /// crate compiles a backend for reports one.
    #[test]
    fn availability_matches_the_compiled_backend() {
        let expected = cfg!(any(
            target_vendor = "apple",
            target_os = "windows",
            target_os = "android",
            target_os = "linux"
        ));
        assert_eq!(capability().available, expected);
    }

    /// A backend that can encode at all can be asked for at least one codec,
    /// and the codec its label names is one of them. The class closed here is a
    /// picker offering a codec `open_encoder` would refuse.
    #[test]
    fn the_codec_list_agrees_with_the_label() {
        let cap = capability();
        if cap.available {
            assert!(!cap.codecs.is_empty(), "an encoder writes some codec");
            assert!(
                cap.codecs.iter().any(|codec| codec.label() == cap.video),
                "the label names {} but the list is {:?}",
                cap.video,
                cap.codecs
            );
        } else {
            assert!(cap.codecs.is_empty(), "no encoder writes no codec");
            assert!(!cap.bitrate, "no encoder takes no bitrate");
        }
    }

    /// Every offered codec is one the backend admits to, and the offer is
    /// narrowed by what the machine has rather than by what was compiled.
    #[test]
    fn every_offered_codec_is_one_the_backend_claims() {
        let cap = capability();
        let offered = available_codecs();
        for codec in &offered {
            assert!(
                cap.codecs.contains(codec),
                "{} is offered but not claimed",
                codec.label()
            );
        }
        assert!(offered.len() <= cap.codecs.len());
    }

    /// A codec outside the list is refused by name, and refused before any file
    /// is touched. Silently writing H.264 for an H.265 request is the failure
    /// this closes.
    #[test]
    fn a_codec_the_backend_cannot_write_is_refused_by_name() {
        let cap = capability();
        let Some(missing) = VideoCodec::ALL
            .into_iter()
            .find(|codec| !cap.codecs.contains(codec))
        else {
            return; // This build writes everything there is to ask for.
        };
        let path = std::env::temp_dir().join("aria-encoder-refusal-probe.mp4");
        let _ = std::fs::remove_file(&path);
        let config = EncoderConfig::new(
            (64, 64),
            aria_core::Rational::FPS_24,
            aria_core::ColorSpace::BT709,
        )
        .with_codec(missing);
        let Err(err) = open_encoder(&path, config) else {
            panic!("an unwritable codec was accepted");
        };
        let message = err.to_string();
        assert!(
            message.contains(missing.label()),
            "the refusal does not name the codec: {message}"
        );
        assert!(!path.exists(), "a refused encode created a file");
    }

    /// A bitrate a backend does not honour is refused rather than ignored: a
    /// file at the encoder's own rate is not the file that was asked for.
    #[test]
    fn a_bitrate_the_backend_ignores_is_refused() {
        let cap = capability();
        if cap.bitrate {
            return;
        }
        let path = std::env::temp_dir().join("aria-encoder-bitrate-probe.mp4");
        let _ = std::fs::remove_file(&path);
        let config = EncoderConfig::new(
            (64, 64),
            aria_core::Rational::FPS_24,
            aria_core::ColorSpace::BT709,
        )
        .with_bitrate_kbps(4_000);
        let Err(err) = open_encoder(&path, config) else {
            panic!("an ignored bitrate was accepted");
        };
        assert!(err.to_string().contains("bitrate"), "{err}");
        assert!(!path.exists(), "a refused encode created a file");
    }
}
