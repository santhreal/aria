//! Export Frame: one composited frame written as a PNG at the canvas size.
//!
//! Covers the size contract (the canvas, not the export sheet's size), the
//! pixel content at the requested time, replacement of an existing file with
//! no staging directory left beside it, and a destination that cannot be
//! written leaving nothing behind. Skips cleanly when no GPU adapter is
//! available.
//!
//! Not covered: a render that fails after the staging file exists. The
//! staging and publication path is the one `TransactionalFile` tests own.

use aria_core::RationalTime;
use aria_models::{CanvasSettings, Generator, Project, Rational, TimeRange, TrackKind};
use aria_render::{Renderer, export_frame_png};

const FPS_24: Rational = Rational::FPS_24;

/// Red for frames 0..2, blue for 2..4, on a 320×180 canvas.
fn two_colour_project() -> Project {
    let mut project = Project::new("frame", FPS_24);
    let mut canvas = CanvasSettings::default();
    canvas.resolution = Some([320, 180]);
    project.timeline_mut().set_canvas(canvas);
    let track = project.add_track(TrackKind::Sticker, "S1");
    for (start, rgba) in [(0, [220, 30, 30, 255]), (2, [30, 30, 220, 255])] {
        project
            .add_generated(
                track,
                Generator::SolidColor { rgba },
                TimeRange::at_rate(start, 2, FPS_24),
            )
            .expect("solid clip");
    }
    project
}

fn decode(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(path).expect("the exported frame opens"),
    ));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("PNG pixels");
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

fn centre(width: u32, height: u32, pixels: &[u8]) -> [u8; 3] {
    let at = (((height / 2) * width + width / 2) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

#[test]
fn the_frame_at_the_requested_time_is_written_at_the_canvas_size() {
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let project = two_colour_project();
    let dir = tempfile::tempdir().expect("scratch dir");

    for (frame, name, dominant) in [(1, "red.png", 0), (3, "blue.png", 2)] {
        let path = dir.path().join(name);
        let size = export_frame_png(
            &mut renderer,
            &project,
            RationalTime::new(frame, FPS_24),
            &path,
        )
        .expect("the frame exports");
        assert_eq!(size, (320, 180), "frame {frame} left the canvas size");

        let (width, height, pixels) = decode(&path);
        assert_eq!(
            (width, height),
            (320, 180),
            "{name} decoded at another size"
        );
        let rgb = centre(width, height, &pixels);
        let strongest = (0..3)
            .max_by_key(|&channel| rgb[channel])
            .expect("three channels");
        assert_eq!(
            strongest, dominant,
            "frame {frame} wrote {rgb:?}, which is not the clip under the playhead"
        );
    }
}

#[test]
fn an_existing_file_is_replaced_and_no_staging_directory_is_left() {
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let project = two_colour_project();
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("still.png");
    std::fs::write(&path, b"an older file").expect("seed the destination");

    export_frame_png(&mut renderer, &project, RationalTime::new(0, FPS_24), &path)
        .expect("the frame exports over the old file");

    assert_eq!(decode(&path).0, 320, "the old bytes survived the export");
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list the folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        entries,
        vec![std::ffi::OsString::from("still.png")],
        "the export left more than the frame beside it"
    );
}

#[test]
fn a_destination_in_a_missing_folder_fails_and_creates_nothing() {
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping: no headless GPU available");
        return;
    };
    let project = two_colour_project();
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("absent").join("still.png");

    let error = export_frame_png(&mut renderer, &project, RationalTime::new(0, FPS_24), &path)
        .expect_err("a missing folder cannot take the frame");

    assert!(
        matches!(error, aria_render::RenderError::Io(_)),
        "expected an I/O error, got {error}"
    );
    assert_eq!(
        std::fs::read_dir(dir.path())
            .expect("list the folder")
            .count(),
        0,
        "the failed export created something"
    );
}
