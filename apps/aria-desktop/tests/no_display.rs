//! A start with no display server to reach ends with the reason and the fix
//! on stderr and exit status 1, for each way the environment leaves the
//! editor without a window: no display variable set, `ZED_HEADLESS` set, an X
//! server that is not running, and a Wayland compositor that is not running.
//! The first two ran the editor headless, with no window and no end; the
//! last two panicked in the platform layer.
//!
//! Each case runs the built binary on a fresh profile under the cargo target
//! directory.
//!
//! Not covered: a display server that accepts the connection and then fails
//! the window, which needs a running server; macOS and Windows.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// An X display number with no server. The test checks that its socket is
/// absent before it runs.
const ABSENT_X: &str = ":9973";
const ABSENT_X_SOCKET: &str = "/tmp/.X11-unix/X9973";

/// How long a start may take to end. It mounts the plugins and builds the
/// session before it reaches for the display; a start that runs headless
/// never ends.
const DEADLINE: Duration = Duration::from_secs(120);

/// One environment without a display server: the variables set over an
/// environment with every display variable removed, and the reason stderr
/// states.
struct Case {
    name: &'static str,
    env: &'static [(&'static str, &'static str)],
    reason: &'static str,
}

const CASES: &[Case] = &[
    Case {
        name: "no-display",
        env: &[],
        reason: "neither WAYLAND_DISPLAY nor DISPLAY is set",
    },
    Case {
        name: "zed-headless",
        env: &[("ZED_HEADLESS", "1"), ("DISPLAY", ABSENT_X)],
        reason: "ZED_HEADLESS is set, so no window can open",
    },
    Case {
        name: "x-absent",
        env: &[("DISPLAY", ABSENT_X)],
        reason: "cannot connect to the X server at DISPLAY=:9973",
    },
    Case {
        name: "wayland-absent",
        env: &[
            ("WAYLAND_DISPLAY", "aria-absent-wayland"),
            ("XDG_RUNTIME_DIR", "/nonexistent-aria-runtime-dir"),
        ],
        reason: "cannot connect to the Wayland compositor at WAYLAND_DISPLAY=aria-absent-wayland",
    },
];

/// The first line every case prints, and the fix that follows it.
const HEADLINE: &str = "aria cannot open its window: ";
const FIX: &str = "WAYLAND_DISPLAY or DISPLAY set to a display server that is running";

/// A fresh profile directory for `case`, under the cargo target directory.
fn profile(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("no-display-{case}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear the last run's profile");
    }
    std::fs::create_dir_all(&dir).expect("a profile directory");
    dir
}

/// **Every start without a display server exits 1 with the reason and the
/// fix, and none panics or runs on.**
#[test]
fn a_start_without_a_display_server_says_why_and_exits() {
    assert!(
        !Path::new(ABSENT_X_SOCKET).exists(),
        "{ABSENT_X_SOCKET} exists, so {ABSENT_X} has a server; pick a free display number"
    );
    let mut wrong = Vec::new();
    for case in CASES {
        let dir = profile(case.name);
        let stderr_path = dir.join("stderr.txt");
        let stderr_file = std::fs::File::create(&stderr_path).expect("a stderr file");
        let mut child = Command::new(env!("CARGO_BIN_EXE_aria-desktop"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("ZED_HEADLESS")
            .env("ARIA_DATA_DIR", &dir)
            .env("RUST_LOG", "off")
            .envs(case.env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr_file)
            .spawn()
            .expect("the aria-desktop binary runs");
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("the editor can be waited on") {
                break Some(status);
            }
            if started.elapsed() > DEADLINE {
                child.kill().ok();
                child.wait().ok();
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        let expected = format!("{HEADLINE}{}", case.reason);
        match status {
            None => wrong.push(format!("{}: still running after {DEADLINE:?}", case.name)),
            Some(status) if status.code() != Some(1) => wrong.push(format!(
                "{}: exited with {status}, expected exit status 1; stderr: {}",
                case.name,
                stderr.trim()
            )),
            Some(_) if !stderr.contains(&expected) || !stderr.contains(FIX) => wrong.push(format!(
                "{}: stderr lacks {expected:?} or {FIX:?}: {}",
                case.name,
                stderr.trim()
            )),
            Some(_) => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
