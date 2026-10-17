//! Subprocess helpers for `aria-bench`: host probes, `ffmpeg` fixture synthesis,
//! isolated child measurement, and private headless `Xvfb` window launch.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Run a host probe binary and return its trimmed stdout when it exits 0.
pub(crate) fn probe_stdout(
    program: &str,
    args: &[&str],
    current_dir: Option<&Path>,
) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Run `ffmpeg` with `args` for local fixture generation.
pub(crate) fn run_ffmpeg(args: &[&str], label: &str) -> Result<(), String> {
    let out = Command::new("ffmpeg")
        .args(args)
        .output()
        .map_err(|err| format!("failed to invoke ffmpeg for {label}: {err}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "ffmpeg failed for {label} (exit {:?}): {stderr}",
            out.status.code()
        ));
    }
    Ok(())
}

/// Check whether a GStreamer element is installed via `gst-inspect-1.0`.
pub(crate) fn has_gst_element(elem: &str) -> bool {
    Command::new("gst-inspect-1.0")
        .arg(elem)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `aria-bench memory` in an isolated child process so earlier scenarios
/// in `aria-bench all` do not inflate `/proc/self/status` `VmRSS`.
pub(crate) fn run_isolated_memory_child(
    exe: &Path,
    machine: &str,
    fixtures_dir: &Path,
    results_dir: &Path,
) -> bool {
    Command::new(exe)
        .env("ARIA_BENCH_MEMORY_ISOLATED", "1")
        .args([
            "memory",
            "--machine",
            machine,
            "--fixtures-dir",
            &fixtures_dir.to_string_lossy(),
            "--results-dir",
            &results_dir.to_string_lossy(),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Guard for a private headless `Xvfb` server started on an unused display `:91..=:99`.
#[derive(Debug)]
pub(crate) struct XvfbGuard {
    child: Child,
    pub(crate) display: String,
}

impl Drop for XvfbGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_free_display() -> Option<u32> {
    for num in 91..=99 {
        let display = format!(":{num}");
        let status = Command::new("xdpyinfo")
            .args(["-display", &display])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let is_free = match status {
            Ok(s) => !s.success(),
            Err(_) => {
                let lock = PathBuf::from(format!("/tmp/.X{num}-lock"));
                !lock.exists()
            }
        };
        if is_free {
            return Some(num);
        }
    }
    None
}

/// Start a private headless `Xvfb` server on a verified free display `:91..=:99`.
pub(crate) fn start_private_xvfb() -> Option<XvfbGuard> {
    let num = find_free_display()?;
    let display = format!(":{num}");
    let mut child = Command::new("Xvfb")
        .args([
            &display,
            "-screen",
            "0",
            "1600x1000x24",
            "-nolisten",
            "tcp",
            "-ac",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return None;
        }
        let ready = Command::new("xdpyinfo")
            .args(["-display", &display])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ready {
            return Some(XvfbGuard { child, display });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

/// Spawn `aria-desktop` against a private `Xvfb` display and measure milliseconds
/// until its top-level X11 window is mapped and visible.
pub(crate) fn measure_process_window_launch(
    bin: &Path,
    display: &str,
    data_dir: &Path,
    cache_dir: &Path,
) -> Result<f64, String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|err| format!("failed to create data_dir {}: {err}", data_dir.display()))?;
    std::fs::create_dir_all(cache_dir)
        .map_err(|err| format!("failed to create cache_dir {}: {err}", cache_dir.display()))?;

    let started = Instant::now();
    let mut cmd = Command::new(bin);
    cmd.env("DISPLAY", display)
        .env("WAYLAND_DISPLAY", "")
        .env("XDG_SESSION_TYPE", "x11")
        .env("ARIA_DATA_DIR", data_dir)
        .env("XDG_DATA_HOME", data_dir)
        .env("XDG_CONFIG_HOME", data_dir)
        .env("XDG_CACHE_HOME", cache_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if std::env::var_os("VK_ICD_FILENAMES").is_none()
        && Path::new("/usr/share/vulkan/icd.d/lvp_icd.json").exists()
    {
        cmd.env("VK_ICD_FILENAMES", "/usr/share/vulkan/icd.d/lvp_icd.json");
    }
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("failed to spawn {}: {err}", bin.display()))?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut mapped_ms = None;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "aria-desktop exited before mapping window: {status:?}"
            ));
        }
        let out = Command::new("xdotool")
            .env("DISPLAY", display)
            .args(["search", "--onlyvisible", ".*"])
            .output();
        if let Ok(out) = out
            && out.status.success()
            && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
        {
            mapped_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let _ = child.kill();
    let _ = child.wait();

    mapped_ms.ok_or_else(|| "timed out waiting for aria-desktop X11 window to map".to_string())
}
