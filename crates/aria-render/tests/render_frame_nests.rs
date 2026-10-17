//! `render_frame` tests covering nested sequences: a sequence clip draws every
//! lane of its sequence, through every level of nesting, and then draws as one
//! picture with the clip's own placement and look.
//!
//! The class this closes: a sequence clip drew only its topmost clip, and a
//! nest drawn before the nests it shows read a target nothing had drawn yet.
//! What this does not catch: a nest whose sequence differs from the canvas it
//! is placed on, which the resolver's own tests pin.

use super::*;
use aria_core::RgbaImage;
use aria_models::{SequenceId, TrackId};

const CANVAS: [u32; 2] = [320, 180];

fn sized() -> CanvasSettings {
    CanvasSettings {
        resolution: Some(CANVAS),
        ..CanvasSettings::default()
    }
}

/// A solid of `rgba` on a lane of its own at `scale`, its center `position`
/// from the canvas center.
fn solid(project: &mut Project, rgba: [u8; 4], position: [f32; 2], scale: f32) {
    let lane = project.add_track(TrackKind::Sticker, "S");
    let clip = project
        .add_generated(
            lane,
            Generator::SolidColor { rgba },
            TimeRange::at_rate(0, 100, FPS_24),
        )
        .unwrap();
    project
        .set_transform(
            clip,
            ClipTransform {
                position,
                scale: scale.into(),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
}

/// A red left half and a blue right half, each half the canvas in size and
/// in the middle band.
fn halves(project: &mut Project) {
    solid(project, [255, 0, 0, 255], [-0.25, 0.0], 0.5);
    solid(project, [0, 0, 255, 255], [0.25, 0.0], 0.5);
}

/// A green bar across the top of the canvas, clear of [`halves`].
fn bar(project: &mut Project) {
    solid(project, [0, 255, 0, 255], [0.0, -0.375], 0.25);
}

/// A new sequence on the test canvas, filled by `fill` while it is open.
fn new_sequence(project: &mut Project, fill: impl FnOnce(&mut Project)) -> SequenceId {
    let open = project.active_sequence();
    let sequence = project.create_sequence("Nest", FPS_24);
    project.set_active_sequence(Some(sequence)).unwrap();
    project.timeline_mut().set_canvas(sized());
    fill(project);
    project.set_active_sequence(open).unwrap();
    sequence
}

/// `sequence` placed over ticks 0..100 on a new video lane of the open
/// timeline.
fn place(project: &mut Project, sequence: SequenceId) -> aria_models::ClipId {
    let lane: TrackId = project.add_track(TrackKind::Video, "V");
    project
        .add_sequence_clip(lane, sequence, TimeRange::at_rate(0, 100, FPS_24), rt(0))
        .unwrap()
}

fn project() -> Project {
    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(sized());
    project
}

fn renderer() -> Option<(Renderer, i32)> {
    let gpu = GpuContext::new_headless_blocking().ok()?;
    let tol = pixel_tol(&gpu);
    Some((Renderer::with_gpu(gpu.share()), tol))
}

fn assert_same(got: &RgbaImage, want: &RgbaImage, tol: i32, what: &str) {
    assert_eq!((got.width, got.height), (want.width, want.height), "{what}");
    for y in 0..want.height {
        for x in 0..want.width {
            assert_px_close(
                got.pixel(x, y),
                want.pixel(x, y),
                tol,
                &format!("{what} at ({x}, {y})"),
            );
        }
    }
}

#[test]
fn a_nested_sequence_draws_as_its_lanes_drawn_flat() {
    let Some((mut renderer, tol)) = renderer() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let mut flat = project();
    halves(&mut flat);
    let mut nested = project();
    let sequence = new_sequence(&mut nested, halves);
    place(&mut nested, sequence);
    let want = renderer.render_frame(&flat, rt(0)).expect("render flat");
    let got = renderer.render_frame(&nested, rt(0)).expect("render nest");
    assert_same(&got, &want, tol, "the nest");
}

/// A frame fitted into another shape letterboxes a nest with the rest of the
/// frame, so the nest's picture covers part of the target it draws onto.
#[test]
fn a_letterboxed_nest_draws_as_its_lanes_drawn_flat() {
    let Some((mut renderer, tol)) = renderer() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let mut flat = project();
    halves(&mut flat);
    let mut nested = project();
    let sequence = new_sequence(&mut nested, halves);
    place(&mut nested, sequence);
    let boxed = |renderer: &Renderer, project: &Project| {
        let mut scene = renderer.resolve_frame(project, rt(0)).expect("resolve");
        scene.fit_into(CANVAS[0], CANVAS[0]);
        scene
    };
    let (flat_scene, nested_scene) = (boxed(&renderer, &flat), boxed(&renderer, &nested));
    let want = renderer
        .render_scene(&flat, &flat_scene)
        .expect("render flat");
    let got = renderer
        .render_scene(&nested, &nested_scene)
        .expect("render nest");
    assert_same(&got, &want, tol, "the letterboxed nest");
}

/// A nest's target starts every frame empty, so a frame shows none of the
/// nest the frame before it drew.
#[test]
fn a_nest_target_starts_each_frame_empty() {
    let Some((mut renderer, tol)) = renderer() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let mut flat = project();
    bar(&mut flat);
    let mut before = project();
    let sequence = new_sequence(&mut before, halves);
    place(&mut before, sequence);
    let mut after = project();
    let sequence = new_sequence(&mut after, bar);
    place(&mut after, sequence);
    let want = renderer.render_frame(&flat, rt(0)).expect("render flat");
    renderer
        .render_frame(&before, rt(0))
        .expect("render the halves");
    let got = renderer
        .render_frame(&after, rt(0))
        .expect("render the bar");
    assert_same(&got, &want, tol, "the frame after another nest");
}

/// A sequence that shows a sequence draws the inner one first, so the outer
/// one reads a picture rather than an empty target.
#[test]
fn a_nest_inside_a_nest_draws_every_level() {
    let Some((mut renderer, tol)) = renderer() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let mut flat = project();
    halves(&mut flat);
    bar(&mut flat);
    let mut nested = project();
    let inner = new_sequence(&mut nested, halves);
    let outer = new_sequence(&mut nested, |project| {
        place(project, inner);
        bar(project);
    });
    place(&mut nested, outer);
    let want = renderer.render_frame(&flat, rt(0)).expect("render flat");
    let got = renderer.render_frame(&nested, rt(0)).expect("render nest");
    assert_same(&got, &want, tol, "the nest of a nest");
}

#[test]
fn a_nest_draws_with_its_clips_scale_and_opacity() {
    let Some((mut renderer, tol)) = renderer() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let mut nested = project();
    let sequence = new_sequence(&mut nested, halves);
    let clip = place(&mut nested, sequence);
    nested
        .set_transform(
            clip,
            ClipTransform {
                scale: 0.5.into(),
                opacity: 0.5,
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    let image = renderer.render_frame(&nested, rt(0)).expect("render");
    // The nest covers x 80..240, y 45..135; its halves the middle band of it.
    for (at, rgba, what) in [
        ([120, 90], [128, 0, 0, 255], "red half at half opacity"),
        ([200, 90], [0, 0, 128, 255], "blue half at half opacity"),
        ([120, 50], [0, 0, 0, 255], "the nest's empty band"),
        ([40, 90], [0, 0, 0, 255], "outside the nest"),
    ] {
        assert_px_close(image.pixel(at[0], at[1]), rgba, tol, what);
    }
}
