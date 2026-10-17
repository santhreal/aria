//! Host, GPU, driver, git commit, and process resource probes.

use std::path::Path;

use crate::{EnvironmentInfo, MachineProfile};

/// Probe the current machine, GPU, driver, git commit, and build profile.
pub fn probe_environment(
    workspace_root: &Path,
    override_machine: Option<MachineProfile>,
) -> EnvironmentInfo {
    let hostname = read_trimmed("/etc/hostname").unwrap_or_else(|| "unknown-host".to_string());
    let cpu = probe_cpu_model().unwrap_or_else(|| "unknown-cpu".to_string());
    let os = probe_os().unwrap_or_else(|| std::env::consts::OS.to_string());
    let (gpu, driver, wgpu_adapter) = probe_gpu_and_driver();
    let commit = probe_git_commit(workspace_root).unwrap_or_else(|| "unknown".to_string());
    let profile = if cfg!(debug_assertions) {
        "debug".to_string()
    } else {
        "release".to_string()
    };
    let machine = override_machine.unwrap_or_else(|| detect_machine_profile(&cpu, &gpu));

    EnvironmentInfo {
        machine,
        hostname,
        os,
        cpu,
        gpu,
        driver,
        wgpu_adapter,
        commit,
        profile,
    }
}

fn detect_machine_profile(cpu: &str, gpu: &str) -> MachineProfile {
    if gpu.contains("5090") || cpu.contains("9950X") {
        MachineProfile::Desktop
    } else if gpu.contains("3080 Ti") || gpu.contains("3080") || gpu.contains("RTX") {
        MachineProfile::Server
    } else {
        MachineProfile::Laptop
    }
}

fn read_trimmed(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn probe_cpu_model() -> Option<String> {
    let content = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in content.lines() {
        if let Some((key, value)) = line.split_once(':')
            && key.trim() == "model name"
        {
            let model = value.trim();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
    }
    None
}

fn probe_os() -> Option<String> {
    crate::process::probe_stdout("uname", &["-srm"], None)
}

fn probe_gpu_and_driver() -> (String, String, String) {
    let wgpu_info = aria_compositor::GpuContext::new_headless_blocking()
        .ok()
        .map(|ctx| ctx.adapter.get_info());
    let wgpu_adapter = wgpu_info
        .as_ref()
        .map(|info| format!("{} ({:?})", info.name, info.backend))
        .unwrap_or_else(|| "none".to_string());

    if let Some((nv_gpu, nv_driver)) = probe_nvidia_smi() {
        return (nv_gpu, nv_driver, wgpu_adapter);
    }

    if let Some(info) = wgpu_info {
        let driver = match (info.driver.is_empty(), info.driver_info.is_empty()) {
            (false, false) => format!("{} {}", info.driver, info.driver_info),
            (false, true) => info.driver,
            (true, false) => info.driver_info,
            (true, true) => "unknown-driver".to_string(),
        };
        let gpu_name = if info.name.contains("llvmpipe") {
            probe_lspci_gpu()
                .map(|pci| format!("{pci} (wgpu: {})", info.name))
                .unwrap_or(info.name)
        } else {
            info.name
        };
        return (gpu_name, driver, wgpu_adapter);
    }

    (
        "unknown-gpu".to_string(),
        "unknown-driver".to_string(),
        wgpu_adapter,
    )
}

fn probe_nvidia_smi() -> Option<(String, String)> {
    let text = crate::process::probe_stdout(
        "nvidia-smi",
        &["--query-gpu=name,driver_version", "--format=csv,noheader"],
        None,
    )?;
    let first = text.lines().next()?.trim();
    let (name, driver) = first.split_once(',')?;
    let name = name.trim();
    let driver = driver.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), format!("NVIDIA {driver}")))
}

fn probe_lspci_gpu() -> Option<String> {
    let text = crate::process::probe_stdout("lspci", &[], None)?;
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("VGA compatible controller:") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn probe_git_commit(workspace_root: &Path) -> Option<String> {
    crate::process::probe_stdout("git", &["rev-parse", "HEAD"], Some(workspace_root))
}

/// Read the current process resident set size (`VmRSS`) in megabytes (MiB).
pub fn current_rss_mb() -> Result<f64, String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|err| format!("failed to read /proc/self/status: {err}"))?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb_str = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| "VmRSS line has no numeric value".to_string())?;
            let kb: u64 = kb_str
                .parse()
                .map_err(|err| format!("invalid VmRSS value '{kb_str}': {err}"))?;
            return Ok(kb as f64 / 1024.0);
        }
    }
    Err("VmRSS entry missing from /proc/self/status".to_string())
}

/// Read the current process thread count from `/proc/self/status` (`Threads:`).
pub fn current_thread_count() -> Result<u64, String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|err| format!("failed to read /proc/self/status: {err}"))?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Threads:") {
            let count_str = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| "Threads line has no numeric value".to_string())?;
            let count: u64 = count_str
                .parse()
                .map_err(|err| format!("invalid Threads value '{count_str}': {err}"))?;
            return Ok(count);
        }
    }
    Err("Threads entry missing from /proc/self/status".to_string())
}
