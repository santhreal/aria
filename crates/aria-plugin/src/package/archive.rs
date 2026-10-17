//! `.ariaplugin` archives: a package directory in a zip file.
//!
//! An archive holds the package's files at its root, or under one top folder.
//! Unpacking writes only regular files and folders, at relative paths with no
//! `.` or `..` part, within the limits below; anything else refuses the whole
//! archive and leaves nothing behind.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{MANIFEST_FILE, Package, PackageError};

/// The file extension of a package archive.
pub const ARCHIVE_EXTENSION: &str = "ariaplugin";

/// Most entries an archive may hold.
pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;

/// Most bytes an archive may unpack to.
pub const MAX_UNPACKED_BYTES: u64 = 1 << 30;

/// Whether `path` names a package archive by its extension.
pub fn is_archive(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
}

/// Top-level entries of a package directory that also holds the plugin's Rust
/// source, marked by a `Cargo.toml` at its root. They build the code and are
/// not part of the package.
pub const SOURCE_ENTRIES: [&str; 5] = ["Cargo.toml", "Cargo.lock", "src", "wit", "target"];

/// Write the package in `dir` to the archive `out`.
///
/// The package is opened first, so an archive holds only a package a host
/// opens. Hidden files and folders, `.ariaplugin` archives, and, when `dir`
/// holds a `Cargo.toml`, the [`SOURCE_ENTRIES`] are left out; a symbolic link
/// refuses the package. The archive is written beside `out` and renamed over
/// it once complete.
pub fn pack(dir: &Path, out: &Path) -> Result<Package, PackageError> {
    let package = Package::open(dir)?;
    let source = dir.join("Cargo.toml").is_file();
    let mut files = Vec::new();
    collect(dir, Path::new(""), source, &mut files)?;
    let mut partial = out.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let written = write_zip(dir, &files, &partial).and_then(|()| fs::rename(&partial, out));
    if let Err(err) = written {
        let _ = fs::remove_file(&partial);
        return Err(PackageError::new(out, err.to_string()));
    }
    Ok(package)
}

/// Every regular file under `dir` that belongs in the archive, relative to the
/// package root, sorted. `source` leaves out the top-level [`SOURCE_ENTRIES`].
fn collect(
    root: &Path,
    dir: &Path,
    source: bool,
    files: &mut Vec<PathBuf>,
) -> Result<(), PackageError> {
    let here = root.join(dir);
    let entries = fs::read_dir(&here).map_err(|err| PackageError::new(&here, err.to_string()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| PackageError::new(&here, err.to_string()))?;
        names.push(entry.file_name());
    }
    names.sort();
    let top = dir.as_os_str().is_empty();
    for name in names {
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if source && top && SOURCE_ENTRIES.iter().any(|entry| name == *entry) {
            continue;
        }
        let relative = dir.join(&name);
        let path = root.join(&relative);
        let kind = fs::symlink_metadata(&path)
            .map_err(|err| PackageError::new(&path, err.to_string()))?
            .file_type();
        if kind.is_symlink() {
            return Err(PackageError::new(
                &path,
                "a package archive holds no symbolic links; copy the target in",
            ));
        }
        if kind.is_dir() {
            collect(root, &relative, source, files)?;
        } else if kind.is_file() && !is_archive(&relative) {
            files.push(relative);
        }
    }
    Ok(())
}

fn write_zip(root: &Path, files: &[PathBuf], out: &Path) -> io::Result<()> {
    let mut zip = ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for relative in files {
        let name: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        zip.start_file(name.join("/"), options)?;
        io::copy(&mut File::open(root.join(relative))?, &mut zip)?;
    }
    zip.finish()?.sync_all()
}

/// Unpack the archive at `archive` into `into`, which must not exist, and
/// open the package it holds. On any refusal `into` is removed.
pub fn unpack(archive: &Path, into: &Path) -> Result<Package, PackageError> {
    unpack_within(archive, into, MAX_UNPACKED_BYTES)
}

fn unpack_within(archive: &Path, into: &Path, max_bytes: u64) -> Result<Package, PackageError> {
    if into.exists() {
        return Err(PackageError::new(into, "the unpack folder already exists"));
    }
    let unpacked =
        extract(archive, into, max_bytes).map_err(|reason| PackageError::new(archive, reason));
    let opened = unpacked.and_then(|()| Package::open(into));
    if opened.is_err() {
        let _ = fs::remove_dir_all(into);
    }
    opened
}

fn extract(archive: &Path, into: &Path, max_bytes: u64) -> Result<(), String> {
    let file = File::open(archive).map_err(|err| err.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|err| format!("not a zip archive: {err}"))?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "holds {} entries; an archive holds at most {MAX_ARCHIVE_ENTRIES}",
            zip.len()
        ));
    }
    let mut names = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|err| err.to_string())?;
        let name = entry.name().map_err(|err| err.to_string())?.into_owned();
        if entry.is_symlink() {
            return Err(format!("{name}: an archive holds no symbolic links"));
        }
        let parts = entry_parts(&name)?;
        names.push((parts, entry.is_dir()));
    }
    let prefix = root_prefix(&names)?;
    fs::create_dir_all(into).map_err(|err| err.to_string())?;
    let mut budget = max_bytes;
    for (index, (parts, is_dir)) in names.iter().enumerate() {
        let Some(inner) = parts.get(prefix..) else {
            continue;
        };
        if inner.is_empty() {
            continue;
        }
        let path = inner
            .iter()
            .fold(into.to_path_buf(), |path, part| path.join(part));
        if *is_dir {
            fs::create_dir_all(&path).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut entry = zip.by_index(index).map_err(|err| err.to_string())?;
        let mut out =
            File::create_new(&path).map_err(|err| format!("{}: {err}", inner.join("/")))?;
        // The sizes in the archive are the archive's own claim: count the
        // bytes as they inflate.
        let copied = io::copy(&mut (&mut entry).take(budget + 1), &mut out)
            .map_err(|err| format!("{}: {err}", inner.join("/")))?;
        if copied > budget {
            return Err(format!("unpacks to more than {max_bytes} bytes"));
        }
        budget -= copied;
    }
    Ok(())
}

/// The parts of an entry name, refusing any that would leave the folder.
fn entry_parts(name: &str) -> Result<Vec<String>, String> {
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    let parts: Vec<&str> = trimmed.split('/').collect();
    let safe = !trimmed.is_empty()
        && parts.iter().all(|part| {
            !part.is_empty() && *part != "." && *part != ".." && !part.contains(['\\', ':', '\0'])
        });
    if !safe {
        return Err(format!(
            "{name:?} is not a relative path inside the package"
        ));
    }
    Ok(parts.into_iter().map(str::to_owned).collect())
}

/// How many leading parts every entry shares before the package root: 0 when
/// `plugin.toml` is at the root, 1 when it is in the one top folder.
fn root_prefix(names: &[(Vec<String>, bool)]) -> Result<usize, String> {
    let is_manifest = |parts: &[String]| parts.len() == 1 && parts[0] == MANIFEST_FILE;
    if names.iter().any(|(parts, dir)| !dir && is_manifest(parts)) {
        return Ok(0);
    }
    let top = names.first().map(|(parts, _)| parts[0].as_str());
    let one_folder = top.is_some_and(|top| {
        names.iter().all(|(parts, _)| parts[0] == top)
            && names
                .iter()
                .any(|(parts, dir)| !dir && parts.len() == 2 && parts[1] == MANIFEST_FILE)
    });
    if one_folder {
        Ok(1)
    } else {
        Err(format!(
            "holds no {MANIFEST_FILE} at its root or in one top folder"
        ))
    }
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
