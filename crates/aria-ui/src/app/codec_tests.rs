//! What the pool says about a file's codec.
//!
//! The class these close: the pool stated shape, length, and audio presence but
//! never the codec, and the tempting fix is to read the file extension — which
//! names the container, not the codec inside it. These assert the two honest
//! outcomes (a name the demuxer gave, or a stated unknown) for every row kind,
//! including the audio-only row, which has no video codec to report at all.

use crate::app::media::{codec_label, media_caption};
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
    assert_eq!(codec_label(None), "unknown");
    // An empty or blank string is a backend reporting nothing, not a codec.
    assert_eq!(codec_label(Some("")), "unknown");
    assert_eq!(codec_label(Some("   ")), "unknown");
    assert_eq!(codec_label(Some(" H.264 ")), "H.264");
}

#[test]
fn a_row_names_both_codecs_when_the_demuxer_did() {
    let mut media = item(UiClipKind::Media);
    media.video_codec = Some("H.264".into());
    media.audio_codec = Some("MPEG-4 AAC".into());
    let caption = media_caption(&media);
    assert_eq!(
        caption,
        "1920×1080  ·  00:00:02:00  ·  H.264  ·  MPEG-4 AAC"
    );
}

#[test]
fn a_row_with_no_codec_still_says_whether_there_is_sound() {
    // The audio fact was the row's whole point before codecs existed; adding a
    // codec must not cost it on a file whose codec is unknown.
    let media = item(UiClipKind::Media);
    assert_eq!(media_caption(&media), "1920×1080  ·  00:00:02:00  ·  audio");
    let mut silent = item(UiClipKind::Media);
    silent.has_audio = false;
    assert_eq!(
        media_caption(&silent),
        "1920×1080  ·  00:00:02:00  ·  silent"
    );
}

#[test]
fn an_audio_only_row_names_its_own_codec_and_no_video_one() {
    let mut media = item(UiClipKind::Audio);
    media.audio_codec = Some("FLAC".into());
    media.video_codec = Some("nonsense".into());
    media.duration_label = "00:00:30:00".into();
    let caption = media_caption(&media);
    assert_eq!(caption, "Audio  ·  00:00:30:00  ·  FLAC");
    assert!(
        !caption.contains("nonsense"),
        "an audio-only row has no picture to describe"
    );
}

#[test]
fn a_missing_file_reports_that_before_anything_else() {
    let mut media = item(UiClipKind::Media);
    media.missing = true;
    media.video_codec = Some("H.264".into());
    assert_eq!(media_caption(&media), "media missing");
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
    // A still carries no codec track, so the row states unknown rather than
    // naming PNG, which is the container.
    assert_eq!(value("Video codec"), "unknown");
    assert_eq!(value("Audio"), "no");

    // A named codec reaches the block as the demuxer wrote it.
    if let Some(entry) = state
        .store_mut()
        .media
        .iter_mut()
        .find(|entry| entry.id == media.raw())
    {
        entry.video_codec = Some("H.264".into());
        entry.audio_codec = Some("MPEG-4 AAC".into());
        entry.has_audio = true;
    }
    let rows = state.media_properties();
    let value = |label: &str| {
        rows.iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {label} row"))
    };
    assert_eq!(value("Video codec"), "H.264");
    assert_eq!(value("Audio"), "MPEG-4 AAC");
}
