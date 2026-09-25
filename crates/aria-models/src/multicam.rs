//! Multicam camera groups, angle definitions, and angle cut lists.

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::{MediaId, MulticamGroupId};

/// One camera angle in a multicam group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MulticamAngle {
    pub media: MediaId,
    /// Sync offset in ticks relative to the group's common timeline.
    #[serde(default)]
    pub sync_offset: i64,
    /// Optional camera/angle label (e.g. "Cam A", "Wide", "Close-up").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl MulticamAngle {
    pub fn new(media: MediaId, sync_offset: i64) -> Self {
        Self {
            media,
            sync_offset,
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// A cut/switch to a specific camera angle at a timeline tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MulticamCut {
    /// The tick at which the angle switch takes effect.
    pub at: i64,
    /// The 0-based index of the angle in the group's `angles` list.
    pub angle: usize,
}

impl MulticamCut {
    pub const fn new(at: i64, angle: usize) -> Self {
        Self { at, angle }
    }
}

/// A synchronized multi-camera group consisting of N angles and an ordered cut list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MulticamGroup {
    pub id: MulticamGroupId,
    pub name: String,
    pub angles: Vec<MulticamAngle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<MulticamCut>,
}

impl MulticamGroup {
    /// Create a new multicam group with a freshly allocated ID and validate it.
    pub fn new(name: impl Into<String>, angles: Vec<MulticamAngle>) -> Result<Self, ModelError> {
        let group = Self {
            id: MulticamGroupId::next(),
            name: name.into(),
            angles,
            cuts: Vec::new(),
        };
        group.validate()?;
        Ok(group)
    }

    /// Construct a multicam group from media IDs and sync offsets.
    pub fn from_media(
        name: impl Into<String>,
        media_with_offsets: impl IntoIterator<Item = (MediaId, i64)>,
    ) -> Result<Self, ModelError> {
        let angles = media_with_offsets
            .into_iter()
            .map(|(media, sync_offset)| MulticamAngle::new(media, sync_offset))
            .collect();
        Self::new(name, angles)
    }

    /// Validate the group invariants:
    /// - At least one angle.
    /// - No duplicate media among angles.
    /// - All angle indices in cuts are `< angles.len()`.
    /// - Cuts are sorted strictly by `at` tick with no duplicate ticks.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.angles.is_empty() {
            return Err(ModelError::EmptyMulticamGroup);
        }
        let mut seen = rustc_hash::FxHashSet::default();
        for angle in &self.angles {
            if !seen.insert(angle.media) {
                return Err(ModelError::DuplicateMulticamMedia(angle.media));
            }
        }
        let mut last_tick = None;
        for cut in &self.cuts {
            if cut.angle >= self.angles.len() {
                return Err(ModelError::MulticamAngleOutOfBounds {
                    index: cut.angle,
                    total: self.angles.len(),
                });
            }
            if let Some(prev) = last_tick {
                if cut.at <= prev {
                    return Err(ModelError::InvalidMulticamCuts);
                }
            }
            last_tick = Some(cut.at);
        }
        Ok(())
    }

    /// Add an angle to the group. Returns the index of the added angle.
    pub fn add_angle(&mut self, angle: MulticamAngle) -> Result<usize, ModelError> {
        if self.angles.iter().any(|a| a.media == angle.media) {
            return Err(ModelError::DuplicateMulticamMedia(angle.media));
        }
        let index = self.angles.len();
        self.angles.push(angle);
        Ok(index)
    }

    /// Remove an angle by index. Adjusts existing cuts to remain in bounds.
    pub fn remove_angle(&mut self, index: usize) -> Result<MulticamAngle, ModelError> {
        if index >= self.angles.len() {
            return Err(ModelError::MulticamAngleOutOfBounds {
                index,
                total: self.angles.len(),
            });
        }
        if self.angles.len() <= 1 {
            return Err(ModelError::EmptyMulticamGroup);
        }
        let removed = self.angles.remove(index);
        self.cuts.retain_mut(|cut| {
            if cut.angle == index {
                cut.angle = 0;
            } else if cut.angle > index {
                cut.angle -= 1;
            }
            true
        });
        Ok(removed)
    }

    /// Switch the active angle at a tick. If a switch already exists at `at`, it replaces it.
    /// Otherwise, inserts it in sorted order.
    pub fn switch_angle(&mut self, at: i64, angle: usize) -> Result<(), ModelError> {
        if angle >= self.angles.len() {
            return Err(ModelError::MulticamAngleOutOfBounds {
                index: angle,
                total: self.angles.len(),
            });
        }
        if let Some(pos) = self.cuts.iter().position(|c| c.at == at) {
            self.cuts[pos].angle = angle;
        } else {
            let insert_pos = self.cuts.partition_point(|c| c.at < at);
            self.cuts.insert(insert_pos, MulticamCut::new(at, angle));
        }
        Ok(())
    }

    /// Move one angle on the group's common clock, returning the offset it
    /// replaced.
    ///
    /// An offset is where that camera's first frame sits on the common clock, so
    /// a camera that started rolling late carries a positive one. Any value is
    /// legal, including one that puts a camera entirely outside the group's span:
    /// a file that turns out to be from another shoot is the operator's finding,
    /// not an invariant to defend, and the resolver already stalls on the first
    /// frame of a read before a camera started.
    pub fn set_angle_offset(&mut self, index: usize, sync_offset: i64) -> Result<i64, ModelError> {
        let total = self.angles.len();
        let angle = self
            .angles
            .get_mut(index)
            .ok_or(ModelError::MulticamAngleOutOfBounds { index, total })?;
        Ok(std::mem::replace(&mut angle.sync_offset, sync_offset))
    }

    /// Read the active angle index at a given tick (last switch at or before `at`).
    /// If `at` is before all cuts or if cuts is empty, returns angle 0.
    pub fn active_angle_at(&self, at: i64) -> usize {
        angle_at(&self.cuts, at)
    }

    /// The angle live at a given tick, or `None` when the cut list names an
    /// angle the group does not hold.
    ///
    /// Every reader of a live camera goes through this: the media, the sync
    /// offset and the renderer's decode all pick the same angle by
    /// construction, so a cut list and an angle list that disagree cannot
    /// answer one of those questions differently from another.
    pub fn live_at(&self, at: i64) -> Option<&MulticamAngle> {
        self.angles.get(self.active_angle_at(at))
    }

    /// Read the active media ID at a given tick.
    pub fn active_media_at(&self, at: i64) -> Option<MediaId> {
        self.live_at(at).map(|angle| angle.media)
    }

    /// Active sync offset at a given tick.
    pub fn active_sync_offset_at(&self, at: i64) -> i64 {
        self.live_at(at).map_or(0, |angle| angle.sync_offset)
    }
}

/// Which angle a cut list makes live at `at`.
///
/// The rule — a cut holds from its own tick until the next one, and angle 0
/// holds before the first — is here rather than on [`MulticamGroup`] because
/// the editor projects the cut list to its panels and resolves it per drawn
/// frame, while the group itself lives behind the engine lock. One function
/// means the sidebar and the timeline cannot disagree about which camera is
/// live at a tick.
pub fn angle_at(cuts: &[MulticamCut], at: i64) -> usize {
    let pos = cuts.partition_point(|cut| cut.at <= at);
    match pos {
        0 => 0,
        _ => cuts[pos - 1].angle,
    }
}
