//! Preview: import → add clip → get_frame, plus the live-preview overrides
//! (gesture transform, inspector generator) that render without touching
//! project state.

mod common;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_models::{ClipSource, ClipTransform, Generator, TrackKind};
use aria_render::RgbaImage;
use common::{import_asset, rt, small_video_asset, temp_engine, tr};

/// H.264-style even rounding used by the auto canvas.
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

fn pixel(frame: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.width + x) * 4) as usize;
    [
        frame.pixels[i],
        frame.pixels[i + 1],
        frame.pixels[i + 2],
        frame.pixels[i + 3],
    ]
}

#[test]
fn get_frame_returns_rgba_for_placed_clip() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media: media_id,
            source: tr(0, 48),
            start: rt(0),
        }))
        .expect("add clip");

    // The auto canvas follows the largest video media (even-rounded).
    let (width, height) = {
        let media = engine.project().media(media_id).expect("media");
        (even(media.width), even(media.height))
    };
    let frame = engine.get_frame(rt(0)).expect("get_frame");

    assert_eq!(frame.width, width);
    assert_eq!(frame.height, height);
    assert_eq!(
        frame.pixels.len(),
        usize::try_from(width * height * 4).unwrap()
    );
    assert!(
        frame.pixels.iter().any(|&b| b != 0),
        "frame should not be blank"
    );
}

#[test]
fn get_frame_after_split_still_decodes() {
    let Some(path) = small_video_asset() else {
        return;
    };
    let (_dir, mut engine) = temp_engine();
    let media_id = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");

    let clip_id = common::add_media_clip(&mut engine, track, media_id, tr(0, 48), rt(0));
    engine
        .apply(Command::Edit(EditCommand::SplitClip {
            clip: clip_id,
            at: rt(24),
        }))
        .expect("split");

    let frame = engine.get_frame(rt(0)).expect("frame at head");
    assert!(frame.pixels.iter().any(|&b| b != 0));
}

#[test]
fn get_frame_returns_black_when_timeline_empty() {
    let (_dir, mut engine) = temp_engine();
    let frame = engine.get_frame(rt(0)).expect("gap frame");
    // No media anywhere: the auto canvas falls back to 1920×1080.
    assert_eq!(frame.width, 1920);
    assert_eq!(frame.height, 1080);
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255])
    );
}

#[test]
fn get_frame_renders_solid_generated_clip() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");

    common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [10, 20, 30, 255],
        },
        tr(0, 48),
    );

    let frame = engine.get_frame(rt(0)).expect("solid frame");
    assert_eq!(frame.width, 1920);
    assert_eq!(frame.height, 1080);
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [10, 20, 30, 255])
    );
}

#[test]
fn get_frame_places_transformed_solid() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");

    let clip_id = common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 40, 10, 255],
        },
        tr(0, 48),
    );

    // Half size, content center moved to the canvas's top-left quadrant
    // center: the solid covers exactly [0, w/2) × [0, h/2).
    engine
        .apply(Command::Edit(EditCommand::SetClipTransform {
            clip: clip_id,
            transform: ClipTransform {
                position: [-0.25, -0.25],
                scale: 0.5.into(),
                ..ClipTransform::IDENTITY
            },
            at: None,
        }))
        .expect("set transform");

    let frame = engine.get_frame(rt(0)).expect("transformed frame");
    let (w, h) = (frame.width, frame.height);

    assert_eq!(
        pixel(&frame, w / 4, h / 4),
        [200, 40, 10, 255],
        "inside placed quad"
    );
    assert_eq!(
        pixel(&frame, 10, 10),
        [200, 40, 10, 255],
        "top-left corner covered"
    );
    assert_eq!(
        pixel(&frame, 3 * w / 4, 3 * h / 4),
        [0, 0, 0, 255],
        "rest of canvas stays black"
    );
    assert_eq!(
        pixel(&frame, 3 * w / 4, h / 4),
        [0, 0, 0, 255],
        "right of the quad is black"
    );
}

#[test]
fn transform_override_previews_without_touching_state() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");

    let clip_id = common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 40, 10, 255],
        },
        tr(0, 48),
    );
    let could_undo_before = engine.can_undo();

    // Live-drag override: half size in the top-left quadrant. Rendering
    // honors it...
    engine.set_transform_override(Some((
        clip_id,
        ClipTransform {
            position: [-0.25, -0.25],
            scale: 0.5.into(),
            ..ClipTransform::IDENTITY
        },
    )));
    let frame = engine.get_frame(rt(0)).expect("override frame");
    let (w, h) = (frame.width, frame.height);
    assert_eq!(
        pixel(&frame, w / 4, h / 4),
        [200, 40, 10, 255],
        "override placed quad"
    );
    assert_eq!(
        pixel(&frame, 3 * w / 4, 3 * h / 4),
        [0, 0, 0, 255],
        "rest stays black"
    );

    // ...but the project and history never saw it: session state only.
    let committed = &engine.project().clip(clip_id).expect("clip").transform;
    assert!(committed.is_identity(), "project transform untouched");
    assert_eq!(engine.can_undo(), could_undo_before, "no history entry");

    // Clearing restores the committed (full-canvas) render.
    engine.set_transform_override(None);
    let frame = engine.get_frame(rt(0)).expect("committed frame");
    let (w, h) = (frame.width, frame.height);
    assert_eq!(
        pixel(&frame, 3 * w / 4, 3 * h / 4),
        [200, 40, 10, 255],
        "solid covers canvas again"
    );
}

