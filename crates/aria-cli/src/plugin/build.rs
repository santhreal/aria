//! Building and staging a source plugin package (`Cargo.toml` -> `plugin.wasm` or `lib/<os>-<arch>.<ext>`).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use aria_plugin::package::{COMPONENT_FILE, LIBRARY_DIR, library_name};
use serde_json::Value;
use wit_component::ComponentEncoder;

use super::process;
use crate::fail::Failure;

/// Which Cargo profile to build with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildProfile {
    /// Debug profile (`aria plugin dev`).
    Debug,
    /// Release profile (`aria plugin pack`).
    Release,
}

/// Which runtime tier a source package compiles for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildTier {
    Wasm,
    Native,
}

/// Detect the build tier of a source plugin directory.
pub(crate) fn detect_build_tier(dir: &Path) -> Result<BuildTier, Failure> {
    let cargo_path = dir.join("Cargo.toml");
    let cargo_text = fs::read_to_string(&cargo_path)
        .map_err(|err| Failure::Failed(format!("{}: {err}", cargo_path.display())))?;

    if let Some(tier) = parse_metadata_tier(&cargo_text) {
        return Ok(tier);
    }
    if has_dependency(&cargo_text, "wit-bindgen") || dir.join("wit").is_dir() {
        return Ok(BuildTier::Wasm);
    }
    if has_dependency(&cargo_text, "aria-sdk") {
        return Ok(BuildTier::Native);
    }
    if dir.join(COMPONENT_FILE).is_file() {
        return Ok(BuildTier::Wasm);
    }
    if dir.join(LIBRARY_DIR).is_dir() {
        return Ok(BuildTier::Native);
    }
    Err(Failure::Failed(format!(
        "{}: cannot determine plugin tier from Cargo.toml (expected wit-bindgen or aria-sdk dependency, or [package.metadata.aria] tier = \"wasm\"|\"native\")",
        cargo_path.display()
    )))
}

fn parse_metadata_tier(cargo_text: &str) -> Option<BuildTier> {
    let mut in_aria_metadata = false;
    for raw_line in cargo_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_aria_metadata = header.trim() == "package.metadata.aria";
            continue;
        }
        if in_aria_metadata
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "tier"
        {
            let val = value.trim().trim_matches('"').trim_matches('\'');
            match val {
                "wasm" => return Some(BuildTier::Wasm),
                "native" => return Some(BuildTier::Native),
                _ => {}
            }
        }
    }
    None
}

fn has_dependency(cargo_text: &str, dep_name: &str) -> bool {
    for raw_line in cargo_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if let Some((key, _)) = line.split_once('=')
            && key.trim() == dep_name
        {
            return true;
        }
    }
    false
}

/// Build the crate at `<dir>/Cargo.toml` and stage its artifact into `<dir>`.
///
/// Returns the staged file path (`<dir>/plugin.wasm` or `<dir>/lib/<os>-<arch>.<ext>`).
pub(crate) fn build_and_stage(dir: &Path, profile: BuildProfile) -> Result<PathBuf, Failure> {
    let manifest_path = dir.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(Failure::Failed(format!(
            "{}: no Cargo.toml found",
            dir.display()
        )));
    }
    let tier = detect_build_tier(dir)?;
    let release = profile == BuildProfile::Release;
    let target = match tier {
        BuildTier::Wasm => Some("wasm32-unknown-unknown"),
        BuildTier::Native => None,
    };

    let output = process::cargo_build(&manifest_path, release, target).map_err(Failure::Failed)?;
    if !output.success {
        let detail = first_error_line(&output.stderr).unwrap_or_else(|| {
            format!(
                "cargo build exited with status {}",
                output.code.unwrap_or(1)
            )
        });
        return Err(Failure::Failed(format!(
            "{}: {detail}",
            manifest_path.display()
        )));
    }

    let artifact = find_cdylib_artifact(&output.stdout, &manifest_path, tier)?;
    stage_artifact(dir, &artifact, tier)
}

fn first_error_line(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("error"))
        .map(str::to_owned)
}

