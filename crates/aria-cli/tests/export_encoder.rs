//! `aria export` prints the encoder it opened and why the ones before it were
//! skipped.
//!
//! Closes the class "an export ran on an encoder nobody can see": a host whose
//! hardware encoder fails to open falls back to a software one, and the only
//! evidence is the speed of the export. The command prints, after its `wrote`
//! line, the encoder the export opened and one `skipped` line per candidate
//! that did not open, each from the session the export used. The expected text
//! is built here from the fields of an encoder opened in this process, so a
//! change to the printed shape fails as well as a missing line. Does not cover
//! the hardware branch on a host without a working hardware encoder.

mod support;

use std::ffi::OsStr;

use aria_models::{Generator, Project, Rational, TimeRange, TrackKind};
use aria_render::{Renderer, export_config};
use support::{aria, scratch};

/// Half a second of a red solid on a 24 fps timeline.
fn project() -> Project {
    let generator = Generator::SolidColor {
        rgba: [220, 30, 30, 255],
    };
    let kind = TrackKind::for_generator(&generator).expect("a solid has a lane kind");
    let mut project = Project::new("export-encoder", Rational::FPS_24);
    let track = project.add_track(kind, "Solid");
    project
        .add_generated(
            track,
            generator,
            TimeRange::at_rate(0, 12, Rational::FPS_24),
        )
        .expect("place the solid");
    project
}

#[test]
fn the_export_prints_the_encoder_it_opened_and_every_skipped_one() {
    if Renderer::new_headless().is_err() {
        return;
    }
    let dir = scratch("export-prints");
    let project = project();
    let Ok(probe) = aria_encoder::open_encoder(&dir.join("probe.mp4"), export_config(&project))
    else {
        // No encoder opens on this host; `aria export` fails before it prints.
        return;
    };
    let choice = probe.choice();
    drop(probe);
    let processor = match choice.hardware {
        Some(true) => "hardware",
        Some(false) => "software",
        None => "selected by the platform",
    };
    let mut expected = vec![format!("encoder {} ({processor})", choice.encoder)];
    expected.extend(
        choice
            .passed_over
            .iter()
            .map(|reason| format!("skipped {reason}")),
    );

    let path = dir.join("cut.aria");
    project.save_to_file(&path).expect("save the project");
    let out = dir.join("cut.mp4");
    let output = aria([
        OsStr::new("export"),
        path.as_os_str(),
        out.as_os_str(),
        OsStr::new("--size"),
        OsStr::new("160x90"),
        OsStr::new("--quiet"),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "aria export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut lines = stdout.lines();
    let wrote = lines.next().expect("a wrote line");
    assert!(
        wrote.starts_with("wrote 12 frames of 160x90 "),
        "first line: {wrote}"
    );
    assert_eq!(lines.collect::<Vec<_>>(), expected, "stdout:\n{stdout}");
}
