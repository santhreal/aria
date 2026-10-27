//! Lightweight media probing: read a source's frame size, rate, duration, and
//! tracks without decoding a frame.
//!
//! This is the FFmpeg-free replacement for container probing. On Linux one
//! demux-and-parse pass reads the stream caps ([`crate::gst_probe`]); every
//! other platform decoder reports its source's dimensions, frame rate, and
//! duration at open time, which is all the media pool needs to register a
//! clip.

use std::path::Path;

use aria_core::{DecodeError, Rational, RationalTime, SourceInfo, resample};

// Non-Linux native probes construct a decoder with an `OutputMode`.
#[cfg(any(target_vendor = "apple", target_os = "windows", target_os = "android"))]
use crate::OutputMode;

/// Static metadata for a media file, read without decoding any frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProbe {
    /// Visible frame size in pixels, after applying container rotation (or
    /// EXIF orientation for stills).
    pub width: u32,
    pub height: u32,
    /// Native (nominal) frame rate. Meaningless for stills, which have no
    /// frame cadence — importers use the model's still-image convention.
    pub frame_rate: Rational,
    /// Source length in whole frames at [`frame_rate`](Self::frame_rate); `0`
    /// when the container reports no duration (and always for stills, whose
    /// placement length is a policy of the media pool, not the file).
    pub frame_count: i64,
    /// Whether the source carries an audio track, detected per-platform at
    /// probe time. The [`aria_core::VideoDecoder`] surfaces only the video
    /// track, so audio presence is answered by the demuxer.
    pub has_audio: bool,
    /// Still image (PNG/JPEG/HEIC/WebP/...): one frame, no timebase.
    /// Detected by magic bytes before the video probe runs.
    pub is_image: bool,
    /// Codec of the first video track as the demuxer describes it, `None` when
    /// the source has no video track or the platform's demuxer cannot name one.
    /// Never inferred from the file extension: a wrong codec is worse than none.
    pub video_codec: Option<String>,
    /// Codec of the first audio track, on the same terms.
    pub audio_codec: Option<String>,
}

/// What a demuxer reports about a source's tracks besides the picture. A
/// backend that cannot name a codec leaves it `None` rather than guessing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Tracks {
    pub(crate) has_audio: bool,
    pub(crate) video_codec: Option<String>,
    pub(crate) audio_codec: Option<String>,
}

impl MediaProbe {
    /// A video source of `info`'s frame size, frame rate, and duration.
    pub(crate) fn video(info: &SourceInfo, tracks: Tracks) -> Self {
        let (width, height) = info.display_size_rotated();
        // Reduce the container's timescale-derived rate (e.g. `600/25`) to its
        // canonical form (`24/1`) so it matches source ranges authored at the
        // `FPS_*` constants under structural rate equality.
        let frame_rate = info.frame_rate.reduced();
        let frame_count = info
            .duration
            .map(|d| resample(d, frame_rate).value.max(0))
            .unwrap_or(0);
        Self {
            width,
            height,
            frame_rate,
            frame_count,
            has_audio: tracks.has_audio,
            is_image: false,
            video_codec: tracks.video_codec,
            audio_codec: tracks.audio_codec,
        }
    }

    /// A source with no decodable video track (a music or voiceover file):
    /// zero dimensions, millisecond ticks, and the audio track's `duration`.
    pub(crate) fn audio_only(duration: RationalTime, audio_codec: Option<String>) -> Self {
        Self {
            width: 0,
            height: 0,
            frame_rate: AUDIO_TICK_RATE,
            frame_count: resample(duration, AUDIO_TICK_RATE).value.max(0),
            has_audio: true,
            is_image: false,
            video_codec: None,
            audio_codec,
        }
    }
}

/// Tick rate for audio-only sources, which have no frame cadence of their
/// own. Millisecond ticks keep duration math exact (matches the model's
/// still-image convention).
const AUDIO_TICK_RATE: Rational = Rational::new(1000, 1);

/// Probe `path`: through a registered decoder that claims its extension
/// ([`crate::register_external_decoder`]), else the platform-native decoder.
///
/// Still images are recognized first by magic bytes (a cheap 16-byte read)
/// and probed via [`crate::probe_image`] — dimensions only, no timebase.
/// Sources without a decodable video stream (music/voiceover files) probe as
/// audio-only: zero dimensions, millisecond ticks, and the audio track's
/// duration. Returns [`DecodeError::Unsupported`] on platforms without a
/// native decoder.
pub fn probe(path: &Path) -> Result<MediaProbe, DecodeError> {
    crate::external::dispatch(path, |decoder| decoder.probe(path), || probe_native(path))
}