/// Locate the compiled `cdylib` output path from `cargo build --message-format=json-render-diagnostics`.
pub(crate) fn find_cdylib_artifact(
    json_stdout: &str,
    manifest_path: &Path,
    tier: BuildTier,
) -> Result<PathBuf, Failure> {
    let canonical_manifest = manifest_path.canonicalize().ok();
    let expected_ext = match tier {
        BuildTier::Wasm => "wasm",
        BuildTier::Native => std::env::consts::DLL_EXTENSION,
    };

    let mut matched_for_manifest: Option<PathBuf> = None;
    let mut last_cdylib: Option<PathBuf> = None;

    for line in json_stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if msg.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let Some(target) = msg.get("target") else {
            continue;
        };
        let is_cdylib = ["kind", "crate_types"].iter().any(|field| {
            target
                .get(*field)
                .and_then(Value::as_array)
                .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("cdylib")))
        });
        if !is_cdylib {
            continue;
        }
        let filenames: Vec<&str> = msg
            .get("filenames")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let Some(chosen) = pick_artifact_filename(&filenames, expected_ext) else {
            continue;
        };
        let is_this_manifest = match (
            &canonical_manifest,
            msg.get("manifest_path").and_then(Value::as_str),
        ) {
            (Some(want), Some(got)) => Path::new(got)
                .canonicalize()
                .ok()
                .is_some_and(|got_canon| &got_canon == want),
            _ => false,
        };
        if is_this_manifest {
            matched_for_manifest = Some(chosen.clone());
        }
        last_cdylib = Some(chosen);
    }
    matched_for_manifest.or(last_cdylib).ok_or_else(|| {
        Failure::Failed(format!(
            "{}: cargo build produced no cdylib artifact",
            manifest_path.display()
        ))
    })
}

fn pick_artifact_filename(filenames: &[&str], expected_ext: &str) -> Option<PathBuf> {
    if let Some(found) = filenames.iter().find(|name| {
        Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(expected_ext))
    }) {
        return Some(PathBuf::from(*found));
    }
    filenames.first().copied().map(PathBuf::from)
}

/// Stage the compiled artifact into `dir` using a temporary file and atomic rename.
pub(crate) fn stage_artifact(
    dir: &Path,
    artifact: &Path,
    tier: BuildTier,
) -> Result<PathBuf, Failure> {
    match tier {
        BuildTier::Native => {
            let lib_dir = dir.join(LIBRARY_DIR);
            fs::create_dir_all(&lib_dir)
                .map_err(|err| Failure::Failed(format!("{}: {err}", lib_dir.display())))?;
            let file_name = library_name();
            let dest = lib_dir.join(&file_name);
            let tmp = lib_dir.join(format!(".{file_name}.tmp.{}", std::process::id()));
            let copied = fs::copy(artifact, &tmp).and_then(|_| fs::rename(&tmp, &dest));
            if let Err(err) = copied {
                let _ = fs::remove_file(&tmp);
                return Err(Failure::Failed(format!(
                    "{} -> {}: {err}",
                    artifact.display(),
                    dest.display()
                )));
            }
            Ok(dest)
        }
        BuildTier::Wasm => {
            let core_bytes = fs::read(artifact)
                .map_err(|err| Failure::Failed(format!("{}: {err}", artifact.display())))?;
            let component_bytes = ComponentEncoder::default()
                .module(&core_bytes)
                .and_then(|encoder| encoder.validate(true).encode())
                .map_err(|err| {
                    Failure::Failed(format!(
                        "{}: failed to encode WebAssembly component: {err:#}",
                        artifact.display()
                    ))
                })?;
            let dest = dir.join(COMPONENT_FILE);
            let tmp = dir.join(format!(".{COMPONENT_FILE}.tmp.{}", std::process::id()));
            let written =
                write_and_sync(&tmp, &component_bytes).and_then(|()| fs::rename(&tmp, &dest));
            if let Err(err) = written {
                let _ = fs::remove_file(&tmp);
                return Err(Failure::Failed(format!("{}: {err}", dest.display())));
            }
            Ok(dest)
        }
    }
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
