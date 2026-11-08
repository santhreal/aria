//! `aria plugin pack` — build, check, and write a `.ariaplugin` package archive.

use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::package::{ARCHIVE_EXTENSION, COMPONENT_FILE, LIBRARY_DIR, Package, pack};

use super::build::{self, BuildProfile};
use super::check;
use crate::args::{Args, reject_misnamed, reject_non_file};
use crate::fail::Failure;

pub(crate) const FLAGS: &[&str] = &["out", "no-build"];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let dir = args.required_path(1, "a plugin directory")?;
    args.reject_extra_positional(2)?;
    let no_build = args.flag("no-build")?;
    let out = args.value("out")?;
    if let Some(out) = out {
        // An archive is written over `--out` whatever its name, so
        // `--out <dir>/plugin.toml` wrote the archive over the manifest. The
        // kind of path is read before its name: `--out dist` naming a
        // directory was told to become `dist.ariaplugin`.
        reject_non_file(out)?;
        reject_misnamed("plugin pack", out, ARCHIVE_EXTENSION)?;
    }
    let out = out.map(PathBuf::from);

    pack_dir(&dir, out.as_deref(), no_build).map(drop)
}

/// Build (unless `no_build`), check, and pack `dir` into an `.ariaplugin` archive.
pub(crate) fn pack_dir(
    dir: &Path,
    out: Option<&Path>,
    no_build: bool,
) -> Result<(Package, PathBuf), Failure> {
    super::require_dir(dir, "a plugin directory")?;
    if dir.join("Cargo.toml").is_file() {
        if !no_build {
            build::build_and_stage(dir, BuildProfile::Release)?;
        } else if check::is_unbuilt_source_dir(dir) {
            return Err(Failure::Failed(format!(
                "{}: --no-build packs what is already built, and this source package has no {COMPONENT_FILE} or {LIBRARY_DIR}/; run `aria plugin pack {}` without --no-build",
                dir.display(),
                dir.display()
            )));
        }
    }

    let package = check::check_dir(dir)?;
    let archive_path = match out {
        Some(path) => path.to_path_buf(),
        None => dir.join(format!(
            "{}-{}.{ARCHIVE_EXTENSION}",
            package.manifest.id, package.manifest.version
        )),
    };
    if let Some(parent) = archive_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| Failure::io(&parent, &err))?;
    }
    let packed = pack(dir, &archive_path).map_err(|err| Failure::Failed(err.to_string()))?;
    println!("packed       {}", archive_path.display());
    Ok((packed, archive_path))
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
