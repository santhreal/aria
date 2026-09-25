//! End-to-end sticker rendering: bundled catalog assets composited on a
//! headless GPU, plus the catalog↔bytes drift check. GPU tests skip cleanly
//! when no adapter is available (CI).

use aria_core::{Rational, RationalTime};
use aria_models::{CanvasAspect, CanvasSettings, Generator, Project, TrackKind, sticker_catalog};
use aria_render::Renderer;

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn pixel(image: &aria_compositor::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * image.width + x) * 4) as usize;
    image.pixels[idx..idx + 4].try_into().unwrap()
}

/// A 16:9 project with one sticker clip centered on a black canvas.
fn sticker_project(asset: &str) -> Project {
    let mut project = Project::new("stickers", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::sticker(asset),
            aria_models::TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project
}

/// Every catalog entry's art must decode to the declared dimensions and
/// animated flag — the drift check between `data/stickers.toml` and the art it
/// names.
#[test]
fn catalog_matches_embedded_asset_bytes() {
    for spec in sticker_catalog() {
        let frames = aria_decoder::decode_animation(&spec.bytes)
            .unwrap_or_else(|e| panic!("sticker '{}' failed to decode: {e}", spec.id));
        let first = &frames[0].image;
        assert_eq!(
            (first.width, first.height),
            (spec.width, spec.height),
            "sticker '{}' catalog dimensions drifted",
            spec.id
        );
        assert_eq!(
            frames.len() > 1,
            spec.animated,
            "sticker '{}' catalog animated flag drifted",
            spec.id
        );
        for frame in &frames {
            assert_eq!(
                (frame.image.width, frame.image.height),
                (first.width, first.height)
            );
        }
    }
}

#[test]
fn renders_a_static_sticker_over_the_background() {
    let project = sticker_project("heart");
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    let frame = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("render sticker");
    assert_eq!((frame.width, frame.height), (640, 360));

    // The 256-reference-px heart sits centered: its interior is the fill
    // color, the canvas corner stays background.
    let center = pixel(&frame, 320, 180);
    assert!(
        center[0] > 150 && center[3] == 255,
        "center should be heart red, got {center:?}"
    );
    assert_eq!(
        pixel(&frame, 10, 10),
        [0, 0, 0, 255],
        "corner is background"
    );

    // Second render hits the decode-once sticker cache: same pixels.
    let again = renderer
        .render_frame_fit(&project, rt(1), 640, 640)
        .expect("render sticker again");
    assert_eq!(frame.pixels, again.pixels);
}

#[test]
fn animated_sticker_frames_advance_with_clip_time() {
    let project = sticker_project("star_spin");
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    // 80 ms per frame: tick 0 (0 ms) shows frame 0, tick 2 (83 ms) frame 1.
    let first = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("render frame 0");
    let second = renderer
        .render_frame_fit(&project, rt(2), 640, 640)
        .expect("render frame 1");
    assert_ne!(
        first.pixels, second.pixels,
        "the spinning star must rotate between animation frames"
    );

    // One full loop later (960 ms = 23.04 ticks ≈ tick 24 at 41.7 ms/tick
    // lands at 40 ms into the loop, still frame 0): same frame as t=0.
    let looped = renderer
        .render_frame_fit(&project, rt(24), 640, 640)
        .expect("render looped");
    assert_eq!(first.pixels, looped.pixels, "animation loops");
}

/// Export path: a sticker timeline encodes to a decodable MP4 whose first
/// frame carries the sticker pixels (Apple-only, like the other export tests).
#[cfg(target_vendor = "apple")]
#[test]
fn exports_a_sticker_timeline_with_visible_pixels() {
    use aria_compositor::{CompositeLayer, Compositor, CompositorConfig, GpuContext};
    use aria_core::VideoDecoder;
    use aria_decoder::{AvfDecoder, OutputMode};

    let project = sticker_project("heart");
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let path = std::env::temp_dir().join("aria_sticker_export.mp4");
    let frames = aria_render::export_to_file(&mut renderer, &project, &path).expect("export");
    assert!(frames > 0);

    let gpu = GpuContext::new_headless_blocking().expect("gpu");
    let mut decoder = AvfDecoder::open(&path, OutputMode::Cpu).expect("reopen export");
    let frame = decoder.next_frame().expect("decode").expect("frame");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(frame.width(), frame.height());
    let layer = CompositeLayer::frame(
        &frame,
        aria_compositor::LayerPlacement::full_canvas(&config),
    );
    let img = comp.render(&gpu, &config, &[layer]).expect("composite");
    let center = img.pixel(960, 540);
    assert!(
        center[0] > 120,
        "exported center should carry the heart, got {center:?}"
    );
    let corner = img.pixel(10, 10);
    assert!(
        corner[0] < 40 && corner[1] < 40,
        "exported corner should stay background, got {corner:?}"
    );

    let _ = std::fs::remove_file(&path);
}
