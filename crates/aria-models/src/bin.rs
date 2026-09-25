//! Folders in the media pool.
//!
//! A bin is organisation, not content: it holds no media of its own. A source
//! names the bin it sits in, and a bin names its parent, so the pool is a tree
//! walked from the root rather than a nesting of owned lists. That keeps a move
//! to a single field write and makes a missing parent a repairable state instead
//! of a lost subtree.

use serde::{Deserialize, Serialize};

use crate::Map;
use crate::ids::{BinId, MediaId};

/// Coordinates of a pool item within a bin in Freeform view.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoolPosition {
    pub x: f32,
    pub y: f32,
}

impl PoolPosition {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A folder in the media pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bin {
    pub id: BinId,
    pub name: String,
    /// The bin this one sits in, or `None` for a top-level bin. Absent from
    /// saves while unset, so a flat pool costs nothing on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<BinId>,
    /// Positions of media items in this bin for freeform view.
    /// Absent from saves while empty.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub positions: Map<MediaId, PoolPosition>,
}

impl Bin {
    /// A bin with a fresh id, in `parent`.
    pub fn new(name: impl Into<String>, parent: Option<BinId>) -> Self {
        Self {
            id: BinId::next(),
            name: name.into(),
            parent,
            positions: Map::default(),
        }
    }

    /// The stored freeform position for `media` in this bin, if arranged.
    pub fn position(&self, media: MediaId) -> Option<PoolPosition> {
        self.positions.get(&media).copied()
    }

    /// Set the freeform position for `media` in this bin.
    pub fn set_position(&mut self, media: MediaId, position: PoolPosition) {
        self.positions.insert(media, position);
    }

    /// Remove the freeform position for `media` in this bin.
    pub fn remove_position(&mut self, media: MediaId) -> Option<PoolPosition> {
        self.positions.remove(&media)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_positions_roundtrip() {
        let mut bin = Bin::new("Footage", None);
        let media1 = MediaId::next();
        let media2 = MediaId::next();
        assert_eq!(bin.position(media1), None);

        bin.set_position(media1, PoolPosition::new(10.0, 20.0));
        bin.set_position(media2, PoolPosition::new(30.0, 40.0));
        assert_eq!(bin.position(media1), Some(PoolPosition::new(10.0, 20.0)));
        assert_eq!(bin.position(media2), Some(PoolPosition::new(30.0, 40.0)));

        let json = serde_json::to_string(&bin).expect("serialize");
        let back: Bin = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.position(media1), Some(PoolPosition::new(10.0, 20.0)));
        assert_eq!(back.position(media2), Some(PoolPosition::new(30.0, 40.0)));

        bin.remove_position(media1);
        assert_eq!(bin.position(media1), None);
        assert_eq!(bin.position(media2), Some(PoolPosition::new(30.0, 40.0)));
    }

    #[test]
    fn empty_bin_positions_omitted_from_json() {
        let bin = Bin::new("Empty", None);
        let json = serde_json::to_string(&bin).expect("serialize");
        assert!(
            !json.contains("positions"),
            "empty positions map is omitted from serialization"
        );
        let back: Bin = serde_json::from_str(&json).expect("deserialize");
        assert!(back.positions.is_empty());
    }
}
