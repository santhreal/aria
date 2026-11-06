//! What the pool says about a file's codec.
//!
//! The class these close: the pool stated shape, length, and audio presence but
//! never the codec, and the tempting fix is to read the file extension — which
//! names the container, not the codec inside it. These assert the two honest
//! outcomes (a name the demuxer gave, or a stated unknown) for every row kind,
//! including the audio-only row, which has no video codec to report at all.

use crate::app::media::{MediaFacts, codec_label, media_facts};
use crate::types::{UiClipKind, UiMediaItem};
use aria_models::Rational;

fn item(kind: UiClipKind) -> UiMediaItem {
    UiMediaItem {
        id: 1,
        name: "clip".into(),
        path: "/tmp/clip.mkv".into(),
        width: 1920,
        height: 1080,
        frame_rate: Rational::new(24, 1),
        duration_ticks: 48,
        duration_label: "00:00:02:00".into(),
        has_audio: true,
        kind,
        still: false,
        missing: false,
        label: None,
        imported_at: None,
        video_codec: None,
        audio_codec: None,
        bin: None,
        proxied: false,
        position: None,
    }
}

#[test]
fn an_unreported_codec_is_stated_not_guessed() {
    assert_eq!(codec_label(None), "Unknown");
    // An empty or blank string is a backend reporting nothing, not a codec.
    assert_eq!(codec_label(Some("")), "Unknown");
    assert_eq!(codec_label(Some("   ")), "Unknown");
    assert_eq!(codec_label(Some(" H.264 ")), "H.264");
}

/// The facts a row draws, as its two lines.
fn lines(picture: &str, streams: Option<&str>) -> MediaFacts {
    MediaFacts {
        picture: picture.to_string(),
        streams: streams.map(str::to_string),
    }
}

#[test]
fn a_row_names_both_codecs_when_the_demuxer_did() {
    let mut media = item(UiClipKind::Media);
    media.video_codec = Some("H.264".into());
    media.audio_codec = Some("MPEG-4 AAC".into());
    let facts = media_facts(&media);
    assert_eq!(
        facts,
        lines("1920×1080 · 00:00:02:00", Some("H.264 · MPEG-4 AAC"))
    );
    assert_eq!(facts.line(), "1920×1080 · 00:00:02:00 · H.264 · MPEG-4 AAC");
}

#[test]
fn a_row_with_no_codec_still_says_whether_there_is_sound() {
    // The audio fact was the row's whole point before codecs existed; adding a
    // codec must not cost it on a file whose codec is unknown.
    let media = item(UiClipKind::Media);
    assert_eq!(
        media_facts(&media),
        lines("1920×1080 · 00:00:02:00", Some("Audio"))
    );
    let mut silent = item(UiClipKind::Media);
    silent.has_audio = false;
    assert_eq!(
        media_facts(&silent),
        lines("1920×1080 · 00:00:02:00", Some("Silent"))
    );
}

/// A still states no sound, with or without a proxy: an image has none, so a
/// `Silent` on every image row is noise beside the movies it is there to flag.
/// A silent movie still reads `Silent`.
#[test]
fn a_still_row_states_no_sound() {
    for has_audio in [false, true] {
        let mut still = item(UiClipKind::Media);
        still.still = true;
        still.has_audio = has_audio;
        still.duration_label = "00:00:05:00".into();
        assert_eq!(
            media_facts(&still),
            lines("1920×1080 · 00:00:05:00", None),
            "a still with has_audio {has_audio}"
        );
        still.proxied = true;
        assert_eq!(
            media_facts(&still),
            lines("1920×1080 · 00:00:05:00", Some("Proxy")),
            "a proxied still with has_audio {has_audio}"
        );
    }
    let mut movie = item(UiClipKind::Media);
    movie.has_audio = false;
    assert_eq!(media_facts(&movie).streams.as_deref(), Some("Silent"));
}

#[test]
fn an_audio_only_row_names_its_own_codec_and_no_video_one() {
    let mut media = item(UiClipKind::Audio);
    media.audio_codec = Some("FLAC".into());
    media.video_codec = Some("nonsense".into());
    media.duration_label = "00:00:30:00".into();
    let facts = media_facts(&media);
    assert_eq!(facts, lines("Audio · 00:00:30:00", Some("FLAC")));
    // An audio file with no codec reported has no streams line to draw.
    media.audio_codec = None;
    let facts = media_facts(&media);
    assert_eq!(facts, lines("Audio · 00:00:30:00", None));
    assert_eq!(facts.line(), "Audio · 00:00:30:00");
}

