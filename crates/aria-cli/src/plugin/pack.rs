//! `aria plugin pack` — build, check, and write a `.ariaplugin` package archive.

use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::package::{ARCHIVE_EXTENSION, Package, pack};

use super::build::{self, BuildProfile};
use super::check;
use crate::args::Args;
use crate::fail::Failure;

pub(crate) const FLAGS: &[&str] = &["out", "no-build"];

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let dir = args.required_path(1, "a plugin directory")?;
    args.reject_extra_positional(2)?;
    let no_build = args.flag("no-build")?;
    let out = args.value("out")?.map(PathBuf::from);

    pack_dir(&dir, out.as_deref(), no_build).map(drop)
}

/// Build (unless `no_build`), check, and pack `dir` into an `.ariaplugin` archive.
pub(crate) fn pack_dir(
    dir: &Path,
    out: Option<&Path>,
    no_build: bool,
) -> Result<(Package, PathBuf), Failure> {
    if !dir.is_dir() {
        return Err(Failure::Failed(format!(
            "{}: not a directory",
            dir.display()
        )));
    }
    if !no_build && dir.join("Cargo.toml").is_file() {
        build::build_and_stage(dir, BuildProfile::Release)?;
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
        fs::create_dir_all(parent)
            .map_err(|err| Failure::Failed(format!("{}: {err}", parent.display())))?;
    }
    let packed = pack(dir, &archive_path).map_err(|err| Failure::Failed(err.to_string()))?;
    println!("packed       {}", archive_path.display());
    Ok((packed, archive_path))
}
