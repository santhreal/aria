//! A media layer whose file is not on disk: the offline slate in a preview,
//! [`RenderError::MediaOffline`] everywhere else.
//!
//! WHY: with a clip's file moved or deleted, the viewer kept the last frame it
//! had drawn, because every render at the playhead failed with a decoder
//! message. A renderer with [`Renderer::set_offline_slate`] on draws the layer
//! as [`OFFLINE_SLATE`] and composites the rest of the frame; with it off, the
//! frame fails and names the missing file rather than the codec. Both export
//! passes render with it off whatever the renderer was set to, and restore it
//! after.
//!
//! Cases run for both media kinds that read a file: a video clip and a still.
//! Covered: the slate pixel and the layer below it, the file named by the
//! error, a file that exists but does not decode (still an error, never the
//! slate), the video export and the frame export with the slate on, and the
//! setting surviving an export. Not covered: a missing file inside a nested
//! sequence, which realizes through the same layer path.
//! Skips cleanly when no GPU adapter is available.

use std::path::{Path, PathBuf};

use aria_core::{Rational, RationalTime};
use aria_models::{CanvasSettings, MediaSource, Project, TrackKind};
use aria_render::{
    OFFLINE_SLATE, PngSequenceEncoder, RenderError, Renderer, export, export_frame_png,
};

const FPS: Rational = Rational::FPS_24;
const FRAMES: i64 = 6;
const BELOW: [u8; 4] = [30, 160, 60, 255];

/// The file kinds a media layer reads.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Video,
    Still,
}

const KINDS: [Kind; 2] = [Kind::Video, Kind::Still];

/// A 320×180 canvas: a green 320×180 picture on the main lane and, on the
/// lane above it, a 64×64 clip of `file`, which is aspect-fit to a 180×180
/// square in the middle. The green picture is written beside `file`.
fn project_over(kind: Kind, file: &Path) -> Project {
    let mut project = Project::new("offline", FPS);
    let mut canvas = CanvasSettings::default();
    canvas.resolution = Some([320, 180]);
    project.timeline_mut().set_canvas(canvas);
    let green = file.with_file_name("below.png");
    write_green(&green);
    let below = project.add_media(MediaSource::image(&green, 320, 180));
    let window = project.media(below).expect("the picture").full_range();
    let lane = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(lane, below, window, RationalTime::new(0, FPS))
        .expect("place the picture");
    let source = match kind {
        Kind::Video => MediaSource::new(file, 64, 64, FPS, FRAMES, false),
        Kind::Still => MediaSource::image(file, 64, 64),
    };
    let media = project.add_media(source);
    let window = project.media(media).expect("the media").full_range();
    let above = project.add_track(TrackKind::Video, "V2");
    project
        .add_clip(above, media, window, RationalTime::new(0, FPS))
        .expect("place the file");
    project
}

fn file_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "clip.mp4",
        Kind::Still => "picture.png",
    }
}

/// A 320×180 PNG filled with [`BELOW`].
fn write_green(path: &Path) {
    let file = std::fs::File::create(path).expect("create the png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 320, 180);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer
        .write_image_data(&BELOW.repeat(320 * 180))
        .expect("png pixels");
}

fn renderer() -> Option<Renderer> {
    match Renderer::new_headless() {
        Ok(renderer) => Some(renderer),
        Err(error) => {
            eprintln!("skipping: no headless GPU available ({error})");
            None
        }
    }
}

fn pixel(image: &aria_compositor::RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().expect("four channels")
}

fn assert_near(actual: [u8; 4], expected: [u8; 4], what: &str) {
    let far = actual.iter().zip(expected).any(|(a, e)| a.abs_diff(e) > 4);
    assert!(!far, "{what}: got {actual:?}, expected ~{expected:?}");
}

fn frame(
    renderer: &mut Renderer,
    project: &Project,
) -> Result<aria_compositor::RgbaImage, RenderError> {
    renderer.render_frame_fit(project, RationalTime::new(2, FPS), 320, 320)
}

fn offline_path(result: &Result<impl std::fmt::Debug, RenderError>) -> Option<PathBuf> {
    match result {
        Err(RenderError::MediaOffline { path }) => Some(path.clone()),
        _ => None,
    }
}

#[test]
fn a_missing_file_draws_the_slate_over_the_rest_of_the_frame_when_on() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    renderer.set_offline_slate(true);
    for kind in KINDS {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(file_name(kind));
        let project = project_over(kind, &file);
        let image = frame(&mut renderer, &project)
            .unwrap_or_else(|error| panic!("{kind:?}: the preview failed: {error}"));
        assert_eq!((image.width, image.height), (320, 180), "{kind:?}");
        assert_near(
            pixel(&image, 160, 90),
            OFFLINE_SLATE,
            &format!("{kind:?} slate"),
        );
        assert_near(
            pixel(&image, 20, 90),
            BELOW,
            &format!("{kind:?} layer below"),
        );
        // The slate is drawn again on the next frame: nothing about the
        // failed open is kept as a frame.
        let again = frame(&mut renderer, &project).expect("the second preview");
        assert_near(
            pixel(&again, 160, 90),
            OFFLINE_SLATE,
            &format!("{kind:?} again"),
        );
    }
}

#[test]
fn a_missing_file_fails_the_frame_and_names_the_file_when_off() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    for kind in KINDS {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(file_name(kind));
        let project = project_over(kind, &file);
        let result = frame(&mut renderer, &project);
        assert_eq!(offline_path(&result), Some(file), "{kind:?}: {result:?}");
    }
}

/// A file that is on disk but does not decode is a broken file, not a moved
/// one: the frame fails with the decoder's error, slate or no slate.
#[test]
fn a_file_that_exists_but_does_not_decode_is_never_the_slate() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    renderer.set_offline_slate(true);
    for kind in KINDS {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(file_name(kind));
        std::fs::write(&file, b"not a picture").expect("write the file");
        let project = project_over(kind, &file);
        let result = frame(&mut renderer, &project);
        assert!(result.is_err(), "{kind:?}: a broken file rendered");
        assert_eq!(offline_path(&result), None, "{kind:?}: {result:?}");
    }
}

/// Both export passes fail on a missing file with the slate on, and leave it
/// on for the preview that follows.
#[test]
fn an_export_fails_on_a_missing_file_and_restores_the_slate() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    renderer.set_offline_slate(true);
    for kind in KINDS {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join(file_name(kind));
        let project = project_over(kind, &file);

        let frames = dir.path().join("frames");
        std::fs::create_dir(&frames).expect("frames dir");
        let mut encoder = PngSequenceEncoder::new(&frames).expect("png encoder");
        let video = export(&mut renderer, &project, &mut encoder);
        assert_eq!(
            offline_path(&video),
            Some(file.clone()),
            "{kind:?} export: {video:?}"
        );

        let still = export_frame_png(
            &mut renderer,
            &project,
            RationalTime::new(2, FPS),
            &dir.path().join("frame.png"),
        );
        assert_eq!(
            offline_path(&still),
            Some(file.clone()),
            "{kind:?} frame: {still:?}"
        );

        let image = frame(&mut renderer, &project).expect("the preview after the exports");
        assert_near(
            pixel(&image, 160, 90),
            OFFLINE_SLATE,
            &format!("{kind:?} after export"),
        );
    }
}