#[test]
fn a_missing_file_reports_that_before_anything_else() {
    let mut media = item(UiClipKind::Media);
    media.missing = true;
    media.video_codec = Some("H.264".into());
    media.proxied = true;
    assert_eq!(media_facts(&media), lines("Media missing", None));
}

/// The proxy closes the streams line of every kind of row, and an audio file
/// with no codec reported draws a streams line that holds the proxy alone.
#[test]
fn a_proxied_row_names_the_proxy_last_on_its_streams_line() {
    let mut video = item(UiClipKind::Media);
    video.video_codec = Some("H.264".into());
    video.proxied = true;
    assert_eq!(
        media_facts(&video),
        lines("1920×1080 · 00:00:02:00", Some("H.264 · Audio · Proxy"))
    );
    let mut sound = item(UiClipKind::Audio);
    sound.proxied = true;
    assert_eq!(
        media_facts(&sound),
        lines("Audio · 00:00:02:00", Some("Proxy"))
    );
}

#[test]
fn the_properties_block_names_the_codecs_it_has() {
    use aria_plugin::PluginContext;

    use crate::app::AppState;
    use crate::scratch_pool::scratch;

    let mut state = AppState::new(PluginContext::new(), scratch("codec")).expect("state");
    state.new_project().expect("project");
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    let media = state.import_media(fixture).expect("import the still");
    state.select_pool_item(media.raw());

    let rows = state.media_properties();
    let value = |label: &str| {
        rows.iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {label} row"))
    };
    // A still has no codec track, so the block omits the Video codec row
    // rather than printing Unknown or naming PNG, which is the container. Nor
    // does it have a frame rate: the rate it was probed with is the tick base,
    // which read as 1000.000 fps.
    assert!(
        rows.iter().all(|(name, _)| *name != "Video codec"),
        "a still should omit the Video codec row: {rows:?}"
    );
    assert_eq!(value("Frame rate"), "Still image");
    assert_eq!(value("Audio"), "No");

    // A named codec reaches the block as the demuxer wrote it, and a moving
    // picture states its own rate.
    if let Some(entry) = state
        .store_mut()
        .media
        .iter_mut()
        .find(|entry| entry.id == media.raw())
    {
        entry.video_codec = Some("H.264".into());
        entry.audio_codec = Some("MPEG-4 AAC".into());
        entry.has_audio = true;
        entry.still = false;
        entry.frame_rate = Rational::new(24_000, 1_001);
    }
    let rows = state.media_properties();
    let value = |label: &str| {
        rows.iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {label} row"))
    };
    assert_eq!(value("Video codec"), "H.264");
    assert_eq!(value("Frame rate"), "23.976 fps");
    assert_eq!(value("Audio"), "MPEG-4 AAC");

    // A moving picture whose demuxer named no codec states Unknown.
    if let Some(entry) = state
        .store_mut()
        .media
        .iter_mut()
        .find(|entry| entry.id == media.raw())
    {
        entry.video_codec = None;
    }
    let rows = state.media_properties();
    let value = |label: &str| {
        rows.iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {label} row"))
    };
    assert_eq!(value("Video codec"), "Unknown");
}

#[test]
fn add_lane_of_kind_names_every_catalog_kind_from_its_stem() {
    use aria_plugin::PluginContext;

    use crate::app::AppState;
    use crate::scratch_pool::scratch;

    let mut state = AppState::new(PluginContext::new(), scratch("lane-stem")).expect("state");
    state.new_project().expect("project");
    for spec in aria_models::track_kind_specs() {
        let ui_kind = crate::types::UiTrackKind::from_model(spec.kind);
        let ordinal = state
            .store
            .sequence
            .tracks
            .iter()
            .filter(|track| track.kind == ui_kind)
            .count()
            + 1;
        let lane = state
            .add_lane_of_kind(spec.kind, None)
            .expect("add lane of kind");
        let track = state
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.id == lane.raw())
            .expect("added track exists");
        assert_eq!(
            track.name,
            format!("{}{ordinal}", spec.name_stem()),
            "lane of kind {} did not use its catalog stem",
            spec.id
        );
    }
}
