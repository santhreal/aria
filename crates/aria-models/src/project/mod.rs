use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Map;
use crate::clip::Clip;
use crate::error::ModelError;
use crate::ids::{BinId, ClipId, MediaId, ProjectId, TrackId};
use crate::media::{MediaKind, MediaSource};
use crate::metadata::ProjectMetadata;
use crate::schema::ProjectSchema;
use crate::time::Rational;
use crate::timeline::Timeline;
use crate::track::{Track, TrackKind};

/// Top-level container: a media pool plus exactly one [`Timeline`].
///
/// `Project` is the aggregate root and the only place that can guarantee
/// referential integrity between clips and media, so clip creation goes through
/// [`add_clip`](Project::add_clip).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Document schema identity (version, kind, extensions).
    #[serde(
        serialize_with = "crate::schema::serialize",
        deserialize_with = "crate::schema::deserialize",
        alias = "schema_version"
    )]
    pub schema: ProjectSchema,
    pub id: ProjectId,
    pub name: String,
    pub metadata: ProjectMetadata,
    /// Per-project plugin key-value store (`plugin_id -> key -> raw bytes`).
    /// Absent from saves while empty, and serialized in deterministic `BTreeMap`
    /// key order with lowercase hex byte strings.
    #[serde(
        default,
        with = "plugin_data_serde",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub plugin_data: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    #[serde(with = "crate::serde_map")]
    media: Map<MediaId, MediaSource>,
    /// Pool folders. Absent from saves while empty, so a flat pool costs
    /// nothing on disk and an old file loads with no bins.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    bins: Map<BinId, crate::bin::Bin>,
    /// Positions of media items at the pool root in freeform view.
    /// Absent from saves while empty, so an unarranged pool costs nothing on disk.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    root_positions: Map<MediaId, crate::bin::PoolPosition>,
    /// Multicam groups. Absent from saves while empty.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    multicam_groups: Map<crate::ids::MulticamGroupId, crate::multicam::MulticamGroup>,
    /// Secondary sequences. Absent from saves while empty.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    sequences: Map<crate::ids::SequenceId, crate::sequence::Sequence>,
    timeline: Timeline,
    #[serde(skip)]
    active_sequence: Option<crate::ids::SequenceId>,
    /// Largest video dimensions referenced by the open timeline. Render
    /// resolution reads this every frame. Timeline and media mutators
    /// invalidate it before returning a mutable reference, and
    /// [`Project::edit_clip`] when an edit changed the media a clip places.
    #[serde(skip)]
    largest_used_video: std::sync::OnceLock<Option<(u32, u32)>>,
}

mod attributes;
mod audio_inserts;
mod bake;
mod edit;
mod helpers;
mod layout;
mod link;
mod link_drift;
mod look;
mod media;
#[path = "multicam.rs"]
mod multicam;
mod params;
mod parent;
mod placement;
mod references;
#[path = "sequence.rs"]
mod sequence;
mod transitions;
mod trim_reach;

pub use link::{EnableAssignment, GroupAssignment, LinkAssignment};
pub use trim_reach::TrimReach;

#[cfg(test)]
mod tests;

impl Project {
    /// Create an empty project whose timeline runs at `frame_rate`.
    pub fn new(name: impl Into<String>, frame_rate: Rational) -> Self {
        Self {
            schema: ProjectSchema::current(),
            id: ProjectId::next(),
            name: name.into(),
            metadata: ProjectMetadata::default(),
            plugin_data: BTreeMap::new(),
            media: Map::default(),
            bins: Map::default(),
            root_positions: Map::default(),
            multicam_groups: Map::default(),
            sequences: Map::default(),
            active_sequence: None,
            largest_used_video: std::sync::OnceLock::new(),
            timeline: Timeline::new(frame_rate),
        }
    }

    pub fn schema(&self) -> &ProjectSchema {
        &self.schema
    }

    pub fn metadata(&self) -> &ProjectMetadata {
        &self.metadata
    }

