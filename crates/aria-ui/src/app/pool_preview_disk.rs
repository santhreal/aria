//! Pool poster frames that outlive the session.
//!
//! The class this closes: [`super::PoolPreviewStore`] holds pictures for one
//! session, so a relaunch re-decoded every tile the operator scrolled past —
//! one decode per source, through the same engine the preview needs, on the
//! first scroll of every launch. A poster frame is a picture of a *file*, and a
//! file does not change between launches, so it belongs on disk.
//!
//! What is persisted is exactly the resting picture: tick 0 at
//! [`super::THUMB_W`]×[`super::THUMB_H`]. A scrub frame is transient — it exists
//! while the pointer is on the tile and is worth nothing to the next launch —
//! and the source monitor's pane-sized frame is a thousand times a thumbnail.
//! One file per source per size keeps the directory bounded by the pool rather
//! than by how far anyone scrubbed.
//!
//! The key is [`content_key_for`](super::analysis::content_key_for), the same
//! key media analysis stores under: path, length and modification time, so a
//! re-encode or an edit in place reads as a different source rather than serving
//! a picture of the old one. The bytes live in the `library_thumbnails` cache,
//! which is disposable — clearing it costs one decode per tile, once.
//!
//! The container is versioned. A file whose magic is not exactly [`MAGIC`], or
//! whose length is not exactly its own header's, is ignored rather than parsed:
//! a stale layout must cost a decode, never a wrong picture.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use aria_analysis::moments::MediaContentKey;
use aria_core::RgbaImage;

/// Magic and version. The last byte is the container version: bump it when the
/// header or the pixel layout changes, and every older file stops being read.
const MAGIC: [u8; 8] = *b"ARIATHM\x01";

/// Magic, width, height. The pixels follow, tightly packed straight-alpha RGBA8.
const HEADER: usize = MAGIC.len() + 8;

/// Where one source's poster frame lives, at one size.
///
/// Flat under the cache root, the way `proxy.rs` names a proxy: one file per
/// source per size, named by content key, so nothing has to be listed to find
/// it and nothing accumulates per scrub position.
pub(super) fn poster_path(root: &Path, key: MediaContentKey, width: u32, height: u32) -> PathBuf {
    root.join(format!("{key}-{width}x{height}.athumb"))
}

/// The picture at `path`, or `None` when there is nothing usable there.
///
/// Every refusal is silent on purpose: a miss, a truncated write, a file from an
/// older container, and a directory that does not exist yet are all the same
/// answer to the caller — decode it.
pub(super) fn read(path: &Path) -> Option<RgbaImage> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; HEADER];
    file.read_exact(&mut header).ok()?;
    if header[..MAGIC.len()] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(header[8..12].try_into().ok()?);
    let height = u32::from_le_bytes(header[12..16].try_into().ok()?);
    let len = usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?;
    if len == 0 {
        return None;
    }
    let on_disk = file.metadata().ok()?.len();
    if on_disk != (HEADER + len) as u64 {
        // A half-written file, or one whose header disagrees with its body.
        return None;
    }
    let mut pixels = vec![0u8; len];
    file.read_exact(&mut pixels).ok()?;
    let image = RgbaImage::new(width, height, pixels);
    image.is_well_formed().then_some(image)
}

/// Write `image` to `path`, creating the cache directory on the way.
///
/// Written to a sibling and renamed, so a reader — this process on the next
/// launch, or a second copy of the editor sharing the cache — never sees a
/// partial file. The temporary name carries the process id for the same reason:
/// two writers must not share one scratch path.
pub(super) fn write(path: &Path, image: &RgbaImage) -> std::io::Result<()> {
    if !image.is_well_formed() || image.width == 0 || image.height == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a malformed picture is not written to the cache",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut part = path.as_os_str().to_os_string();
    part.push(format!(".{}.part", std::process::id()));
    let part = PathBuf::from(part);
    let mut file = std::fs::File::create(&part)?;
    let written = file
        .write_all(&MAGIC)
        .and_then(|()| file.write_all(&image.width.to_le_bytes()))
        .and_then(|()| file.write_all(&image.height.to_le_bytes()))
        .and_then(|()| file.write_all(&image.pixels));
    drop(file);
    if let Err(err) = written {
        let _ = std::fs::remove_file(&part);
        return Err(err);
    }
    std::fs::rename(&part, path)
}

#[cfg(test)]
#[path = "pool_preview_disk_tests.rs"]
mod tests;