#[test]
fn generator_override_previews_without_touching_state() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");

    let clip_id = common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 40, 10, 255],
        },
        tr(0, 48),
    );
    let could_undo_before = engine.can_undo();

    // Live inspector edit (e.g. dragging a color slider): render the
    // substituted generator...
    engine.set_generator_override(Some((
        clip_id,
        Generator::SolidColor {
            rgba: [10, 200, 40, 255],
        },
    )));
    let frame = engine.get_frame(rt(0)).expect("override frame");
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [10, 200, 40, 255])
    );

    // ...while the committed generator and history stay untouched.
    let clip = engine.project().clip(clip_id).expect("clip");
    match &clip.content {
        ClipSource::Generated(generator) => assert_eq!(
            *generator,
            Generator::SolidColor {
                rgba: [200, 40, 10, 255],
            },
            "project generator untouched"
        ),
        other => panic!("unexpected content {other:?}"),
    }
    assert_eq!(engine.can_undo(), could_undo_before, "no history entry");

    // Clearing restores the committed render.
    engine.set_generator_override(None);
    let frame = engine.get_frame(rt(0)).expect("committed frame");
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [200, 40, 10, 255])
    );
}

#[test]
fn new_session_clears_overrides() {
    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");
    let clip_id = common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 40, 10, 255],
        },
        tr(0, 48),
    );
    engine.set_transform_override(Some((
        clip_id,
        ClipTransform {
            scale: 0.5.into(),
            ..ClipTransform::IDENTITY
        },
    )));
    engine.set_generator_override(Some((
        clip_id,
        Generator::SolidColor {
            rgba: [10, 200, 40, 255],
        },
    )));

    // A fresh session must not leak the old session's live overrides onto
    // whatever clip is created next (ids restart, so they could collide).
    engine.new_session();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");
    common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [10, 20, 30, 255],
        },
        tr(0, 48),
    );

    let frame = engine.get_frame(rt(0)).expect("fresh session frame");
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [10, 20, 30, 255]),
        "committed solid covers the whole canvas — no stale override applied"
    );
}

#[test]
fn media_proxy_registry_sets_clears_and_dies_with_the_session() {
    use aria_models::MediaId;
    use std::path::{Path, PathBuf};

    let (_dir, mut engine) = temp_engine();
    let media = MediaId::from_raw(7);

    engine.set_media_proxy(media, PathBuf::from("proxy.mp4"));
    assert_eq!(engine.media_proxy(media), Some(Path::new("proxy.mp4")));

    engine.clear_media_proxy(media);
    assert_eq!(engine.media_proxy(media), None);

    // Media ids persist in project files: a fresh session must not inherit
    // the old session's substitutions (id 7 could name a different file).
    engine.set_media_proxy(media, PathBuf::from("proxy.mp4"));
    engine.new_session();
    assert_eq!(engine.media_proxy(media), None);
}

/// Previews read a registered proxy while proxies are on and the original
/// while they are off; an export, which always reads the originals, leaves the
/// switch where it was; a proxy missing from disk reads the original.
///
/// Defends the Viewer's Toggle Proxies button against an export turning
/// proxies back on, and against a deleted proxy file failing every frame.
/// The export step runs only where the build has an encoder.
#[test]
fn the_preview_proxy_switch_holds_across_export_and_a_missing_proxy_reads_the_original() {
    use std::path::PathBuf;

    let Some(path) = small_video_asset() else {
        return;
    };
    let proxy = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../aria-decoder/tests/fixtures/opengop_h264.mp4");
    let (dir, mut engine) = temp_engine();
    let media = import_asset(&mut engine, &path);
    let track = common::add_track(&mut engine, TrackKind::Video, "V1");
    engine
        .apply(Command::Edit(EditCommand::AddClip {
            track,
            media,
            source: tr(0, 48),
            start: rt(0),
        }))
        .expect("add clip");
    let original = engine.get_frame(rt(0)).expect("original frame");
    assert!(engine.preview_proxies(), "previews use proxies by default");

    engine.set_media_proxy(media, proxy);
    let proxied = engine.get_frame(rt(0)).expect("proxy frame");
    assert_ne!(proxied.pixels, original.pixels, "the proxy was not read");

    let revision = engine.proxy_revision();
    engine.set_preview_proxies(false);
    assert!(!engine.preview_proxies());
    assert_eq!(engine.proxy_revision(), revision + 1);
    engine.set_preview_proxies(false);
    assert_eq!(engine.proxy_revision(), revision + 1, "no change, no bump");
    assert_eq!(
        engine.get_frame(rt(0)).expect("off").pixels,
        original.pixels
    );

    let _ = engine.apply(Command::Project(ProjectCommand::Export {
        path: dir.path().join("out.mp4"),
        format: aria_commands::ExportFormat::PROJECT,
    }));
    assert!(!engine.preview_proxies());
    assert_eq!(
        engine.get_frame(rt(0)).expect("after export").pixels,
        original.pixels,
        "the export turned proxies back on"
    );

    engine.set_preview_proxies(true);
    assert_eq!(engine.get_frame(rt(0)).expect("on").pixels, proxied.pixels);

    engine.set_media_proxy(media, dir.path().join("deleted-proxy.mp4"));
    assert_eq!(
        engine
            .get_frame(rt(0))
            .expect("a missing proxy reads the original")
            .pixels,
        original.pixels
    );
}

