//! A title's carets, as `Renderer::layer_carets` places them on the canvas,
//! against the pixels `render_frame` draws for the same title.
//!
//! Closes the class where the Viewer draws a text caret somewhere other than
//! the character boundary it edits: a caret that misses the raster padding,
//! the alignment, the layer scale or the rotation lands between the wrong
//! characters. The first character is styled red by a range run, so a run
//! mapped onto the wrong glyphs fails here too. Animated titles are not
//! covered: their glyphs move every frame.

use aria_core::{Rational, RationalTime, RgbaImage};
use aria_models::{
    CanvasSettings, Generator, Param, Project, Scale2, TextAlignH, TextStroke, TextStyle,
    TextStyleRun, TimeRange, TrackKind,
};
use aria_render::{LayerCaret, Renderer, resolve};

const FPS: Rational = Rational::FPS_24;
/// Slack for antialiased edges, in canvas pixels.
const EDGE: f32 = 2.0;

fn renderer() -> Option<Renderer> {
    match Renderer::new_headless() {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            eprintln!("skipping: no headless GPU available: {err}");
            None
        }
    }
}

/// "HI" with the H red and the I white, on a black 320x180 canvas.
fn title(style: TextStyle, rotation: f32, scale: f32) -> Project {
    let mut project = Project::new("caret", FPS);
    let mut canvas = CanvasSettings::default();
    canvas.resolution = Some([320, 180]);
    canvas.background = [0, 0, 0];
    project.timeline_mut().set_canvas(canvas);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "HI".into(),
                style,
            },
            TimeRange::at_rate(0, 24, FPS),
        )
        .expect("title");
    let clip = project.clip_mut(clip).expect("title clip");
    clip.transform.rotation = Param::Constant(rotation);
    clip.transform.scale = Param::Constant(Scale2::uniform(scale));
    project
}

fn red_first(mut style: TextStyle) -> TextStyle {
    style.fill = Param::Constant([255, 255, 255, 255]);
    let mut run = TextStyleRun::new(0, 1);
    run.fill = Some([255, 0, 0, 255]);
    style.runs = vec![run];
    style
}

fn carets_and_frame(renderer: &mut Renderer, project: &Project) -> (Vec<LayerCaret>, RgbaImage) {
    let at = RationalTime::new(0, FPS);
    let scene = resolve(project, at).expect("resolve");
    let canvas = [scene.width as f32, scene.height as f32];
    let layer = scene
        .layers
        .iter()
        .find(|layer| layer.clip.is_some())
        .expect("the title is a layer");
    let carets = renderer
        .layer_carets(layer, canvas)
        .expect("a title has carets");
    let frame = renderer.render_frame(project, at).expect("render");
    (carets, frame)
}

/// Canvas pixel centres of every pixel `keep` accepts.
fn pixels(frame: &RgbaImage, keep: impl Fn([u8; 3]) -> bool) -> Vec<[f32; 2]> {
    let width = frame.width as usize;
    frame
        .pixels
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, px)| keep([px[0], px[1], px[2]]))
        .map(|(i, _)| [(i % width) as f32 + 0.5, (i / width) as f32 + 0.5])
        .collect()
}

fn reddish([r, g, b]: [u8; 3]) -> bool {
    r > 150 && g < 90 && b < 90
}

fn whitish([r, g, b]: [u8; 3]) -> bool {
    r > 150 && g > 150 && b > 150
}

fn stop(carets: &[LayerCaret], byte: usize) -> LayerCaret {
    *carets
        .iter()
        .find(|caret| caret.byte == byte)
        .unwrap_or_else(|| panic!("no caret before byte {byte} in {carets:?}"))
}

/// Signed distance of `point` along the reading direction from the caret's
/// line: positive after it, negative before it.
fn after(caret: LayerCaret, direction: [f32; 2], point: [f32; 2]) -> f32 {
    (point[0] - caret.top[0]) * direction[0] + (point[1] - caret.top[1]) * direction[1]
}

