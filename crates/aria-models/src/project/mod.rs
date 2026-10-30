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
