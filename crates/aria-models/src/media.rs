use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::{BinId, MediaId};
use crate::time::{Rational, RationalTime, TimeRange, check_same_rate};

/// Tick rate for still images, which have no frame cadence of their own.
/// Millisecond ticks (like audio-only sources) keep duration math exact.
pub const STILL_TICK_RATE: Rational = Rational::new(1000, 1);

/// Default pool duration for still images: 5 seconds at
/// [`STILL_TICK_RATE`], so a library drop places a CapCut-style 5s clip.
pub const STILL_DEFAULT_DURATION_TICKS: i64 = 5_000;

/// What a [`MediaSource`] fundamentally is, derived from its fields —
/// see [`MediaSource::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

/// An imported source file in the project's media pool.
///
/// This is the *asset*, not a placement on the timeline; many [`Clip`](crate::Clip)s
/// can reference the same `MediaSource`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaSource {
    pub id: MediaId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Native frame rate of the source.
    pub frame_rate: Rational,
    /// Total length of the source at [`frame_rate`](Self::frame_rate).
    pub duration: RationalTime,
    pub has_audio: bool,
    /// Still image (PNG/JPEG/WebP): one frame shown for the clip's whole
    /// extent. `duration` is the *default placement length* (5s), not an
    /// intrinsic property. Absent from saves while false, so old files
    /// load unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_image: bool,
    /// Operator grouping colour for the bin row, from the clip label set.
    /// Absent from saves while unset, so old files load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<crate::LabelColor>,
    /// Unix seconds when the file entered the pool, so a pool can be ordered by
    /// date added. Absent from saves made before it existed; such an entry has
    /// no import time rather than a fabricated one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<u64>,
    /// Codec names as the probe read them, for the pool row and the properties
    /// block. `None` means the platform's demuxer could not name one, which is
    /// stated as unknown rather than guessed from the extension. Absent from
    /// saves while unset, so old files load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    /// The pool folder this source sits in, or `None` for the pool root. Absent
    /// from saves while unset, so old files load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinId>,
    /// Optional parent media if this source is a subclip created from another source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<MediaId>,
    /// Source in/out window within the parent media if this is a subclip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<TimeRange>,
    /// User-visible name if explicitly set (e.g. for subclips or renamed media).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// `&bool` is the signature `skip_serializing_if` requires.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// Wall-clock seconds, or nothing if the host clock is before the epoch. A pool
/// entry with no time sorts last rather than pretending to be the oldest.
fn now_unix() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

impl MediaSource {
    /// Create a media source with a freshly allocated [`MediaId`].
    pub fn new(
        path: impl Into<PathBuf>,
        width: u32,
        height: u32,
        frame_rate: Rational,
        duration_ticks: i64,
        has_audio: bool,
    ) -> Self {
        let duration_ticks = duration_ticks.max(0);
        Self {
            id: MediaId::next(),
            path: path.into(),
            width,
            height,
            frame_rate,
            duration: RationalTime::new(duration_ticks, frame_rate),
            has_audio,
            is_image: false,
            label: None,
            imported_at: now_unix(),
            video_codec: None,
            audio_codec: None,
            bin: None,
            parent: None,
            source_range: None,
            name: None,
        }
    }

    /// Create a still-image source (PNG/JPEG/WebP) with the default 5s
    /// placement length at [`STILL_TICK_RATE`].
    pub fn image(path: impl Into<PathBuf>, width: u32, height: u32) -> Self {
        Self {
            is_image: true,
            ..Self::new(
                path,
                width,
                height,
                STILL_TICK_RATE,
                STILL_DEFAULT_DURATION_TICKS,
                false,
            )
        }
    }

    /// The full extent of the source as `[0, duration)`.
    pub fn full_range(&self) -> TimeRange {
        TimeRange::at_rate(0, self.duration.value, self.frame_rate)
    }

    /// Whether this source has no video stream (music/voiceover files).
    /// Probing reports such sources with zero dimensions.
    pub fn is_audio_only(&self) -> bool {
        self.width == 0 && self.has_audio
    }

