#![allow(unused_imports)]

use std::path::Path;

use crate::bin::Bin;
use crate::clip::{
    Clip, ClipParam, ClipSource, ClipTransform, CropRect, Generator, ParamValue, Replaceable,
    SlotMedia, look_animation_combo_period_ticks, look_animation_window_ticks, split_speed_curve,
};
use crate::effects::EffectInstance;
use crate::error::ModelError;
use crate::ids::{BinId, ClipId, MediaId, ProjectId, TrackId};
use crate::look::{
    AnimationRef, AnimationSlot, AudioRole, ChromaKey, ColorAdjustments, Filter, Lut, Mask,
    StabilizeLevel, animation_spec,
};
use crate::media::MediaSource;
use crate::metadata::ProjectMetadata;
use crate::param::{Easing, Param};
use crate::schema::ProjectSchema;
use crate::time::{
    Rational, RationalTime, TimeRange, check_same_rate, resample, time_add, time_sub,
};
use crate::timeline::Timeline;
use crate::track::{Track, TrackKind};
use crate::transition::Transition;

use super::Project;

impl Project {
    // --- media pool -------------------------------------------------------

    /// Add a source to the media pool. Returns its [`MediaId`].
    pub fn add_media(&mut self, media: MediaSource) -> MediaId {
        self.invalidate_largest_used_video();
        let id = media.id;
        self.media.insert(id, media);
        id
    }

    pub fn media(&self, id: MediaId) -> Option<&MediaSource> {
        self.media.get(&id)
    }

    /// Mutable pool access for state repair (missing-media relink, M0):
    /// re-pointing an entry at a re-probed file in place, keeping its id so
    /// clips stay attached. Editing flows must not mutate sources behind the
    /// timeline's back — placement math reads pool metadata.
    pub fn media_mut(&mut self, id: MediaId) -> Option<&mut MediaSource> {
        self.invalidate_largest_used_video();
        self.media.get_mut(&id)
    }

    pub fn media_iter(&self) -> impl Iterator<Item = &MediaSource> {
        self.media.values()
    }

    pub fn media_count(&self) -> usize {
        self.media.len()
    }

    /// The frame rate of the largest *video* source in the media pool (by pixel
    /// area, breaking ties with duration), or `None` when the pool holds no video.
    ///
    /// A still's [`frame_rate`](MediaSource::frame_rate) is
    /// [`STILL_TICK_RATE`](crate::media::STILL_TICK_RATE) — a tick base chosen so
    /// duration math stays exact, not a cadence the footage was shot at. Reading
    /// it as one told a project holding a single PNG that its footage ran at
    /// 1000 fps. Audio-only sources are excluded by the same rule.
    pub fn largest_source_frame_rate(&self) -> Option<Rational> {
        self.media_iter()
            .filter(|m| m.kind() == crate::media::MediaKind::Video)
            .filter(|m| m.width > 0 && m.height > 0 && m.frame_rate.is_valid())
            .max_by_key(|m| (u64::from(m.width) * u64::from(m.height), m.duration.value))
            .map(|m| m.frame_rate)
    }

    /// True iff the media pool holds a video source whose frame rate differs
    /// from the sequence frame rate, returning that source's frame rate.
    pub fn frame_rate_mismatch(&self) -> Option<Rational> {
        let source_rate = self.largest_source_frame_rate()?;
        if !crate::time::rate_eq(self.timeline().frame_rate, source_rate) {
            Some(source_rate)
        } else {
            None
        }
    }
    /// Lookup a pool entry by filesystem path (canonical comparison when possible).
    pub fn find_media_by_path(&self, path: &Path) -> Option<MediaId> {
        self.media_iter()
            .find(|m| super::helpers::paths_refer_to_same_file(m.path(), path))
            .map(|m| m.id)
    }

    /// Whether any clip or multicam group currently references `media_id`.
    pub fn is_media_referenced(&self, media_id: MediaId) -> bool {
        let in_timeline = self
            .timeline()
            .tracks_ordered()
            .flat_map(Track::clips)
            .any(|c| c.media() == Some(media_id));
        if in_timeline {
            return true;
        }
        self.multicam_groups
            .values()
            .any(|g| g.angles.iter().any(|a| a.media == media_id))
    }

    /// Remove a source from the pool. Fails if any clip still references it.
    pub fn remove_media(&mut self, media_id: MediaId) -> Result<MediaSource, ModelError> {
        if self.is_media_referenced(media_id) {
            return Err(ModelError::MediaReferenced(media_id));
        }
        self.root_positions.remove(&media_id);
        for bin in self.bins.values_mut() {
            bin.remove_position(media_id);
        }
        self.media
            .remove(&media_id)
            .ok_or(ModelError::UnknownMedia(media_id))
    }

    /// The freeform position of a pool item in `bin`, or at the pool root with `None`.
    pub fn pool_item_position(
        &self,
        bin: Option<BinId>,
        media: MediaId,
    ) -> Option<crate::bin::PoolPosition> {
        match bin {
            Some(bin_id) => self.bins.get(&bin_id).and_then(|b| b.position(media)),
            None => self.root_positions.get(&media).copied(),
        }
    }

    /// Set the freeform position of a pool item in `bin`, or at the pool root with `None`.
    pub fn set_pool_item_position(
        &mut self,
        bin: Option<BinId>,
        media: MediaId,
        position: crate::bin::PoolPosition,
    ) -> Result<(), ModelError> {
        if !self.media.contains_key(&media) {
            return Err(ModelError::UnknownMedia(media));
        }
        match bin {
            Some(bin_id) => {
                let bin = self
                    .bins
                    .get_mut(&bin_id)
                    .ok_or(ModelError::UnknownBin(bin_id))?;
                bin.set_position(media, position);
            }
            None => {
                self.root_positions.insert(media, position);
            }
        }
        Ok(())
    }