/// Rows the compositor writes, kept as an image: what a Viewer sink receives.
#[derive(Default)]
struct Rows(Option<RgbaImage>);

impl aria_render::FrameSink for Rows {
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        let len = (width * height * 4) as usize;
        &mut self
            .0
            .insert(RgbaImage::new(width, height, vec![0; len]))
            .pixels
    }
}

/// The Viewer's transparency grid: with preview transparency on, the two
/// entries the Viewer renders through clear the uncovered canvas to alpha 0
/// and keep covered pixels opaque; `get_frame`, which thumbnails and mask
/// tracking read, keeps the background; and the switch moves
/// `proxy_revision`, so a Viewer cache keyed on it drops frames from the other
/// mode.
#[test]
fn preview_transparency_clears_the_background_only_in_the_viewer_entries() {
    use aria_render::SeekPolicy;

    let (_dir, mut engine) = temp_engine();
    let track = common::add_track(&mut engine, TrackKind::Sticker, "T1");
    let clip_id = common::add_generated(
        &mut engine,
        track,
        Generator::SolidColor {
            rgba: [200, 40, 10, 255],
        },
        tr(0, 48),
    );
    // The solid covers the top-left quarter of the canvas.
    engine
        .apply(Command::Edit(EditCommand::SetClipTransform {
            clip: clip_id,
            transform: ClipTransform {
                position: [-0.25, -0.25],
                scale: 0.5.into(),
                ..ClipTransform::IDENTITY
            },
            at: None,
        }))
        .expect("set transform");
    let fit = |engine: &mut aria_engine::Engine| {
        let mut rows = Rows::default();
        engine
            .get_frame_fit_into(rt(0), 960, 540, SeekPolicy::Exact, &mut rows)
            .expect("fit frame");
        rows.0.expect("the fit entry wrote no rows")
    };
    let full = |engine: &mut aria_engine::Engine| {
        let mut rows = Rows::default();
        engine
            .get_frame_into(rt(0), SeekPolicy::Exact, &mut rows)
            .expect("full frame");
        rows.0.expect("the full entry wrote no rows")
    };

    assert!(!engine.preview_transparency(), "the grid is off by default");
    let opaque = fit(&mut engine);
    assert_eq!(
        pixel(&opaque, opaque.width - 10, opaque.height - 10),
        [0, 0, 0, 255]
    );

    let revision = engine.proxy_revision();
    engine.set_preview_transparency(true);
    assert!(engine.preview_transparency());
    assert_eq!(engine.proxy_revision(), revision + 1);
    engine.set_preview_transparency(true);
    assert_eq!(engine.proxy_revision(), revision + 1, "no change, no bump");

    for (entry, frame) in [("fit", fit(&mut engine)), ("full", full(&mut engine))] {
        let (w, h) = (frame.width, frame.height);
        assert_eq!(
            pixel(&frame, w - 10, h - 10),
            [0, 0, 0, 0],
            "{entry}: the uncovered canvas kept the background"
        );
        assert_eq!(
            pixel(&frame, w / 4, h / 4),
            [200, 40, 10, 255],
            "{entry}: a covered pixel lost its colour"
        );
    }
    let thumbnail = engine.get_frame(rt(0)).expect("thumbnail frame");
    assert_eq!(
        pixel(&thumbnail, thumbnail.width - 10, thumbnail.height - 10),
        [0, 0, 0, 255],
        "get_frame is not a Viewer entry and keeps the background"
    );

    engine.set_preview_transparency(false);
    assert_eq!(fit(&mut engine).pixels, opaque.pixels);
}
