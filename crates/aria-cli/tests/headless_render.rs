//! Headless render proof: a project built from generators only, rendered without
//! a display, then rendered again.
//!
//! The class closed here is "a render or resolve regression is only visible by
//! eye, on a machine with a screen". These assertions need no media, no window,
//! and no golden file:
//!
//! - **The canvas is the project's canvas.** A resolve that loses the canvas
//!   settings shows up as a differently sized image.
//! - **A stacked layer composites where it was placed.** The solid covers the
//!   frame at one instant and not at another, so z-order and clip extents are
//!   both exercised.
//! - **Two renders of one instant are byte-identical.** Determinism is what
//!   makes any future golden file possible; a renderer that varies per call
//!   cannot be regression-tested at all.
//!
//! Exact pixel values are deliberately not asserted: they are a driver
//! property, and pinning them would make the suite fail on a different GPU
//! rather than on a real regression.

use aria_commands::{Command, EditCommand};
use aria_core::{Rational, RationalTime, TimeRange};
use aria_engine::{Engine, EngineConfig};
use aria_models::{Generator, TrackKind};
use aria_render::Renderer;

/// A project with a red solid over `[0, 2)` s on a 24 fps timeline.
///
/// The lane kind comes from `TrackKind::for_generator`, because a lane is not
/// free to hold any generator: a solid belongs on the graphic-overlay stack, and
/// hardcoding `Video` here would test the error path instead of the render.
fn project() -> aria_models::Project {
    let generator = Generator::SolidColor {
        rgba: [220, 30, 30, 255],
    };
    let kind = TrackKind::for_generator(&generator).expect("a solid has no lane kind");
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine
        .apply(Command::Edit(EditCommand::AddTrack {
            kind,
            name: "S1".into(),
            index: None,
            pinned: true,
        }))
        .expect("add track");
    let track = engine
        .project()
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == kind)
        .expect("the lane was not added")
        .id;
    engine
        .apply(Command::Edit(EditCommand::AddGenerated {
            track,
            generator,
            timeline: TimeRange::at_rate(0, 48, Rational::FPS_24),
        }))
        .expect("add generated");
    engine.project().clone()
}

#[test]
fn a_generated_project_renders_headlessly_and_deterministically() {
    let project = project();
    let mut renderer = match Renderer::new_headless() {
        Ok(renderer) => renderer,
        // A machine with no usable GPU cannot answer this question either way;
        // failing here would report a missing device as a render regression.
        Err(err) => {
            eprintln!("skipping: no headless renderer here ({err})");
            return;
        }
    };
    let rate = Rational::FPS_24;

    let inside = renderer
        .render_frame(&project, RationalTime::new(24, rate))
        .expect("render inside the clip");
    let canvas = aria_render::canvas_size(&project);
    assert_eq!(
        (inside.width, inside.height),
        canvas,
        "the frame is not the project canvas"
    );
    assert_eq!(
        inside.pixels.len(),
        (canvas.0 * canvas.1 * 4) as usize,
        "the frame is not RGBA at its own size"
    );

    // The solid covers the canvas: the centre pixel is opaque and red-dominant.
    let centre = (((canvas.1 / 2) * canvas.0 + canvas.0 / 2) * 4) as usize;
    let pixel = &inside.pixels[centre..centre + 4];
    assert_eq!(pixel[3], 255, "the solid did not composite opaquely");
    assert!(
        pixel[0] > pixel[1] && pixel[0] > pixel[2],
        "the solid composited as {pixel:?}, which is not red-dominant"
    );

    // Same instant, second render: identical bytes.
    let again = renderer
        .render_frame(&project, RationalTime::new(24, rate))
        .expect("render inside the clip again");
    assert_eq!(
        inside.pixels, again.pixels,
        "two renders of one instant differ"
    );

    // Past the clip there is nothing to composite, so the canvas is the
    // background rather than the solid.
    let after = renderer
        .render_frame(&project, RationalTime::new(72, rate))
        .expect("render past the clip");
    assert_ne!(
        after.pixels, inside.pixels,
        "the frame past the clip still shows it"
    );
}

#[test]
fn a_fit_render_scales_down_and_never_up() {
    let project = project();
    let mut renderer = match Renderer::new_headless() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping: no headless renderer here ({err})");
            return;
        }
    };
    let rate = Rational::FPS_24;
    let canvas = aria_render::canvas_size(&project);

    let small = renderer
        .render_frame_fit(&project, RationalTime::new(0, rate), 640, 360)
        .expect("fit render");
    assert!(
        small.width <= 640 && small.height <= 360,
        "a fit render exceeded its box: {}x{}",
        small.width,
        small.height
    );

    // A box larger than the canvas leaves the canvas alone: a preview never
    // upscales, because the extra pixels carry no detail.
    let large = renderer
        .render_frame_fit(
            &project,
            RationalTime::new(0, rate),
            canvas.0 * 2,
            canvas.1 * 2,
        )
        .expect("oversized fit render");
    assert_eq!(
        (large.width, large.height),
        canvas,
        "a fit render upscaled past the canvas"
    );
}