    pub fn kind(&self) -> MediaKind {
        if self.is_image {
            MediaKind::Image
        } else if self.is_audio_only() {
            MediaKind::Audio
        } else {
            MediaKind::Video
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a subclip from a parent media source and an in/out range.
    pub fn subclip(
        parent: &MediaSource,
        name: impl Into<String>,
        source_range: TimeRange,
    ) -> Result<Self, ModelError> {
        check_same_rate(source_range.start.rate, parent.frame_rate)?;
        if source_range.is_empty() {
            return Err(ModelError::InvalidRange);
        }
        if source_range.start.value < 0
            || (!parent.is_image && source_range.end_tick() > parent.duration.value)
        {
            return Err(ModelError::SourceOutOfBounds);
        }
        let duration = source_range.duration;
        Ok(Self {
            id: MediaId::next(),
            path: parent.path.to_path_buf(),
            width: parent.width,
            height: parent.height,
            frame_rate: parent.frame_rate,
            duration,
            has_audio: parent.has_audio,
            is_image: parent.is_image,
            label: parent.label,
            imported_at: now_unix(),
            video_codec: parent.video_codec.as_ref().map(|s| s.to_string()),
            audio_codec: parent.audio_codec.as_ref().map(|s| s.to_string()),
            bin: parent.bin,
            parent: Some(parent.id),
            source_range: Some(source_range),
            name: Some(name.into()),
        })
    }

    /// Whether this media source is a subclip.
    pub fn is_subclip(&self) -> bool {
        self.parent.is_some()
    }

    /// The display name of this source: its explicit name, or the file stem of its path.
    pub fn display_name(&self) -> &str {
        if let Some(name) = &self.name {
            name.as_str()
        } else {
            self.path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const R24: Rational = Rational::FPS_24;
    const R30: Rational = Rational::FPS_30;

    fn sample(path: &str, duration: i64) -> MediaSource {
        MediaSource::new(path, 1920, 1080, R24, duration, true)
    }

    // --- MediaSource::new ---------------------------------------------------

    #[test]
    fn new_wires_all_fields() {
        let media = MediaSource::new("/media/clip.mp4", 3840, 2160, R30, 1_800, false);

        assert_eq!(media.path, PathBuf::from("/media/clip.mp4"));
        assert_eq!(media.width, 3840);
        assert_eq!(media.height, 2160);
        assert_eq!(media.frame_rate, R30);
        assert_eq!(media.duration, RationalTime::new(1_800, R30));
        assert!(!media.has_audio);
        assert!(media.id.raw() >= 1);
    }

    #[test]
    fn new_accepts_path_buf_and_str() {
        let from_str = MediaSource::new("a.mp4", 1, 1, R24, 10, true);
        let from_buf = MediaSource::new(PathBuf::from("b.mp4"), 1, 1, R24, 10, true);
        assert_eq!(from_str.path(), Path::new("a.mp4"));
        assert_eq!(from_buf.path(), Path::new("b.mp4"));
    }

    #[test]
    fn new_assigns_distinct_media_ids() {
        let a = sample("a.mp4", 100);
        let b = sample("b.mp4", 100);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn new_clamps_negative_duration_to_zero() {
        let media = sample("short.mp4", -50);
        assert_eq!(media.duration.value, 0);
        assert_eq!(media.duration.rate, R24);
    }

    #[test]
    fn new_zero_duration_is_valid() {
        let media = sample("empty.mp4", 0);
        assert_eq!(media.duration.value, 0);
        assert!(media.full_range().is_empty());
    }

    #[test]
    fn new_duration_carries_frame_rate() {
        let ntsc = MediaSource::new("ntsc.mp4", 1920, 1080, Rational::FPS_23_976, 2_400, true);
        assert_eq!(ntsc.frame_rate, Rational::FPS_23_976);
        assert_eq!(ntsc.duration.rate, Rational::FPS_23_976);
        assert_eq!(ntsc.duration.value, 2_400);
    }

    #[test]
    fn new_has_audio_flag() {
        assert!(sample("with-audio.mp4", 10).has_audio);
        assert!(!MediaSource::new("silent.mp4", 1, 1, R24, 10, false).has_audio);
    }

    // --- full_range -------------------------------------------------------

    #[test]
    fn full_range_spans_entire_source() {
        let media = sample("clip.mp4", 500);
        assert_eq!(media.full_range(), TimeRange::at_rate(0, 500, R24));
    }

    #[test]
    fn full_range_matches_duration_at_native_rate() {
        let media = MediaSource::new("clip.mp4", 1280, 720, R30, 300, true);
        let range = media.full_range();
        assert_eq!(range.start, RationalTime::new(0, R30));
        assert_eq!(range.duration, media.duration);
        assert_eq!(range.end_tick(), 300);
    }

    #[test]
    fn full_range_zero_duration_is_empty() {
        let media = sample("blank.mp4", 0);
        let range = media.full_range();
        assert!(range.is_empty());
        assert_eq!(range.start.value, 0);
        assert_eq!(range.end_tick(), 0);
    }

    // --- path -------------------------------------------------------------

    #[test]
    fn path_returns_borrowed_path() {
        let media = sample("/vault/footage/take_01.mov", 100);
        assert_eq!(media.path(), Path::new("/vault/footage/take_01.mov"));
        assert_eq!(
            media.path().file_name().and_then(|s| s.to_str()),
            Some("take_01.mov")
        );
    }

    // --- Clone / Eq / Debug -----------------------------------------------

    #[test]
    fn clone_and_eq_preserve_all_fields() {
        let original = MediaSource::new("x.mp4", 640, 480, R24, 42, true);
        let cloned = original.clone();
        assert_eq!(original, cloned);
        assert_eq!(original.id, cloned.id);
        assert_eq!(original.duration, cloned.duration);
    }

    #[test]
    fn eq_requires_matching_id_and_metadata() {
        let a = MediaSource::new("same.mp4", 1920, 1080, R24, 100, true);
        let b = MediaSource::new("same.mp4", 1920, 1080, R24, 100, true);
        // Distinct `next()` ids => not equal even with identical metadata.
        assert_ne!(a, b);

        let fixed_a = MediaSource {
            id: MediaId::from_raw(1),
            path: PathBuf::from("fixed.mp4"),
            width: 1920,
            height: 1080,
            frame_rate: R24,
            duration: RationalTime::new(100, R24),
            has_audio: true,
            is_image: false,
            label: None,
            imported_at: Some(1_700_000_000),
            video_codec: Some("H.264".into()),
            audio_codec: Some("MPEG-4 AAC".into()),
            bin: Some(crate::BinId::from_raw(4)),
            parent: None,
            source_range: None,
            name: None,
        };
        let fixed_b = fixed_a.clone();
        assert_eq!(fixed_a, fixed_b);
    }

    #[test]
    fn debug_includes_path_and_dimensions() {
        let media = sample("debug.mp4", 10);
        let s = format!("{media:?}");
        assert!(s.contains("debug.mp4"));
        assert!(s.contains("1920"));
        assert!(s.contains("1080"));
    }

    // --- image stills -------------------------------------------------------

    #[test]
    fn image_constructor_defaults_to_five_seconds() {
        let media = MediaSource::image("/photos/sunset.png", 4000, 3000);
        assert!(media.is_image);
        assert_eq!(media.kind(), MediaKind::Image);
        assert_eq!(media.width, 4000);
        assert_eq!(media.height, 3000);
        assert!(!media.has_audio);
        assert_eq!(media.frame_rate, STILL_TICK_RATE);
        assert_eq!(media.duration.value, STILL_DEFAULT_DURATION_TICKS);
        // 5000 ticks at 1000/1 is exactly 5 seconds.
        let seconds = media.duration.value as f64 * media.duration.rate.seconds_per_unit();
        assert_eq!(seconds, 5.0);
        assert!(!media.is_audio_only());
    }

    #[test]
    fn kind_distinguishes_video_audio_image() {
        assert_eq!(sample("v.mp4", 10).kind(), MediaKind::Video);
        let audio = MediaSource::new("a.mp3", 0, 0, Rational::new(1000, 1), 10, true);
        assert_eq!(audio.kind(), MediaKind::Audio);
        assert_eq!(MediaSource::image("i.jpg", 8, 8).kind(), MediaKind::Image);
    }

    #[test]
    fn non_image_serializes_without_image_field() {
        let media = sample("plain.mp4", 10);
        let json = serde_json::to_string(&media).unwrap();
        assert!(!json.contains("is_image"));

        let image = MediaSource::image("pic.png", 64, 64);
        let json = serde_json::to_string(&image).unwrap();
        assert!(json.contains("\"is_image\":true"));
    }

    #[test]
    fn image_flag_roundtrips_and_defaults_false() {
        let image = MediaSource::image("pic.webp", 320, 240);
        let json = serde_json::to_string(&image).unwrap();
        let loaded: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, image);

        // Old saves carry no `is_image` field — it must default to false.
        let legacy = r#"{"id":1,"path":"old.mp4","width":1920,"height":1080,
            "frame_rate":{"num":24,"den":1},
            "duration":{"value":48,"rate":{"num":24,"den":1}},"has_audio":true}"#;
        let loaded: MediaSource = serde_json::from_str(legacy).unwrap();
        assert!(!loaded.is_image);
        assert_eq!(loaded.kind(), MediaKind::Video);
    }

    #[test]
    fn a_new_source_records_when_it_was_imported() {
        let media = sample("fresh.mp4", 10);
        let at = media.imported_at.expect("an import records its time");
        let now = now_unix().expect("clock after the epoch");
        assert!(at <= now && now - at < 600, "{at} is not near {now}");
    }

    #[test]
    fn the_import_time_roundtrips_and_is_absent_from_old_saves() {
        let media = sample("dated.mp4", 10);
        let json = serde_json::to_string(&media).unwrap();
        assert!(json.contains("imported_at"));
        let loaded: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, media);

        let legacy = r#"{"id":1,"path":"old.mp4","width":1920,"height":1080,
            "frame_rate":{"num":24,"den":1},
            "duration":{"value":48,"rate":{"num":24,"den":1}},"has_audio":true}"#;
        let loaded: MediaSource = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            loaded.imported_at, None,
            "an undated save must stay undated rather than gain a fake time"
        );
        let json = serde_json::to_string(&loaded).unwrap();
        assert!(
            !json.contains("imported_at"),
            "an undated entry must not write the field back"
        );
    }

    /// A codec name is a fact from the probe, not a default: an entry that never
    /// carried one must keep saying nothing rather than write an empty string or
    /// a guess back into the file.
    #[test]
    fn codec_names_roundtrip_and_stay_absent_when_unknown() {
        let mut media = sample("coded.mp4", 10);
        assert_eq!(media.video_codec, None, "a bare constructor names no codec");
        let json = serde_json::to_string(&media).unwrap();
        assert!(!json.contains("video_codec"));
        assert!(!json.contains("audio_codec"));

        media.video_codec = Some("H.264".to_string());
        media.audio_codec = Some("MPEG-4 AAC".to_string());
        let json = serde_json::to_string(&media).unwrap();
        let loaded: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, media);
        assert_eq!(loaded.video_codec.as_deref(), Some("H.264"));

        let legacy = r#"{"id":1,"path":"old.mp4","width":1920,"height":1080,
            "frame_rate":{"num":24,"den":1},
            "duration":{"value":48,"rate":{"num":24,"den":1}},"has_audio":true}"#;
        let loaded: MediaSource = serde_json::from_str(legacy).unwrap();
        assert_eq!(loaded.video_codec, None);
        assert_eq!(loaded.audio_codec, None);
    }
}
