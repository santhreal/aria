//! A signal stops `aria export` between frames, and the export leaves its
//! output folder as it found it.
//!
//! WHY: SIGTERM ended an export mid-encode with status 143 and left
//! `.aria-export-<pid>-…/export.mp4`, a partial file, beside the output: the
//! command installed no signal handler, and the staging directory is removed
//! only when the export returns. Each case signals a running export once its
//! staging directory exists, then holds the exit status, the error line, and a
//! folder that contains only the project. Does not cover SIGKILL, which no
//! handler receives, or a second signal, which ends the process at once.

mod support;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aria_models::{Generator, Project, Rational, TimeRange, TrackKind};
use aria_render::Renderer;
use support::scratch;

/// An hour of a solid on a 24 fps timeline: longer than any case waits.
fn project() -> Project {
    let generator = Generator::SolidColor {
        rgba: [30, 30, 220, 255],
    };
    let kind = TrackKind::for_generator(&generator).expect("a solid has a lane kind");
    let mut project = Project::new("interrupt", Rational::FPS_24);
    let track = project.add_track(kind, "Solid");
    project
        .add_generated(
            track,
            generator,
            TimeRange::at_rate(0, 24 * 3600, Rational::FPS_24),
        )
        .expect("place the solid");
    project
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list the scratch directory")
        .map(|entry| entry.expect("a directory entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Poll `done` every 10 ms until it holds or `limit` passes.
fn within(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

#[test]
fn a_signal_stops_the_export_and_leaves_no_staging_directory() {
    if Renderer::new_headless().is_err() {
        return;
    }
    for (signal, number) in [("HUP", 1), ("INT", 2), ("TERM", 15)] {
        let dir = scratch(&format!("interrupt-{signal}"));
        let path = dir.join("long.aria");
        project().save_to_file(&path).expect("save the project");
        let out = dir.join("long.mp4");
        let mut child = Command::new(env!("CARGO_BIN_EXE_aria"))
            .arg("export")
            .arg(&path)
            .arg(&out)
            .args(["--size", "160x90", "--quiet"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start aria export");
        let staged = within(Duration::from_secs(60), || {
            names(&dir)
                .iter()
                .any(|name| name.starts_with(".aria-export-"))
        });
        if !staged {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect aria export");
            panic!(
                "SIG{signal}: no staging directory appeared: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let sent = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(child.id().to_string())
            .status()
            .expect("run kill");
        assert!(sent.success(), "kill -{signal} failed");
        let stopped = within(Duration::from_secs(30), || {
            child.try_wait().expect("poll aria export").is_some()
        });
        if !stopped {
            let _ = child.kill();
        }
        let output = child.wait_with_output().expect("collect aria export");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stopped, "SIG{signal}: still running 30 s later: {stderr}");
        assert_eq!(
            output.status.code(),
            Some(128 + number),
            "SIG{signal}: {stderr}"
        );
        let line = format!(
            "aria: interrupted; nothing was written to {}",
            out.display()
        );
        assert!(
            stderr.lines().any(|text| text == line),
            "SIG{signal}: {stderr}"
        );
        assert_eq!(names(&dir), ["long.aria"], "SIG{signal}");
    }
}