fn probe_native(path: &Path) -> Result<MediaProbe, DecodeError> {
    if crate::image::sniff_image(path).is_some() {
        let info = crate::image::probe_image(path)?;
        return Ok(MediaProbe {
            width: info.width,
            height: info.height,
            // Stills have no cadence; rate/count are placeholders the
            // importer replaces with the model's still-image convention.
            frame_rate: AUDIO_TICK_RATE,
            frame_count: 0,
            has_audio: false,
            is_image: true,
            // A still has no codec track worth naming; the image format is
            // already visible in the row's kind and path.
            video_codec: None,
            audio_codec: None,
        });
    }
    probe_stream(path)
}

/// One demux-and-parse pass answers every track question, and plugs no
/// decoder.
#[cfg(target_os = "linux")]
fn probe_stream(path: &Path) -> Result<MediaProbe, DecodeError> {
    crate::gst_probe::probe(path)
}

/// Open the video decoder for its [`SourceInfo`], and fall back to the audio
/// track when the source has no video the decoder opens.
#[cfg(not(target_os = "linux"))]
fn probe_stream(path: &Path) -> Result<MediaProbe, DecodeError> {
    probe_video(path).or_else(|video_err| {
        audio_track_duration(path)
            .map(|duration| MediaProbe::audio_only(duration, None))
            .ok_or(video_err)
    })
}

// Each platform's video probe also answers "does the source carry audio?", so
// a backend whose reader can report both (Windows) opens the file **once**;
// the others ask their demuxer separately. A missing audio track is never
// fatal to probing — the separate checks return `false` on any error. None of
// these backends names a codec.

#[cfg(target_vendor = "apple")]
fn probe_video(path: &Path) -> Result<MediaProbe, DecodeError> {
    use aria_core::VideoDecoder;
    // The audio-track check reads the decoder's already-parsed asset — no
    // second open of the file.
    let decoder = crate::AvfDecoder::open(path, OutputMode::Cpu)?;
    let has_audio = decoder.has_audio_track();
    Ok(MediaProbe::video(
        decoder.info(),
        Tracks {
            has_audio,
            ..Tracks::default()
        },
    ))
}

#[cfg(target_os = "windows")]
fn probe_video(path: &Path) -> Result<MediaProbe, DecodeError> {
    use aria_core::VideoDecoder;
    // The audio-track check reads the open reader's presentation descriptor
    // — no second open of the file.
    let decoder = crate::WmfDecoder::open(path, OutputMode::Cpu)?;
    let has_audio = decoder.has_audio_track();
    Ok(MediaProbe::video(
        decoder.info(),
        Tracks {
            has_audio,
            ..Tracks::default()
        },
    ))
}

#[cfg(target_os = "android")]
fn probe_video(path: &Path) -> Result<MediaProbe, DecodeError> {
    use aria_core::VideoDecoder;
    let decoder = crate::MediaCodecDecoder::open(path, OutputMode::Cpu)?;
    let has_audio = crate::android::has_audio_track(path);
    Ok(MediaProbe::video(
        decoder.info(),
        Tracks {
            has_audio,
            ..Tracks::default()
        },
    ))
}

#[cfg(not(any(
    target_vendor = "apple",
    target_os = "windows",
    target_os = "android",
    target_os = "linux"
)))]
fn probe_video(_path: &Path) -> Result<MediaProbe, DecodeError> {
    Err(DecodeError::unsupported(
        "no native video decoder for this platform",
    ))
}

/// Duration of the first audio track, for the audio-only fallback probe.
/// `None` when the platform can't report one (the fallback then propagates
/// the original video-open error).
#[cfg(target_vendor = "apple")]
fn audio_track_duration(path: &Path) -> Option<RationalTime> {
    crate::apple::audio_track_duration(path)
}

#[cfg(target_os = "windows")]
fn audio_track_duration(path: &Path) -> Option<RationalTime> {
    crate::wmf::audio_track_duration(path)
}

#[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
fn audio_track_duration(_path: &Path) -> Option<RationalTime> {
    None
}
