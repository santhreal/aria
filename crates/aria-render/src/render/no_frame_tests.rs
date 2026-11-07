//! A source that ends before the media pool holds it to end is named in the
//! render error, by the file and the second it ran out at.
//!
//! WHY: the error printed the media's id and the raw source time,
//! `no frame available for media MediaId(1) at RationalTime { value: 38,
//! rate: Rational { num: 30, den: 1 } }`, so `aria export` over a truncated
//! copy of a clip stated neither the file nor a time a person reads. The case
//! plays a copy of the fixture cut to its first 20000 bytes, which the pool
//! still holds as three seconds, frame by frame from the start, as an export
//! does.
//!
//! Not caught: a source cut so short that no decoder opens, which fails as a
//! decode error before any frame.

use std::path::PathBuf;

use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::*;

const FPS_30: Rational = Rational::FPS_30;

#[test]
fn a_source_that_ends_early_is_named_by_file_and_second() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4");
    let dir = tempfile::tempdir().expect("tempdir");
    let cut = dir.path().join("cut.mp4");
    let bytes = std::fs::read(&fixture).expect("read the fixture");
    std::fs::write(&cut, &bytes[..20_000]).expect("write the cut copy");

    let mut project = Project::new("ends early", FPS_30);
    let media = project.add_media(MediaSource::new(cut.clone(), 640, 360, FPS_30, 90, false));
    let track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 90, FPS_30),
            RationalTime::new(0, FPS_30),
        )
        .expect("place the clip");
    let Ok(mut renderer) = Renderer::new_headless() else {
        return;
    };

    let failed = (0..90).find_map(|tick| {
        renderer
            .render_frame_fit(&project, RationalTime::new(tick, FPS_30), 160, 90)
            .err()
            .map(|error| (tick, error))
    });
    let Some((tick, error)) = failed else {
        panic!("every frame of a 20000-byte cut of a 3 s file rendered");
    };
    assert!(tick > 0, "the cut copy rendered no frame: {error}");
    let RenderError::NoFrame { path, time } = &error else {
        panic!("frame {tick}: {error:?}");
    };
    assert_eq!((path, time.value), (&cut, tick), "{error}");
    assert_eq!(
        error.to_string(),
        format!(
            "{} has no frame at {:.3} s; the file ends early, so relink a whole copy or trim the clip",
            cut.display(),
            tick as f64 / 30.0
        )
    );
}