    /// Remove the freeform position of a pool item in `bin`, or at the pool root with `None`.
    pub fn remove_pool_item_position(
        &mut self,
        bin: Option<BinId>,
        media: MediaId,
    ) -> Result<Option<crate::bin::PoolPosition>, ModelError> {
        match bin {
            Some(bin_id) => {
                let bin = self
                    .bins
                    .get_mut(&bin_id)
                    .ok_or(ModelError::UnknownBin(bin_id))?;
                Ok(bin.remove_position(media))
            }
            None => Ok(self.root_positions.remove(&media)),
        }
    }

    // --- pool folders -----------------------------------------------------

    /// Create a bin, optionally inside `parent`.
    pub fn add_bin(
        &mut self,
        name: impl Into<String>,
        parent: Option<BinId>,
    ) -> Result<BinId, ModelError> {
        if let Some(parent) = parent
            && !self.bins.contains_key(&parent)
        {
            return Err(ModelError::UnknownBin(parent));
        }
        let bin = Bin::new(name, parent);
        let id = bin.id;
        self.bins.insert(id, bin);
        Ok(id)
    }

    /// Insert a bin that already has an id, so an undo can restore the one that
    /// was deleted rather than a new bin that merely looks like it.
    pub fn restore_bin(&mut self, bin: Bin) -> Result<(), ModelError> {
        if let Some(parent) = bin.parent
            && !self.bins.contains_key(&parent)
        {
            return Err(ModelError::UnknownBin(parent));
        }
        self.bins.insert(bin.id, bin);
        Ok(())
    }

    pub fn bin(&self, id: BinId) -> Option<&Bin> {
        self.bins.get(&id)
    }

    pub fn bin_iter(&self) -> impl Iterator<Item = &Bin> {
        self.bins.values()
    }

    pub fn bin_count(&self) -> usize {
        self.bins.len()
    }

    pub fn rename_bin(&mut self, id: BinId, name: impl Into<String>) -> Result<(), ModelError> {
        let bin = self.bins.get_mut(&id).ok_or(ModelError::UnknownBin(id))?;
        bin.name = name.into();
        Ok(())
    }

    /// Move a bin into `parent`, or to the top level with `None`.
    ///
    /// A bin cannot be moved into itself or into one of its own descendants: the
    /// pool is a tree, and a cycle would make a subtree unreachable from the
    /// root rather than merely oddly placed.
    pub fn move_bin(&mut self, id: BinId, parent: Option<BinId>) -> Result<(), ModelError> {
        if !self.bins.contains_key(&id) {
            return Err(ModelError::UnknownBin(id));
        }
        if let Some(parent) = parent {
            if !self.bins.contains_key(&parent) {
                return Err(ModelError::UnknownBin(parent));
            }
            if parent == id || self.bin_ancestors(parent).any(|step| step == id) {
                return Err(ModelError::BinCycle(id));
            }
        }
        self.bins
            .get_mut(&id)
            .ok_or(ModelError::UnknownBin(id))?
            .parent = parent;
        Ok(())
    }

    /// Delete a bin. Its media and child bins move up to its parent, because a
    /// folder is organisation: deleting one must never take a source with it.
    pub fn remove_bin(&mut self, id: BinId) -> Result<Bin, ModelError> {
        let bin = self.bins.remove(&id).ok_or(ModelError::UnknownBin(id))?;
        let parent = bin.parent;
        for child in self.bins.values_mut() {
            if child.parent == Some(id) {
                child.parent = parent;
            }
        }
        for source in self.media.values_mut() {
            if source.bin == Some(id) {
                source.bin = parent;
            }
        }
        Ok(bin)
    }

    /// Put a source in `bin`, or at the pool root with `None`.
    pub fn move_media_to_bin(
        &mut self,
        media: MediaId,
        bin: Option<BinId>,
    ) -> Result<Option<BinId>, ModelError> {
        if let Some(bin) = bin
            && !self.bins.contains_key(&bin)
        {
            return Err(ModelError::UnknownBin(bin));
        }
        let source = self
            .media
            .get_mut(&media)
            .ok_or(ModelError::UnknownMedia(media))?;
        let was = source.bin;
        source.bin = bin;
        Ok(was)
    }

    /// Every bin between `from` and the root, nearest first.
    ///
    /// Bounded by the number of bins, so a cycle written by a corrupt file walks
    /// out instead of hanging.
    pub fn bin_ancestors(&self, from: BinId) -> impl Iterator<Item = BinId> + '_ {
        let mut at = self.bins.get(&from).and_then(|bin| bin.parent);
        let mut left = self.bins.len();
        std::iter::from_fn(move || {
            let id = at?;
            if left == 0 {
                return None;
            }
            left -= 1;
            at = self.bins.get(&id).and_then(|bin| bin.parent);
            Some(id)
        })
    }

    /// Create a subclip in the media pool from an in/out range of parent media.
    pub fn create_subclip(
        &mut self,
        parent_id: MediaId,
        name: impl Into<String>,
        source_range: TimeRange,
        bin: Option<BinId>,
    ) -> Result<MediaId, ModelError> {
        if let Some(bin_id) = bin {
            if !self.bins.contains_key(&bin_id) {
                return Err(ModelError::UnknownBin(bin_id));
            }
        }
        let parent = self
            .media
            .get(&parent_id)
            .ok_or(ModelError::UnknownMedia(parent_id))?;
        let mut subclip = MediaSource::subclip(parent, name, source_range)?;
        if bin.is_some() {
            subclip.bin = bin;
        }
        let id = subclip.id;
        self.media.insert(id, subclip);
        Ok(id)
    }
}
