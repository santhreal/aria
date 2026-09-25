//! What a proxy pass is allowed to change about a source, and what it is not.
//!
//! The class these close: a proxy that is not the same content. The renderer
//! substitutes a proxy for a source's decode while placement geometry and UVs
//! still come from the model's dimensions, so any change to framing, aspect,
//! duration, or rate makes the preview show a picture the export does not have.
//! Resolution is the only property a proxy may differ in, so every assertion
//! here is either "the size shrank" or "nothing else moved".
//!
//! What they do not catch: whether the encoder writes the frames it is handed.
//! That is the export path's own coverage, and it needs a GPU.

use aria_models::{CanvasSettings, MediaSource, Rational};

use super::{PROXY_MAX_EDGE, proxy_project, proxy_size};

fn video(width: u32, height: u32, frames: i64) -> MediaSource {
    MediaSource::new(
        "/tmp/proxy-fixture.mp4",
        width,
        height,
        Rational::FPS_24,
        frames,
        true,
    )
}

#[test]
fn the_long_edge_comes_down_to_the_cap_and_the_aspect_does_not_move() {
    assert_eq!(proxy_size(3840, 2160, 960), Some([960, 540]));
    assert_eq!(proxy_size(1080, 1920, 960), Some([540, 960]));
    assert_eq!(proxy_size(1920, 1920, 960), Some([960, 960]));
    // 21:9 rounds down on the short edge rather than up: 960 / (21/9) = 411.4.
    assert_eq!(proxy_size(2520, 1080, 960), Some([960, 410]));
}

#[test]
fn a_source_under_the_cap_is_not_upscaled() {
    // A proxy exists to be cheaper to decode. Enlarging one would cost more
    // than the original for every frame of the edit.
    assert_eq!(proxy_size(640, 360, 960), Some([640, 360]));
    assert_eq!(proxy_size(960, 540, 960), Some([960, 540]));
    // An odd source size still has to be even to encode.
    assert_eq!(proxy_size(641, 361, 960), Some([640, 360]));
}

#[test]
fn a_size_that_is_not_a_canvas_is_refused_rather_than_clamped() {
    // Clamping one edge into range would change the aspect ratio, which is the
    // one thing a proxy may not do — so these are refusals, not repairs.
    assert_eq!(proxy_size(0, 1080, 960), None);
    assert_eq!(proxy_size(1920, 0, 960), None);
    assert_eq!(proxy_size(1920, 1080, 0), None);
    // Below the canvas floor: 15×9 rounds to 14×8, and 8 is under MIN_DIM.
    assert!(proxy_size(15, 9, 960).is_none());
    assert!(
        CanvasSettings::MIN_DIM > 8,
        "the case above assumes the floor"
    );
    // Over the ceiling only when nothing scaled it: the cap is what keeps a
    // 12000-pixel source inside the bound.
    assert!(proxy_size(12_000, 6_750, 0).is_none());
    assert_eq!(proxy_size(12_000, 6_750, 960), Some([960, 540]));
}

#[test]
fn the_pass_renders_the_whole_source_at_the_source_rate() {
    let source = video(1920, 1080, 480);
    let project = proxy_project(source, PROXY_MAX_EDGE).expect("a proxy project");

    assert_eq!(project.timeline().frame_rate, Rational::FPS_24);
    assert_eq!(
        project.timeline().canvas().resolution,
        Some([PROXY_MAX_EDGE, 540])
    );
    let clips: Vec<_> = project
        .timeline()
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .collect();
    assert_eq!(clips.len(), 1, "a proxy pass is one clip, not a timeline");
    let clip = clips[0];
    let range = clip.source_range().expect("media-backed");
    assert_eq!(range.start.value, 0, "the pass starts at the first frame");
    assert_eq!(range.duration.value, 480, "and ends at the last one");
    assert_eq!(clip.timeline.start.value, 0);
    assert_eq!(clip.timeline.duration.value, 480);
    assert_eq!(clip.speed, Rational::new(1, 1), "a proxy is not retimed");
    assert!(!clip.reversed);
}

#[test]
fn a_source_a_proxy_cannot_help_is_refused_by_name() {
    let audio = MediaSource::new("/tmp/proxy-fixture.wav", 0, 0, Rational::FPS_24, 480, true);
    let refusal = proxy_project(audio, PROXY_MAX_EDGE).expect_err("no picture");
    assert!(
        refusal.to_string().contains("no picture"),
        "the refusal must name the reason: {refusal}"
    );

    let still = MediaSource::image("/tmp/proxy-fixture.png", 4000, 3000);
    let refusal = proxy_project(still, PROXY_MAX_EDGE).expect_err("a still");
    assert!(
        refusal.to_string().contains("still"),
        "the refusal must name the reason: {refusal}"
    );

    let empty = video(1920, 1080, 0);
    let refusal = proxy_project(empty, PROXY_MAX_EDGE).expect_err("no frames");
    assert!(
        refusal.to_string().contains("no frames"),
        "the refusal must name the reason: {refusal}"
    );

    let unencodable = video(15, 9, 480);
    let refusal = proxy_project(unencodable, PROXY_MAX_EDGE).expect_err("not a canvas");
    assert!(
        refusal.to_string().contains("15×9"),
        "the refusal must name the size it could not use: {refusal}"
    );
}
