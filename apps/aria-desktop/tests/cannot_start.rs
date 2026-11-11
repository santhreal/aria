//! A start that cannot run ends with the reason and the fix on stderr and exit
//! status 1.
//!
//! No display server to reach: no display variable set, `ZED_HEADLESS` set,
//! an X server that is not running, and a Wayland compositor that is not
//! running. The first two ran the editor headless, with no window and no end;
//! the last two panicked in the platform layer.
//!
//! A profile directory the editor cannot write: `ARIA_DATA_DIR` naming a file,
//! or a path under one, started the editor with the settings plugin unmounted,
//! and a directory without write permission started it with nothing logged.
//!
//! Each case runs the built binary on a fresh profile under the cargo target
//! directory.
//!
//! Not covered: a display server that accepts the connection and then fails
//! the window, which needs a running server; a read-only directory when the
//! suite runs as root, which writes anyway; macOS and Windows.

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

/// What a start prints first when the profile directory is unusable.
const PROFILE_HEADLINE: &str = "aria cannot start: the profile directory ";
const PROFILE_FIX: &str = "Set ARIA_DATA_DIR to a directory this account can write.";

/// Run the editor with no display on `data_dir` and return its exit status
/// (`None` past [`DEADLINE`]) and stderr.
fn start_on(data_dir: &Path, stderr_path: &Path) -> (Option<std::process::ExitStatus>, String) {
    let stderr_file = std::fs::File::create(stderr_path).expect("a stderr file");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aria-desktop"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("ZED_HEADLESS")
        .env("ARIA_DATA_DIR", data_dir)
        .env("RUST_LOG", "off")
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
        std::thread::sleep(Duration::from_millis(20));
    };
    (
        status,
        std::fs::read_to_string(stderr_path).unwrap_or_default(),
    )
}

/// **A profile directory the editor cannot write stops the start with the
/// directory, the reason and the fix, before any plugin mounts; one it can make
/// is made and the start goes on.**
///
/// The start has no display, so a profile that passes reaches the display
/// refusal: that refusal in place of the profile one is the check being skipped.
#[test]
fn a_start_on_a_profile_it_cannot_write_says_why_and_exits() {
    let root = profile("unusable-profile");
    let file = root.join("a-file");
    std::fs::write(&file, b"not a directory").expect("a plain file");
    let locked = root.join("locked");
    std::fs::create_dir(&locked).expect("a directory to lock");
    let mut perms = std::fs::metadata(&locked)
        .expect("its metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o555);
    std::fs::set_permissions(&locked, perms).expect("lock the directory");
    // Root writes into a 0555 directory, so that case only stands for an
    // account the mode binds.
    let locked_binds = std::fs::write(locked.join("probe"), b"").is_err();
    let mut unusable = vec![file.join("profile"), file];
    if locked_binds {
        unusable.push(locked.clone());
    }
    let mut wrong = Vec::new();
    for (n, dir) in unusable.iter().enumerate() {
        let (status, stderr) = start_on(dir, &root.join(format!("stderr-{n}.txt")));
        let headline = format!("{PROFILE_HEADLINE}{} is not usable: ", dir.display());
        match status {
            Some(status) if status.code() == Some(1) => {}
            other => wrong.push(format!(
                "{}: ended {other:?}, expected exit status 1",
                dir.display()
            )),
        }
        if !stderr.starts_with(&headline) || !stderr.contains(PROFILE_FIX) {
            wrong.push(format!(
                "{}: stderr lacks {headline:?} first or {PROFILE_FIX:?}: {}",
                dir.display(),
                stderr.trim()
            ));
        }
        if stderr.contains(HEADLINE) {
            wrong.push(format!(
                "{}: reached the display before refusing the profile",
                dir.display()
            ));
        }
    }
    let fresh = root.join("made").join("on-start");
    let (status, stderr) = start_on(&fresh, &root.join("stderr-fresh.txt"));
    if status.and_then(|s| s.code()) != Some(1)
        || !stderr.contains(HEADLINE)
        || stderr.contains(PROFILE_HEADLINE)
    {
        wrong.push(format!(
            "a profile it can make: ended {status:?}, expected the display refusal: {}",
            stderr.trim()
        ));
    }
    if !fresh.is_dir()
        || std::fs::read_dir(&fresh)
            .expect("the made profile")
            .any(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".write-probe-")
            })
    {
        wrong.push(format!(
            "{}: not made, or the write probe was left in it",
            fresh.display()
        ));
    }
    let mut perms = std::fs::metadata(&locked)
        .expect("its metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&locked, perms).expect("unlock the directory");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
