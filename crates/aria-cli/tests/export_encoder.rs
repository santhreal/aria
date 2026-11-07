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

/// WHY: `aria export --fps` took only whole numbers, so a timeline could not
/// be exported at 29.97, a rate `aria new` offers. The file written with
/// `--fps 29.97` reads back at `30000/1001`, and holds the frames of the half
/// second at that rate. Not caught: a player that rounds the rate it reads.
#[test]
fn an_export_at_29_97_writes_the_exact_broadcast_rate() {
    if Renderer::new_headless().is_err() {
        return;
    }
    let dir = scratch("export-rate");
    let project = project();
    if aria_encoder::open_encoder(&dir.join("probe.mp4"), export_config(&project)).is_err() {
        return;
    }
    let path = dir.join("cut.aria");
    project.save_to_file(&path).expect("save the project");
    let out = dir.join("cut.mp4");
    let output = aria([
        OsStr::new("export"),
        path.as_os_str(),
        out.as_os_str(),
        OsStr::new("--fps"),
        OsStr::new("29.97"),
        OsStr::new("--size"),
        OsStr::new("160x90"),
        OsStr::new("--quiet"),
    ]);
    assert!(
        output.status.success(),
        "aria export --fps 29.97 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("wrote 15 frames of 160x90 "),
        "stdout:\n{stdout}"
    );
    let probe = aria_decoder::probe(&out).expect("probe the export");
    assert_eq!(
        (probe.frame_rate, probe.frame_count),
        (Rational::FPS_29_97, 15)
    );
}

/// WHY: the `wrote` line read `MP4 · H.264 / AAC` for an export of a project
/// with nothing audible, a file the export writes with no sound track: the line
/// was built from the Audio setting, not from the file. Each case reads the
/// file back, and the line states the sound track the file holds: none for a
/// silent timeline and for `--no-audio`, AAC for a timeline with a tone on it.
/// Not caught: a track holding silence, which no case here writes.
#[test]
fn the_wrote_line_states_the_sound_track_the_file_holds() {
    if Renderer::new_headless().is_err() {
        return;
    }
    let dir = scratch("export-sound-line");
    let silent = project();
    if aria_encoder::open_encoder(&dir.join("probe.mp4"), export_config(&silent)).is_err() {
        return;
    }
    let solid = dir.join("solid.aria");
    silent.save_to_file(&solid).expect("save the project");
    let tone = dir.join("tone.aria");
    let wav = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-sine-48k-2s.wav");
    let made = aria([OsStr::new("new"), tone.as_os_str(), wav.as_os_str()]);
    assert!(made.status.success(), "aria new failed: {made:?}");
    let label = |audio| aria_encoder::capability().label_for(Default::default(), audio);
    for (name, path, flag, audio) in [
        ("solid", &solid, "--quiet", false),
        ("tone", &tone, "--quiet", true),
        ("muted", &tone, "--no-audio", false),
    ] {
        let out = dir.join(format!("{name}.mp4"));
        let output = aria([
            OsStr::new("export"),
            path.as_os_str(),
            out.as_os_str(),
            OsStr::new("--to"),
            OsStr::new("0.5"),
            OsStr::new("--size"),
            OsStr::new("160x90"),
            OsStr::new(flag),
        ]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{name}: {output:?}");
        let probe = aria_decoder::probe(&out).expect("probe the export");
        assert_eq!(probe.has_audio, audio, "{name}: the file's sound track");
        let wrote = format!(
            "wrote 12 frames of 160x90 {} to {}",
            label(audio),
            out.display()
        );
        assert_eq!(stdout.lines().next(), Some(wrote.as_str()), "{name}");
    }
}