    pub fn metadata_mut(&mut self) -> &mut ProjectMetadata {
        &mut self.metadata
    }
    /// Look up `key` in `plugin_id`'s per-project key-value store.
    pub fn plugin_storage_get(&self, plugin_id: &str, key: &str) -> Option<&[u8]> {
        self.plugin_data
            .get(plugin_id)
            .and_then(|entries| entries.get(key))
            .map(Vec::as_slice)
    }

    /// Write `value` under `key` in `plugin_id`'s per-project key-value store.
    /// Returns `true` when the stored bytes changed.
    pub fn plugin_storage_set(&mut self, plugin_id: &str, key: &str, value: Vec<u8>) -> bool {
        if self.plugin_storage_get(plugin_id, key) == Some(value.as_slice()) {
            return false;
        }
        self.plugin_data
            .entry(plugin_id.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
        true
    }

    /// Remove `key` from `plugin_id`'s per-project key-value store, returning
    /// `true` when an entry was removed.
    pub fn plugin_storage_delete(&mut self, plugin_id: &str, key: &str) -> bool {
        let Some(entries) = self.plugin_data.get_mut(plugin_id) else {
            return false;
        };
        let removed = entries.remove(key).is_some();
        if entries.is_empty() {
            self.plugin_data.remove(plugin_id);
        }
        removed
    }

    /// All keys stored for `plugin_id` in sorted order.
    pub fn plugin_storage_keys(&self, plugin_id: &str) -> Vec<String> {
        self.plugin_data
            .get(plugin_id)
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    // --- timeline ---------------------------------------------------------

    /// Largest video dimensions referenced by the open timeline.
    ///
    /// The first read after a timeline, sequence, or media mutation computes
    /// the value. Frame resolution then reads the `OnceLock` in O(1), and an
    /// edit through [`Self::edit_clip`] keeps it.
    pub fn largest_used_video_dimensions(&self) -> Option<(u32, u32)> {
        *self
            .largest_used_video
            .get_or_init(|| self.largest_video_dimensions_on(self.timeline()))
    }

    /// Largest video dimensions referenced by `timeline`, the open one or a
    /// nested sequence's, computed on every call. Visual lanes are read bottom
    /// to top and each lane's clips in timeline order, so of two sources with
    /// the same area the one placed first sizes the canvas.
    pub fn largest_video_dimensions_on(&self, timeline: &Timeline) -> Option<(u32, u32)> {
        self.largest_video_among(
            timeline
                .tracks_ordered()
                .filter(|track| track.kind.is_visual())
                .flat_map(Track::clips_ordered)
                .filter_map(Clip::media),
        )
    }

    /// Largest dimensions among the sources `media`, by area: of two with the
    /// same area, the earlier. A still, a sound-only source and an id not in
    /// the pool are skipped.
    ///
    /// The single definition of which placed source sizes an `Auto` canvas. A
    /// caller holding the sources each visual lane places, lanes bottom to top
    /// and each lane's sources in the timeline order of the clip first placing
    /// it, reads the size
    /// [`largest_video_dimensions_on`](Self::largest_video_dimensions_on)
    /// reads, without a walk over every clip.
    pub fn largest_video_among(
        &self,
        media: impl IntoIterator<Item = MediaId>,
    ) -> Option<(u32, u32)> {
        let mut best: Option<(u32, u32)> = None;
        for id in media {
            let Some(media) = self.media(id) else {
                continue;
            };
            if media.kind() != MediaKind::Video {
                continue;
            }
            let area = u64::from(media.width) * u64::from(media.height);
            if best.is_none_or(|(width, height)| area > u64::from(width) * u64::from(height)) {
                best = Some((media.width, media.height));
            }
        }
        best
    }

    fn invalidate_largest_used_video(&mut self) {
        let _ = self.largest_used_video.take();
    }

    /// The timeline being edited: the open secondary sequence's, or the root
    /// one when nothing is open.
    ///
    /// This is what "the timeline" means to an editing surface. A nested
    /// sequence is opened for editing, so every placement, look, marker and
    /// lane operation belongs to whatever is open; a caller that means the
    /// project's own root timeline regardless of that says
    /// [`root_timeline`](Self::root_timeline). Splitting those two apart is
    /// what stops a lane added inside a sequence from landing on the root,
    /// where the clip placed on it afterwards could not find it.
    pub fn timeline(&self) -> &Timeline {
        if let Some(id) = self.active_sequence
            && let Some(seq) = self.sequences.get(&id)
        {
            return &seq.timeline;
        }
        &self.timeline
    }

    /// The timeline being edited, mutably. See [`timeline`](Self::timeline).
    pub fn timeline_mut(&mut self) -> &mut Timeline {
        self.invalidate_largest_used_video();
        self.open_timeline_mut()
    }

    /// [`Self::timeline_mut`] with the largest video size kept.
    fn open_timeline_mut(&mut self) -> &mut Timeline {
        if let Some(id) = self.active_sequence
            && let Some(seq) = self.sequences.get_mut(&id)
        {
            return &mut seq.timeline;
        }
        &mut self.timeline
    }

    /// Apply `edit` to the clip `clip_id` on the open timeline: an edit to
    /// how the clip draws or sounds, never to where it sits.
    ///
    /// The largest video size an `Auto` canvas reads stays cached unless
    /// `edit` changed the media the clip places, so a parameter scrub reads
    /// it without a walk over every clip. An edit that moves the clip goes
    /// through [`Self::clip_timeline_mut`].
    pub fn edit_clip<R>(
        &mut self,
        clip_id: ClipId,
        edit: impl FnOnce(&mut Clip) -> Result<R, ModelError>,
    ) -> Result<R, ModelError> {
        let clip = self.open_timeline_mut().require_clip_mut(clip_id)?;
        let placed = clip.media();
        let edited = edit(&mut *clip);
        if clip.media() != placed {
            self.invalidate_largest_used_video();
        }
        edited
    }

    /// The project's own root timeline, whatever is open for editing.
    pub fn root_timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// The project's own root timeline, mutably.
    pub fn root_timeline_mut(&mut self) -> &mut Timeline {
        self.invalidate_largest_used_video();
        &mut self.timeline
    }

    /// Convenience: create and append a track, returning its [`TrackId`].
    pub fn add_track(&mut self, kind: TrackKind, name: impl Into<String>) -> TrackId {
        self.timeline_mut().add_track(Track::new(kind, name))
    }

    /// Convenience: create a track at `order_index` in the stack (0 = bottom
    /// layer; clamped), returning its [`TrackId`].
    pub fn insert_track(
        &mut self,
        kind: TrackKind,
        name: impl Into<String>,
        order_index: usize,
    ) -> TrackId {
        self.timeline_mut()
            .insert_track(Track::new(kind, name), order_index)
    }

    /// Find a clip by id anywhere in the project: the open timeline first, then
    /// the root, then every other sequence.
    pub fn clip(&self, clip_id: ClipId) -> Option<&Clip> {
        if let Some(clip) = self.timeline().clip(clip_id) {
            return Some(clip);
        }
        if let Some(clip) = self.root_timeline().clip(clip_id) {
            return Some(clip);
        }
        for seq in self.sequences.values() {
            if let Some(clip) = seq.timeline.clip(clip_id) {
                return Some(clip);
            }
        }
        None
    }

    /// [`Self::clip`], or [`ModelError::UnknownClip`] when no timeline in the
    /// project holds it.
    pub fn require_clip(&self, clip_id: ClipId) -> Result<&Clip, ModelError> {
        self.clip(clip_id).ok_or(ModelError::UnknownClip(clip_id))
    }

    /// [`Self::clip_mut`], or [`ModelError::UnknownClip`] when no timeline in
    /// the project holds it.
    pub fn require_clip_mut(&mut self, clip_id: ClipId) -> Result<&mut Clip, ModelError> {
        self.clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))
    }

