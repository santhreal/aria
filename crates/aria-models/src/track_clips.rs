//! The clips of one lane and the revision the lane keeps for each.

use crate::Map;
use crate::clip::Clip;
use crate::content_hash::Kept;
use crate::ids::ClipId;

/// The clips of a lane by id, and the revision the lane keeps for each: a hash
/// of everything the clip saves, computed on the first read.
///
/// Reads go through `Deref`. Every write goes through a method here, and each
/// drops the kept revision of every clip it hands out or takes in. The lane
/// holds the revisions rather than the clips: a clip written through its
/// fields, or a whole clip written over one the lane handed out, holds no hash
/// to carry in.
#[derive(Clone, Default)]
pub(crate) struct LaneClips {
    clips: Map<ClipId, Clip>,
    /// One per clip in `clips`, by the same id.
    revisions: Map<ClipId, Kept>,
}

impl LaneClips {
    pub(crate) fn new(clips: Map<ClipId, Clip>) -> Self {
        let revisions = clips.keys().map(|&id| (id, Kept::default())).collect();
        Self { clips, revisions }
    }

    /// The revision of the clip `id`, or `None` for a clip not here.
    pub(crate) fn revision(&self, id: ClipId) -> Option<u64> {
        let clip = self.clips.get(&id)?;
        let hash = || {
            #[cfg(test)]
            CLIPS_HASHED.with(|count| count.set(count.get() + 1));
            crate::content_hash::content_hash(clip)
        };
        Some(match self.revisions.get(&id) {
            Some(kept) => kept.get_or(hash),
            None => hash().max(1),
        })
    }

    pub(crate) fn get_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        if let Some(kept) = self.revisions.get_mut(&id) {
            kept.forget();
        }
        self.clips.get_mut(&id)
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut Clip> {
        self.revisions.values_mut().for_each(Kept::forget);
        self.clips.values_mut()
    }

    pub(crate) fn insert(&mut self, clip: Clip) -> Option<Clip> {
        self.revisions.insert(clip.id, Kept::default());
        self.clips.insert(clip.id, clip)
    }

    pub(crate) fn remove(&mut self, id: ClipId) -> Option<Clip> {
        self.revisions.remove(&id);
        self.clips.remove(&id)
    }

    /// The clips as `serde_map` saves them, or, inside
    /// [`Track::revision`](crate::Track::revision), as each clip's id and
    /// revision in id order. The lane's revision still follows the content of
    /// every clip, and a write to one clip hashes that clip again rather than
    /// every clip on the lane.
    pub(crate) fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !crate::content_hash::in_summary() {
            return crate::serde_map::serialize(&self.clips, serializer);
        }
        let mut revisions: Vec<(ClipId, u64)> = self
            .clips
            .keys()
            .filter_map(|&id| Some((id, self.revision(id)?)))
            .collect();
        revisions.sort_unstable_by_key(|(id, _)| *id);
        serializer.collect_seq(revisions)
    }
}

impl std::ops::Deref for LaneClips {
    type Target = Map<ClipId, Clip>;

    fn deref(&self) -> &Self::Target {
        &self.clips
    }
}

impl std::fmt::Debug for LaneClips {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.clips, f)
    }
}

#[cfg(test)]
thread_local! {
    /// Clips whose revision was hashed on this thread.
    pub(crate) static CLIPS_HASHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