/// The H's pixels all fall before the caret between H and I, the I's all
/// after it, and every pixel of ink lies between the first and last caret.
fn assert_boundaries(carets: &[LayerCaret], frame: &RgbaImage, direction: [f32; 2]) {
    let (first, middle, last) = (stop(carets, 0), stop(carets, 1), stop(carets, 2));
    let red = pixels(frame, reddish);
    let white = pixels(frame, whitish);
    assert!(!red.is_empty(), "the run painted no red H");
    assert!(!white.is_empty(), "the title painted no white I");
    for point in &red {
        let along = after(middle, direction, *point);
        assert!(
            along < EDGE,
            "red H pixel {point:?} is {along} px past the H|I caret {middle:?}"
        );
        let along = after(first, direction, *point);
        assert!(
            along > -EDGE,
            "red H pixel {point:?} is {along} px before the first caret {first:?}"
        );
    }
    for point in &white {
        let along = after(middle, direction, *point);
        assert!(
            along > -EDGE,
            "white I pixel {point:?} is {along} px before the H|I caret {middle:?}"
        );
        let along = after(last, direction, *point);
        assert!(
            along < EDGE,
            "white I pixel {point:?} is {along} px past the last caret {last:?}"
        );
    }
}

#[test]
fn carets_fall_between_the_painted_characters_of_a_padded_title() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    // The stroke pads the raster on every side, so a caret that forgets the
    // padding sits inside the H.
    let mut style = red_first(TextStyle::default());
    style.size = Param::Constant(600.0);
    style.stroke = Some(TextStroke {
        rgba: Param::Constant([0, 0, 255, 255]),
        width: Param::Constant(90.0),
    });
    let project = title(style, 0.0, 1.0);
    let (carets, frame) = carets_and_frame(&mut renderer, &project);
    assert_eq!(
        carets.iter().map(|caret| caret.byte).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for caret in &carets {
        assert!(
            (caret.top[0] - caret.bottom[0]).abs() < 0.01,
            "an upright caret is vertical: {caret:?}"
        );
    }
    assert_boundaries(&carets, &frame, [1.0, 0.0]);
    let ink = pixels(&frame, |px| reddish(px) || whitish(px));
    let (top, bottom) = (stop(&carets, 1).top[1], stop(&carets, 1).bottom[1]);
    for point in &ink {
        assert!(
            point[1] > top - EDGE && point[1] < bottom + EDGE,
            "ink at {point:?} is outside the caret's line {top}..{bottom}"
        );
    }
}

#[test]
fn carets_follow_the_layer_scale_rotation_and_alignment() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let mut style = red_first(TextStyle::default());
    style.size = Param::Constant(300.0);
    style.align_h = TextAlignH::Left;
    // A clockwise quarter turn reads top to bottom on the canvas.
    let project = title(style, 90.0, 1.5);
    let (carets, frame) = carets_and_frame(&mut renderer, &project);
    for caret in &carets {
        assert!(
            (caret.top[1] - caret.bottom[1]).abs() < 0.01,
            "a quarter-turned caret is horizontal: {caret:?}"
        );
    }
    assert_boundaries(&carets, &frame, [0.0, 1.0]);
    // The carets span the ink and not much more: a caret placed at the
    // unscaled size or about the wrong centre misses it.
    let ink = pixels(&frame, |px| reddish(px) || whitish(px));
    let span = |points: &mut dyn Iterator<Item = [f32; 2]>| {
        points.fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        })
    };
    let ink_box = span(&mut ink.iter().copied());
    let caret_box = span(&mut carets.iter().flat_map(|caret| [caret.top, caret.bottom]));
    for axis in 0..2 {
        assert!(
            ink_box[axis] > caret_box[axis] - EDGE
                && ink_box[axis + 2] < caret_box[axis + 2] + EDGE,
            "ink {ink_box:?} leaves the caret box {caret_box:?}"
        );
    }
    let area = |b: [f32; 4]| (b[2] - b[0]) * (b[3] - b[1]);
    assert!(
        area(caret_box) < area(ink_box) * 3.0,
        "the caret box {caret_box:?} is far larger than the ink {ink_box:?}"
    );
}
