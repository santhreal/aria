use super::StickerSequence;
use super::api::media_scene;
use aria_core::{RationalTime, RgbaImage};
use aria_models::{MediaSource, Project, Rational};

fn seq(delays_ms: &[u32]) -> StickerSequence {
    StickerSequence::new(
        delays_ms
            .iter()
            .map(|_| RgbaImage::new(1, 1, vec![0; 4]))
            .collect(),
        delays_ms,
    )
}

#[test]
fn sticker_frame_selection_walks_delays_and_loops() {
    let s = seq(&[100, 50, 100]);
    assert_eq!(s.frame_at(0.0), 0);
    assert_eq!(s.frame_at(0.099), 0);
    assert_eq!(s.frame_at(0.100), 1);
    assert_eq!(s.frame_at(0.149), 1);
    assert_eq!(s.frame_at(0.150), 2);
    // Loops at total (250 ms) and clamps negatives to the first frame.
    assert_eq!(s.frame_at(0.250), 0);
    assert_eq!(s.frame_at(0.601), 1);
    assert_eq!(s.frame_at(-1.0), 0);
}

#[test]
fn static_stickers_always_show_frame_zero() {
    let s = seq(&[100]);
    assert_eq!(s.frame_at(12.34), 0);
}

#[test]
fn sticker_lookup_is_logarithmic_in_frame_count() {
    let delays = vec![1; 16_384];
    let sequence = seq(&delays);
    let (index, comparisons) = super::media_cache::sticker_frame_index(&sequence.ends_ms, 12_345);
    assert_eq!(index, 12_345);
    assert!(
        comparisons <= 15,
        "{comparisons} comparisons exceeded the logarithmic bound"
    );
}

#[test]
fn direct_media_scene_preserves_project_identity_without_timeline() {
    let rate = Rational::FPS_24;
    let mut project = Project::new("media scene", rate);
    let _other = project.add_media(MediaSource::new("other.mp4", 640, 360, rate, 120, false));
    let media = project.add_media(MediaSource::new("unused.mp4", 1920, 1080, rate, 240, false));
    let at = RationalTime::new(23, rate);
    let scene = media_scene(&project, media, at).expect("media scene");

    assert_eq!(scene.layers.len(), 1);
    match &scene.layers[0].source {
        crate::LayerSource::Media {
            media: scene_media,
            source_time,
        } => {
            assert_eq!(*scene_media, media);
            assert_eq!(*source_time, at);
        }
        other => panic!("expected media layer, got {other:?}"),
    }
    assert_eq!(project.timeline().tracks_ordered().count(), 0);
}