    /// Find a clip by id mutably anywhere in the project.
    pub fn clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        if self.timeline().clip(clip_id).is_some() {
            return self.timeline_mut().clip_mut(clip_id);
        }
        if self.root_timeline().clip(clip_id).is_some() {
            return self.root_timeline_mut().clip_mut(clip_id);
        }
        for seq in self.sequences.values_mut() {
            if seq.timeline.clip(clip_id).is_some() {
                return seq.timeline.clip_mut(clip_id);
            }
        }
        None
    }

    /// Find a clip by id for an edit that may change its timeline start.
    pub fn clip_timeline_mut(&mut self, clip_id: ClipId) -> Option<crate::ClipMut<'_>> {
        if self.timeline().clip(clip_id).is_some() {
            return self.timeline_mut().clip_timeline_mut(clip_id);
        }
        if self.root_timeline().clip(clip_id).is_some() {
            return self.root_timeline_mut().clip_timeline_mut(clip_id);
        }
        for seq in self.sequences.values_mut() {
            if seq.timeline.clip(clip_id).is_some() {
                return seq.timeline.clip_timeline_mut(clip_id);
            }
        }
        None
    }
}

mod plugin_data_serde {
    use std::collections::BTreeMap;
    use std::fmt;

    use serde::de::{self, Deserializer, SeqAccess, Visitor};
    use serde::ser::{SerializeMap, Serializer};
    use serde::{Deserialize, Serialize};

