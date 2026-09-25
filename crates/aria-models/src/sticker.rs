//! Sticker catalog: the assets a [`Generator::Sticker`] may reference.
//!
//! The set is data (`data/stickers.toml`, replaceable under
//! [`crate::catalog::root`]) and the art is bytes. Each entry names a file; the
//! bytes come from `<catalog root>/stickers/<file>` when that file exists, and
//! from the copy bundled into the binary otherwise. So a new sticker lands by
//! dropping art and a catalog file next to it, and a build with nothing
//! installed still has the starter pack.
//!
//! An entry whose art resolves to neither is dropped rather than offered: a tile
//! that cannot draw is worse than a missing one. The reason is reported through
//! [`crate::catalog::problems`], the same place a refused catalog file appears.
//!
//! [`Generator::Sticker`]: crate::Generator::Sticker

use std::sync::LazyLock;

/// One sticker as the catalog file declares it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct StickerEntry {
    id: String,
    label: String,
    width: u32,
    height: u32,
    #[serde(default)]
    animated: bool,
    /// Art file name, resolved under `<catalog root>/stickers/` or against the
    /// bundled pack.
    file: String,
}

/// One sticker: a stable id (what [`crate::Generator::Sticker`] stores), a
/// display label, the intrinsic pixel size of the art, whether it animates, and
/// the encoded bytes (PNG, APNG, or GIF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickerSpec {
    pub id: String,
    pub label: String,
    /// Intrinsic width of the encoded asset, in pixels.
    pub width: u32,
    /// Intrinsic height of the encoded asset, in pixels.
    pub height: u32,
    /// Whether the asset has more than one frame (APNG / animated GIF).
    pub animated: bool,
    /// The encoded asset: bundled bytes, or the file that replaced them.
    pub bytes: Vec<u8>,
    /// Where the bytes came from, for a host that shows the catalog's source.
    pub source: ArtSource,
}

/// Where one sticker's art was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtSource {
    /// The copy compiled into this build.
    Bundled,
    /// A file under `<catalog root>/stickers/`.
    Installed,
}

macro_rules! bundled_art {
    ($($file:literal),* $(,)?) => {
        &[$(($file, include_bytes!(concat!("../../../assets/stickers/", $file)) as &[u8])),*]
    };
}

/// The art that ships inside the binary, by file name. Placeholder art generated
/// in-repo; swapping in real artwork only touches `assets/stickers/`.
const BUNDLED: &[(&str, &[u8])] = bundled_art![
    "heart.png",
    "star.png",
    "smiley.png",
    "bolt.png",
    "bubble.png",
    "flower.png",
    "star_spin.gif",
    "heart_beat.png",
];

/// Every sticker, read once. Shipped set at `data/stickers.toml`.
static STICKERS: LazyLock<Vec<StickerSpec>> = LazyLock::new(|| {
    let entries: Vec<StickerEntry> = crate::catalog::load(
        "stickers",
        include_str!("../data/stickers.toml"),
        |entries: &[StickerEntry]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
            for entry in entries {
                if entry.width == 0 || entry.height == 0 {
                    return Err(format!("sticker '{}' has a zero dimension", entry.id));
                }
                if entry.file.trim().is_empty() {
                    return Err(format!("sticker '{}' names no art file", entry.id));
                }
            }
            Ok(())
        },
    );
    let mut specs = Vec::with_capacity(entries.len());
    for entry in entries {
        match resolve_art(&entry.file) {
            Some((bytes, source)) => specs.push(StickerSpec {
                id: entry.id,
                label: entry.label,
                width: entry.width,
                height: entry.height,
                animated: entry.animated,
                bytes,
                source,
            }),
            None => crate::catalog::amend(
                "stickers",
                format!("sticker '{}': no art at '{}'", entry.id, entry.file),
            ),
        }
    }
    specs
});

/// Installed art wins over the bundled copy, so replacing the starter pack is a
/// file drop. A file name with a path separator in it is refused: art lives
/// directly in the art directory, and `..` must not walk out of it.
fn resolve_art(file: &str) -> Option<(Vec<u8>, ArtSource)> {
    if file.contains('/') || file.contains('\\') {
        return None;
    }
    if let Some(dir) = crate::catalog::root()
        && let Ok(bytes) = std::fs::read(dir.join("stickers").join(file))
        && !bytes.is_empty()
    {
        return Some((bytes, ArtSource::Installed));
    }
    BUNDLED
        .iter()
        .find(|(name, _)| *name == file)
        .map(|(_, bytes)| (bytes.to_vec(), ArtSource::Bundled))
}

/// Every sticker, in UI display order.
pub fn sticker_catalog() -> &'static [StickerSpec] {
    &STICKERS
}

/// The spec for `id`, or `None` for an unknown id.
pub fn sticker_spec(id: &str) -> Option<&'static StickerSpec> {
    STICKERS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_lookup_works() {
        let catalog = sticker_catalog();
        assert!(!catalog.is_empty());
        for (i, spec) in catalog.iter().enumerate() {
            assert_eq!(
                sticker_spec(&spec.id),
                Some(spec),
                "lookup of '{}'",
                spec.id
            );
            assert!(
                !catalog[..i].iter().any(|s| s.id == spec.id),
                "duplicate sticker id '{}'",
                spec.id
            );
            assert!(!spec.bytes.is_empty(), "'{}' has no art", spec.id);
            assert!(spec.width > 0 && spec.height > 0);
        }
        assert_eq!(sticker_spec("nope"), None);
    }

    /// Every shipped entry resolves to art, so the browser has no dead tile.
    ///
    /// The class this closes: the art used to be `include_bytes!` beside the
    /// entry, which made "declared but missing" impossible. Now that art is a
    /// file name, a catalog entry can name art that does not exist, and the
    /// answer must be a dropped entry, never an offered sticker with no bytes.
    #[test]
    fn shipped_entries_use_the_bundled_art() {
        for spec in sticker_catalog() {
            assert_eq!(
                spec.source,
                ArtSource::Bundled,
                "'{}' did not come from the bundled pack",
                spec.id
            );
        }
    }

    #[test]
    fn art_outside_the_art_directory_is_refused() {
        assert_eq!(resolve_art("../secrets.png"), None);
        assert_eq!(resolve_art("nested/heart.png"), None);
        assert_eq!(resolve_art("not_a_file.png"), None);
        let (bytes, source) = resolve_art("heart.png").expect("bundled heart");
        assert!(!bytes.is_empty());
        assert_eq!(source, ArtSource::Bundled);
    }
}
