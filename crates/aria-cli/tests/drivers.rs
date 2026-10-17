//! `aria frame` evaluates property drivers written in the built-in language.
//!
//! Closes the class "a driver evaluates in the editor and not on the command
//! line": the `aria` process registers `aria.expressions` before it renders,
//! and prints every driver that failed, since a failed driver leaves the
//! keyframed value in the frame. Does not cover drivers in plugin languages,
//! which the command line does not load.

use std::path::{Path, PathBuf};
use std::process::Command;

use aria_models::{ClipParam, DriverCode, Generator, Project, Rational, TimeRange, TrackKind};
use aria_render::{Renderer, decode_png};

const FAILED: &str = "aria: warning: the driver on Backdrop opacity failed";

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("aria-cli-drivers-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the scratch directory");
    dir
}

/// A red solid named "Backdrop" on 0..2 s, its opacity driven by `driver`.
fn project(driver: Option<&str>) -> Project {
    let generator = Generator::SolidColor {
        rgba: [220, 30, 30, 255],
    };
    let kind = TrackKind::for_generator(&generator).expect("a solid has a lane kind");
    let mut project = Project::new("drivers", Rational::FPS_24);
    let track = project.add_track(kind, "Solid");
    let clip = project
        .add_generated(
            track,
            generator,
            TimeRange::at_rate(0, 48, Rational::FPS_24),
        )
        .expect("place the solid");
    project
        .set_clip_name(clip, Some("Backdrop".into()))
        .expect("name the solid");
    if let Some(source) = driver {
        project
            .set_param_driver(
                clip,
                ClipParam::Opacity,
                Some(DriverCode::new("aria.expressions", source)),
            )
            .expect("attach the driver");
    }
    project
}

/// `aria frame` at 0.5 s: the centre pixel and what the command printed on
/// stderr.
fn frame(dir: &Path, name: &str, project: &Project) -> ([u8; 4], String) {
    let path = dir.join(format!("{name}.aria"));
    project.save_to_file(&path).expect("save the project");
    let out = dir.join(format!("{name}.png"));
    let output = Command::new(env!("CARGO_BIN_EXE_aria"))
        .arg("frame")
        .arg(&path)
        .arg(&out)
        .args(["--at", "0.5", "--fit", "64x36"])
        .output()
        .expect("run aria");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{name}: {stderr}");
    let image = decode_png(&std::fs::read(&out).expect("read the frame")).expect("decode it");
    let at = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    let pixel = image.pixels[at..at + 4]
        .try_into()
        .expect("a pixel is four bytes");
    (pixel, stderr)
}

#[test]
fn the_command_line_evaluates_expression_drivers_and_reports_failures() {
    if Renderer::new_headless().is_err() {
        return;
    }
    let dir = scratch("evaluates");
    let (plain, stderr) = frame(&dir, "plain", &project(None));
    assert!(!stderr.contains("aria: warning"), "{stderr}");

    // Opacity 100 is the undriven frame; opacity 0 hides the solid, which a
    // driver that never ran cannot do.
    let (full, stderr) = frame(&dir, "full", &project(Some("50 * 2")));
    assert_eq!(full, plain);
    assert!(!stderr.contains("aria: warning"), "{stderr}");
    let (hidden, stderr) = frame(&dir, "hidden", &project(Some("value * 0")));
    assert_ne!(hidden, plain, "the driver hides the solid");
    assert!(!stderr.contains("aria: warning"), "{stderr}");

    // A failing driver leaves the keyframed opacity, and the command says so.
    let (faulted, stderr) = frame(&dir, "faulted", &project(Some("nope")));
    assert_eq!(faulted, plain);
    let line = format!(
        "{FAILED} and its keyframed value was used: line 1, column 1: nope is not defined\n"
    );
    assert!(stderr.contains(&line), "{stderr}");
}
