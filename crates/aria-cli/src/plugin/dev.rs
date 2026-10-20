//! `aria plugin dev` — build, stage, start the editor with `ARIA_PLUGIN_DEV`,
//! and rebuild whenever the crate sources change.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::thread;
use std::time::{Duration, SystemTime};

use aria_plugin::package::{MANIFEST_FILE, Package, PackageManifest};

use super::build::{self, BuildProfile};
use super::process;
use crate::args::Args;
use crate::fail::Failure;

pub(crate) const FLAGS: &[&str] = &["editor", "data-dir"];

pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(500);
pub(crate) const WATCHED_ROOTS: &[&str] = &["Cargo.toml", "src", "wit"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    hash: u64,
}

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let dir = args.required_path(1, "a plugin directory")?;
    if let Some(extra) = args.positional.get(2) {
        return Err(Failure::Usage(format!("unexpected argument \"{extra}\"")));
    }
    let editor_arg = args.value("editor")?.map(PathBuf::from);
    let data_dir_arg = args.value("data-dir")?.map(PathBuf::from);

    run_dev(&dir, editor_arg.as_deref(), data_dir_arg.as_deref())
}

pub(crate) fn run_dev(
    dir: &Path,
    editor_arg: Option<&Path>,
    data_dir_arg: Option<&Path>,
) -> Result<(), Failure> {
    let abs_dir = dir
        .canonicalize()
        .map_err(|err| Failure::Failed(format!("{}: {err}", dir.display())))?;
    if !abs_dir.is_dir() {
        return Err(Failure::Failed(format!(
            "{}: not a directory",
            abs_dir.display()
        )));
    }

    let manifest_path = abs_dir.join(MANIFEST_FILE);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|err| Failure::Failed(format!("{}: {err}", manifest_path.display())))?;
    PackageManifest::parse(&manifest_text)
        .map_err(|reason| Failure::Failed(format!("{}: {reason}", manifest_path.display())))?;

    let current_exe = std::env::current_exe().ok();
    let editor_path = resolve_editor_binary(editor_arg, current_exe.as_deref())?;

    let data_dir = match data_dir_arg {
        Some(raw) => {
            fs::create_dir_all(raw)
                .map_err(|err| Failure::Failed(format!("{}: {err}", raw.display())))?;
            Some(
                raw.canonicalize()
                    .map_err(|err| Failure::Failed(format!("{}: {err}", raw.display())))?,
            )
        }
        None => None,
    };

    let has_cargo = abs_dir.join("Cargo.toml").is_file();
    if has_cargo {
        let staged = build::build_and_stage(&abs_dir, BuildProfile::Debug)?;
        println!("staged       {}", staged.display());
    }
    let _package = Package::open(&abs_dir).map_err(|err| Failure::Failed(err.to_string()))?;

    let mut child = process::spawn_editor(&editor_path, &abs_dir, data_dir.as_deref())
        .map_err(Failure::Failed)?;

    let status = if has_cargo {
        let mut snapshot = snapshot_sources(&abs_dir);
        loop {
            thread::sleep(POLL_INTERVAL);
            if let Some(status) = child
                .try_wait()
                .map_err(|err| Failure::Failed(format!("{}: {err}", editor_path.display())))?
            {
                break status;
            }
            let current = snapshot_sources(&abs_dir);
            if current != snapshot {
                snapshot = current;
                match build::build_and_stage(&abs_dir, BuildProfile::Debug) {
                    Ok(staged) => println!("rebuilt      {}", staged.display()),
                    Err(err) => eprintln!("rebuild failed: {}", err.message()),
                }
            }
        }
    } else {
        child
            .wait()
            .map_err(|err| Failure::Failed(format!("{}: {err}", editor_path.display())))?
    };

    exit_for_editor_status(status)
}

/// Resolve the editor binary from `--editor` or `aria-desktop` beside `current_exe`.
pub(crate) fn resolve_editor_binary(
    editor_arg: Option<&Path>,
    current_exe: Option<&Path>,
) -> Result<PathBuf, Failure> {
    if let Some(explicit) = editor_arg {
        if explicit.is_file() {
            return Ok(explicit.to_path_buf());
        }
        return Err(Failure::Failed(format!(
            "{}: editor binary not found (specified via --editor)",
            explicit.display()
        )));
    }
    let exe_name = format!("aria-desktop{}", std::env::consts::EXE_SUFFIX);
    if let Some(sibling) = current_exe.and_then(|exe| exe.parent().map(|dir| dir.join(&exe_name))) {
        if sibling.is_file() {
            return Ok(sibling);
        }
        return Err(Failure::Failed(format!(
            "{}: editor binary not found; pass --editor <PATH>",
            sibling.display()
        )));
    }
    Err(Failure::Failed(
        "editor binary not found; pass --editor <PATH>".into(),
    ))
}

fn exit_for_editor_status(status: ExitStatus) -> Result<(), Failure> {
    if status.success() {
        return Ok(());
    }
    let code = status.code().unwrap_or(1);
    Err(Failure::Exit {
        code,
        message: format!("editor exited with status {code}"),
    })
}

pub(crate) fn snapshot_sources(dir: &Path) -> BTreeMap<PathBuf, FileStamp> {
    let mut stamps = BTreeMap::new();
    for entry in WATCHED_ROOTS {
        let path = dir.join(entry);
        collect_stamps(dir, &path, &mut stamps);
    }
    stamps
}

fn collect_stamps(root: &Path, path: &Path, stamps: &mut BTreeMap<PathBuf, FileStamp>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        children.sort();
        for child in children {
            collect_stamps(root, &child, stamps);
        }
    } else if meta.is_file() {
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let bytes = fs::read(path).unwrap_or_default();
        stamps.insert(
            rel,
            FileStamp {
                modified: meta.modified().ok(),
                len: meta.len(),
                hash: fnv1a64(&bytes),
            },
        );
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