    struct HexBytesRef<'a>(&'a [u8]);

    impl Serialize for HexBytesRef<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            const LUT: &[u8; 16] = b"0123456789abcdef";
            let mut hex = String::with_capacity(self.0.len() * 2);
            for &b in self.0 {
                hex.push(LUT[(b >> 4) as usize] as char);
                hex.push(LUT[(b & 0x0f) as usize] as char);
            }
            serializer.serialize_str(&hex)
        }
    }

    struct InnerMapRef<'a>(&'a BTreeMap<String, Vec<u8>>);

    impl Serialize for InnerMapRef<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for (k, v) in self.0 {
                map.serialize_entry(k, &HexBytesRef(v))?;
            }
            map.end()
        }
    }

    pub(super) fn serialize<S: Serializer>(
        data: &BTreeMap<String, BTreeMap<String, Vec<u8>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let non_empty = data.values().filter(|m| !m.is_empty()).count();
        let mut map = serializer.serialize_map(Some(non_empty))?;
        for (plugin_id, entries) in data {
            if !entries.is_empty() {
                map.serialize_entry(plugin_id, &InnerMapRef(entries))?;
            }
        }
        map.end()
    }

    struct HexBytes(Vec<u8>);

    impl<'de> Deserialize<'de> for HexBytes {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct HexVisitor;

            impl<'de> Visitor<'de> for HexVisitor {
                type Value = HexBytes;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a hex-encoded byte string or an array of u8")
                }

                fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                    if !v.len().is_multiple_of(2) {
                        return Err(E::custom("hex byte string must have an even length"));
                    }
                    let mut out = Vec::with_capacity(v.len() / 2);
                    let bytes = v.as_bytes();
                    let mut i = 0;
                    while i < bytes.len() {
                        let hi = decode_nibble(bytes[i]).ok_or_else(|| {
                            E::custom(format!("invalid hex character {:?}", bytes[i] as char))
                        })?;
                        let lo = decode_nibble(bytes[i + 1]).ok_or_else(|| {
                            E::custom(format!("invalid hex character {:?}", bytes[i + 1] as char))
                        })?;
                        out.push((hi << 4) | lo);
                        i += 2;
                    }
                    Ok(HexBytes(out))
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                    while let Some(byte) = seq.next_element::<u8>()? {
                        out.push(byte);
                    }
                    Ok(HexBytes(out))
                }
            }

            deserializer.deserialize_any(HexVisitor)
        }
    }

    fn decode_nibble(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, BTreeMap<String, Vec<u8>>>, D::Error> {
        let raw = BTreeMap::<String, BTreeMap<String, HexBytes>>::deserialize(deserializer)?;
        Ok(raw
            .into_iter()
            .filter_map(|(plugin_id, entries)| {
                let inner: BTreeMap<String, Vec<u8>> =
                    entries.into_iter().map(|(k, HexBytes(v))| (k, v)).collect();
                (!inner.is_empty()).then_some((plugin_id, inner))
            })
            .collect())
    }
}
