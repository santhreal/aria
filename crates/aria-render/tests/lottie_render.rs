//! End-to-end Lottie rendering: a file-backed `Generator::Lottie` clip
//! composited on a headless GPU — parse, capped-fps frame sampling, LRU
//! reuse, and a missing or unparseable file. GPU tests skip cleanly when no
//! adapter is available (CI).

use aria_core::{Rational, RationalTime};
use aria_models::{CanvasAspect, CanvasSettings, Generator, Project, TrackKind};
use aria_render::{ExportSettings, OFFLINE_SLATE, RenderError, Renderer, check_export_media};

const FPS_24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn pixel(image: &aria_compositor::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * image.width + x) * 4) as usize;
    image.pixels[idx..idx + 4].try_into().unwrap()
}

/// 100×100, 30 fps, 2 s: a centered 80×80 red rectangle whose opacity
/// animates 100 → 0 over the composition, so early and late frames differ.
const FADING_RECT: &str = r#"{
  "v": "5.7.0", "fr": 30, "ip": 0, "op": 60, "w": 100, "h": 100,
  "layers": [{
    "ty": 4, "ip": 0, "op": 60, "st": 0,
    "ks": {"o": {"a": 1, "k": [{"t": 0, "s": [100]}, {"t": 60, "s": [0]}]},
           "p": {"a": 0, "k": [50, 50]},
           "a": {"a": 0, "k": [0, 0, 0]}, "s": {"a": 0, "k": [100, 100, 100]},
           "r": {"a": 0, "k": 0}},
    "shapes": [
      {"ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [80, 80]}, "r": {"a": 0, "k": 0}},
      {"ty": "fl", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100}}
    ]
  }]
}"#;

/// A 16:9 project with one Lottie clip centered on a black canvas.
fn lottie_project(path: &str) -> Project {
    let mut project = Project::new("lottie", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::lottie(path, 100, 100),
            aria_models::TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project
}

/// One fixed path per concurrent caller, rewritten before it is read.
fn temp_lottie(source: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let slot = std::env::var("NEXTEST_TEST_GLOBAL_SLOT").unwrap_or_default();
    let path = std::env::temp_dir().join(format!("aria_lottie_test_{slot}_{seq}.json"));
    std::fs::write(&path, source).unwrap();
    path
}

#[test]
fn renders_a_lottie_clip_and_advances_frames() {
    let file = temp_lottie(FADING_RECT);
    let project = lottie_project(file.to_str().unwrap());
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };

    let first = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("render frame 0");
    assert_eq!((first.width, first.height), (640, 360));

    // The 100-reference-px composition sits centered (~33 px at this canvas
    // height); its interior carries the opaque red rect at t=0.
    let center = pixel(&first, 320, 180);
    assert!(
        center[0] > 150,
        "center should carry the red rect, got {center:?}"
    );
    assert_eq!(
        pixel(&first, 10, 10),
        [0, 0, 0, 255],
        "corner is background"
    );

    // 1.5 s in, the rect has faded to 25% opacity — visibly darker.
    let faded = renderer
        .render_frame_fit(&project, rt(36), 640, 640)
        .expect("render faded frame");
    let faded_center = pixel(&faded, 320, 180);
    assert!(
        faded_center[0] < center[0] - 50,
        "opacity animation should darken the composited rect: t0 {center:?} vs t1.5 {faded_center:?}"
    );

    // Same sampled frame (t=0 again) — served from the LRU, identical pixels.
    let again = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("render frame 0 again");
    assert_eq!(first.pixels, again.pixels);

    let _ = std::fs::remove_file(&file);
}

/// WHY: a Lottie clip whose file was missing drew nothing in the preview and
/// exported blank with no error, because the renderer cached the failed load
/// as "draw nothing". A missing file draws the offline slate in a preview,
/// fails a frame rendered without the slate and the export media check, and
/// is looked for again on the next frame, so moving the file back restores
/// the animation. Not covered: a file deleted after it parsed, which keeps
/// drawing from the parsed composition.
#[test]
fn a_missing_lottie_file_slates_the_preview_fails_the_export_and_recovers() {
    let file = temp_lottie(FADING_RECT);
    std::fs::remove_file(&file).unwrap();
    let project = lottie_project(file.to_str().unwrap());

    let settings = ExportSettings::for_project(&project);
    match check_export_media(&project, settings, true) {
        Err(RenderError::FileOffline { path }) => assert_eq!(path, file),
        other => panic!("the export check should name the Lottie file, got {other:?}"),
    }
    check_export_media(&project, settings, false).expect("a sound export reads no Lottie");

    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    match renderer.render_frame_fit(&project, rt(0), 640, 640) {
        Err(RenderError::FileOffline { path }) => assert_eq!(path, file),
        Err(other) => panic!("expected the missing Lottie file, got {other}"),
        Ok(_) => panic!("a missing Lottie file must fail a frame rendered without the slate"),
    }

    renderer.set_offline_slate(true);
    let slate = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("the slate draws");
    assert_eq!(pixel(&slate, 320, 180), OFFLINE_SLATE);
    assert_eq!(
        pixel(&slate, 10, 10),
        [0, 0, 0, 255],
        "corner is background"
    );

    std::fs::write(&file, FADING_RECT).unwrap();
    let restored = renderer
        .render_frame_fit(&project, rt(0), 640, 640)
        .expect("the restored file draws");
    let center = pixel(&restored, 320, 180);
    assert!(
        center[0] > 200 && center[1] < 30,
        "the restored animation draws its red rect, got {center:?}"
    );
    let _ = std::fs::remove_file(&file);
}

/// WHY: a Lottie file that did not parse drew nothing, silently, in the
/// preview and the export. It fails the frame with the file and the parser's
/// reason, with the offline slate on, on every frame rather than only the one
/// that first parsed it.
#[test]
fn a_lottie_file_that_does_not_parse_fails_every_frame() {
    let file = temp_lottie("{ \"v\": ");
    let project = lottie_project(file.to_str().unwrap());
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        let _ = std::fs::remove_file(&file);
        return;
    };
    renderer.set_offline_slate(true);
    for frame in [0, 1] {
        match renderer.render_frame_fit(&project, rt(frame), 640, 640) {
            Err(error @ RenderError::FileUnusable { .. }) => {
                let text = error.to_string();
                assert!(
                    text.starts_with(&format!("{} cannot be used (", file.display())),
                    "{text}"
                );
            }
            Err(other) => panic!("expected the unusable Lottie file, got {other}"),
            Ok(_) => panic!("frame {frame}: a Lottie file that does not parse must fail"),
        }
    }
    let _ = std::fs::remove_file(&file);
}
