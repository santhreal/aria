//! Clip linkage: which clips move, trim and delete together.
//!
//! A link is a group id stamped on every member ([`Clip::link`]). Everything
//! that decides *membership* lives here, so the engine's undoable action and a
//! script driving the model directly cannot disagree about what linking means:
//! the engine adds the inverse-capture, not the rules.
//!
//! [`Clip::link`]: crate::Clip::link

use std::collections::BTreeSet;

use crate::error::ModelError;
use crate::ids::{ClipId, LinkId};
use crate::project::Project;

/// One clip's target link value. A plan is a whole group's worth of them, so
/// applying it is atomic and its inverse is the same shape.
pub type LinkAssignment = (ClipId, Option<LinkId>);

impl Project {
    /// The assignments that put `clips` into one freshly allocated group.
    ///
    /// Every input must exist. An empty set is rejected: a group of nothing is
    /// not a link, and silently succeeding would leave a caller believing it
    /// had linked something.
    pub fn link_plan(&self, clips: &[ClipId]) -> Result<Vec<LinkAssignment>, ModelError> {
        if clips.is_empty() {
            return Err(ModelError::InvalidRange);
        }
        for &clip in clips {
            if self.clip(clip).is_none() {
                return Err(ModelError::UnknownClip(clip));
            }
        }
        let link = LinkId::next();
        Ok(clips.iter().map(|&clip| (clip, Some(link))).collect())
    }

    /// The assignments that dissolve every group touched by `clips`.
    ///
    /// Inputs are validated before anything is planned. An unlinked input is
    /// ignored while at least one input is linked; an entirely unlinked set is
    /// rejected, because "unlink these" cannot be satisfied by doing nothing.
    /// Members are listed in track/clip order so the plan is deterministic.
    pub fn unlink_plan(&self, clips: &[ClipId]) -> Result<Vec<LinkAssignment>, ModelError> {
        if clips.is_empty() {
            return Err(ModelError::InvalidRange);
        }
        let mut touched = BTreeSet::new();
        for &clip_id in clips {
            let clip = self.clip(clip_id).ok_or(ModelError::UnknownClip(clip_id))?;
            if let Some(link) = clip.link {
                touched.insert(link);
            }
        }
        if touched.is_empty() {
            return Err(ModelError::InvalidRange);
        }
        let mut plan = Vec::new();
        for track in self.timeline().tracks_ordered() {
            for clip in track.clips_ordered() {
                if clip.link.is_some_and(|link| touched.contains(&link)) {
                    plan.push((clip.id, None));
                }
            }
        }
        Ok(plan)
    }

    /// Apply a plan, returning the plan that undoes it.
    ///
    /// The whole set is validated first, so a missing clip cannot leave a group
    /// half-linked — the state a later unlink would silently mis-report.
    pub fn set_clip_links(
        &mut self,
        plan: &[LinkAssignment],
    ) -> Result<Vec<LinkAssignment>, ModelError> {
        for &(clip, _) in plan {
            if self.clip(clip).is_none() {
                return Err(ModelError::UnknownClip(clip));
            }
        }
        let mut inverse = Vec::with_capacity(plan.len());
        for &(clip_id, link) in plan {
            let clip = self
                .timeline_mut()
                .clip_mut(clip_id)
                .ok_or(ModelError::UnknownClip(clip_id))?;
            inverse.push((clip_id, clip.link));
            clip.link = link;
        }
        Ok(inverse)
    }

    /// Link `clips` as one group, returning its id.
    pub fn link_clips(&mut self, clips: &[ClipId]) -> Result<LinkId, ModelError> {
        let plan = self.link_plan(clips)?;
        let link = plan
            .first()
            .and_then(|&(_, link)| link)
            .ok_or(ModelError::InvalidRange)?;
        self.set_clip_links(&plan)?;
        Ok(link)
    }

    /// Dissolve every group touched by `clips`.
    pub fn unlink_clips(&mut self, clips: &[ClipId]) -> Result<(), ModelError> {
        let plan = self.unlink_plan(clips)?;
        self.set_clip_links(&plan)?;
        Ok(())
    }

    /// Live members of `link`, in track/clip order.
    pub fn linked_clips(&self, link: LinkId) -> Vec<ClipId> {
        self.timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips_ordered())
            .filter(|clip| clip.link == Some(link))
            .map(|clip| clip.id)
            .collect()
    }
}
