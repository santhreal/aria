//! Clip linkage: which clips move, trim and delete together.
//!
//! A link is a group id stamped on every member ([`Clip::link`]). Everything
//! that decides *membership* lives here, so the engine's undoable action and a
//! script driving the model directly cannot disagree about what linking means:
//! the engine adds the inverse-capture, not the rules.
//!
//! [`Clip::link`]: crate::Clip::link

use std::collections::BTreeSet;

use crate::error::{ClipSetRefusal, ModelError};
use crate::ids::{ClipId, GroupId, LinkId};
use crate::project::Project;

/// One clip's target link value. A plan is a whole group's worth of them, so
/// applying it is atomic and its inverse is the same shape.
pub type LinkAssignment = (ClipId, Option<LinkId>);
/// One clip's target group value.
pub type GroupAssignment = (ClipId, Option<GroupId>);
/// One clip's target enabled value.
pub type EnableAssignment = (ClipId, bool);

impl Project {
    /// The assignments that put `clips` into one freshly allocated group.
    ///
    /// Every input must exist, and a clip named twice counts once. Fewer than
    /// two different clips is rejected: a link of one clip moves nothing with
    /// it, and succeeding would leave a caller believing it had linked
    /// something.
    pub fn link_plan(&self, clips: &[ClipId]) -> Result<Vec<LinkAssignment>, ModelError> {
        let distinct = self.distinct_clips(clips)?;
        if distinct.len() < 2 {
            return Err(ModelError::ClipSet(ClipSetRefusal::LinkOfOne));
        }
        let link = LinkId::next();
        Ok(distinct
            .into_iter()
            .map(|clip| (clip, Some(link)))
            .collect())
    }

    /// `clips` in order with each clip once, or the first that does not exist.
    fn distinct_clips(&self, clips: &[ClipId]) -> Result<Vec<ClipId>, ModelError> {
        let mut distinct = Vec::with_capacity(clips.len());
        for &clip in clips {
            if self.clip(clip).is_none() {
                return Err(ModelError::UnknownClip(clip));
            }
            if !distinct.contains(&clip) {
                distinct.push(clip);
            }
        }
        Ok(distinct)
    }

    /// The assignments that dissolve every group touched by `clips`.
    ///
    /// Inputs are validated before anything is planned. An unlinked input is
    /// ignored while at least one input is linked; an entirely unlinked set is
    /// rejected, because "unlink these" cannot be satisfied by doing nothing.
    /// Members are listed in track/clip order so the plan is deterministic.
    pub fn unlink_plan(&self, clips: &[ClipId]) -> Result<Vec<LinkAssignment>, ModelError> {
        if clips.is_empty() {
            return Err(ModelError::ClipSet(ClipSetRefusal::NoClips));
        }
        let mut touched = BTreeSet::new();
        for &clip_id in clips {
            let clip = self.require_clip(clip_id)?;
            if let Some(link) = clip.link {
                touched.insert(link);
            }
        }
        if touched.is_empty() {
            return Err(ModelError::ClipSet(ClipSetRefusal::NoneLinked));
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

    /// Apply a plan, returning the plan that undoes it: the values each
    /// assignment replaced, newest first, so a plan that names a clip twice
    /// undoes to the value it held before the first.
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
            let clip = self.timeline_mut().require_clip_mut(clip_id)?;
            inverse.push((clip_id, clip.link));
            clip.link = link;
        }
        inverse.reverse();
        Ok(inverse)
    }

    /// Link `clips` as one group, returning its id.
    pub fn link_clips(&mut self, clips: &[ClipId]) -> Result<LinkId, ModelError> {
        let plan = self.link_plan(clips)?;
        let link = plan
            .first()
            .and_then(|&(_, link)| link)
            .ok_or(ModelError::ClipSet(ClipSetRefusal::LinkOfOne))?;
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

    /// The tails of one cut that link together. Each `(head, tail)` is a clip
    /// split at the same tick by [`Project::split_clip`]; the tails of heads
    /// that share a link form one group, so a picture and its sound stay
    /// paired on both sides of the cut. A tail whose head was unlinked, or was
    /// cut without any of its partners, is in no group.
    pub fn split_tail_links(&self, cuts: &[(ClipId, ClipId)]) -> Vec<Vec<ClipId>> {
        let mut tails: Vec<(LinkId, ClipId)> = cuts
            .iter()
            .filter_map(|&(head, tail)| Some((self.clip(head)?.link?, tail)))
            .collect();
        tails.sort_unstable_by_key(|&(link, _)| link);
        tails
            .chunk_by(|a, b| a.0 == b.0)
            .filter(|group| group.len() > 1)
            .map(|group| group.iter().map(|&(_, tail)| tail).collect())
            .collect()
    }

    /// The assignments that put `clips` into one freshly allocated group.
    ///
    /// Requires at least two distinct clips, all of which must exist. If
    /// re-grouping removes members from an existing group and leaves a single
    /// survivor, that survivor is ungrouped in the same plan so undoing
    /// restores the full previous group.
    pub fn group_plan(&self, clips: &[ClipId]) -> Result<Vec<GroupAssignment>, ModelError> {
        let distinct = self.distinct_clips(clips)?;
        if distinct.len() < 2 {
            return Err(ModelError::ClipSet(ClipSetRefusal::GroupOfOne));
        }
        let group = GroupId::next();
        let mut plan: Vec<GroupAssignment> =
            distinct.iter().map(|&clip| (clip, Some(group))).collect();
        let mut old_groups = BTreeSet::new();
        for &clip_id in &distinct {
            if let Some(old) = self.clip(clip_id).and_then(|c| c.group) {
                old_groups.insert(old);
            }
        }
        for old in old_groups {
            let remaining: Vec<ClipId> = self
                .grouped_clips(old)
                .into_iter()
                .filter(|id| !distinct.contains(id))
                .collect();
            if remaining.len() == 1 {
                plan.push((remaining[0], None));
            }
        }
        Ok(plan)
    }

    /// The assignments that dissolve every group touched by `clips`.
    pub fn ungroup_plan(&self, clips: &[ClipId]) -> Result<Vec<GroupAssignment>, ModelError> {
        if clips.is_empty() {
            return Err(ModelError::ClipSet(ClipSetRefusal::NoClips));
        }
        let mut touched = BTreeSet::new();
        for &clip_id in clips {
            let clip = self.require_clip(clip_id)?;
            if let Some(group) = clip.group {
                touched.insert(group);
            }
        }
        if touched.is_empty() {
            return Err(ModelError::ClipSet(ClipSetRefusal::NoneGrouped));
        }
        let mut plan = Vec::new();
        for track in self.timeline().tracks_ordered() {
            for clip in track.clips_ordered() {
                if clip.group.is_some_and(|group| touched.contains(&group)) {
                    plan.push((clip.id, None));
                }
            }
        }
        Ok(plan)
    }

    /// Apply a group plan, returning the plan that undoes it, newest first as
    /// [`Self::set_clip_links`] returns it.
    pub fn set_clip_groups(
        &mut self,
        plan: &[GroupAssignment],
    ) -> Result<Vec<GroupAssignment>, ModelError> {
        for &(clip, _) in plan {
            if self.clip(clip).is_none() {
                return Err(ModelError::UnknownClip(clip));
            }
        }
        let mut inverse = Vec::with_capacity(plan.len());
        for &(clip_id, group) in plan {
            let clip = self.timeline_mut().require_clip_mut(clip_id)?;
            inverse.push((clip_id, clip.group));
            clip.group = group;
        }
        inverse.reverse();
        Ok(inverse)
    }

    /// Live members of `group`, in track/clip order.
    pub fn grouped_clips(&self, group: GroupId) -> Vec<ClipId> {
        self.timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips_ordered())
            .filter(|clip| clip.group == Some(group))
            .map(|clip| clip.id)
            .collect()
    }

    /// Every clip that moves together with `primary` while links are not
    /// followed: the clips of its [`Clip::group`](crate::Clip::group) whose
    /// lanes are not locked, in track/clip order, or `primary` alone.
    pub fn group_companions(&self, primary: ClipId) -> Vec<ClipId> {
        match self.clip(primary).and_then(|clip| clip.group) {
            Some(group) => self.followers(primary, |clip| clip.group == Some(group)),
            None => vec![primary],
        }
    }

    /// Every clip that moves and selects together with `primary`: the
    /// transitive closure of [`Clip::link`](crate::Clip::link) and
    /// [`Clip::group`](crate::Clip::group), less the clips on locked lanes, in
    /// track/clip order. Trims use [`Self::linked_followers`] instead so trims
    /// stay per-clip (or per linked A/V pair).
    pub fn move_companions(&self, primary: ClipId) -> Vec<ClipId> {
        let Some(seed) = self.clip(primary) else {
            return vec![primary];
        };
        if seed.link.is_none() && seed.group.is_none() {
            return vec![primary];
        }
        let mut links = BTreeSet::new();
        let mut groups = BTreeSet::new();
        if let Some(link) = seed.link {
            links.insert(link);
        }
        if let Some(group) = seed.group {
            groups.insert(group);
        }
        loop {
            let mut grew = false;
            for track in self.timeline().tracks_ordered() {
                for clip in track.clips_ordered() {
                    let in_set = clip.link.is_some_and(|l| links.contains(&l))
                        || clip.group.is_some_and(|g| groups.contains(&g));
                    if in_set {
                        if let Some(l) = clip.link {
                            grew |= links.insert(l);
                        }
                        if let Some(g) = clip.group {
                            grew |= groups.insert(g);
                        }
                    }
                }
            }
            if !grew {
                break;
            }
        }
        self.followers(primary, |clip| {
            clip.link.is_some_and(|l| links.contains(&l))
                || clip.group.is_some_and(|g| groups.contains(&g))
        })
    }

    /// The clips a trim, retime or slip of `primary` changes along with it:
    /// `primary` and every clip linked to it whose lane is not locked, in
    /// track/clip order.
    pub fn linked_followers(&self, primary: ClipId) -> Vec<ClipId> {
        match self.clip(primary).and_then(|clip| clip.link) {
            Some(link) => self.followers(primary, |clip| clip.link == Some(link)),
            None => vec![primary],
        }
    }

    /// `primary` and every clip `member` admits on a lane that is not locked,
    /// in track/clip order. A lock rejects every edit addressed to its lane,
    /// so a companion there stays where it is through an edit elsewhere and
    /// the pair drifts out of sync instead.
    fn followers(&self, primary: ClipId, member: impl Fn(&crate::Clip) -> bool) -> Vec<ClipId> {
        let member = &member;
        self.timeline()
            .tracks_ordered()
            .flat_map(|track| {
                track
                    .clips_ordered()
                    .filter(move |clip| clip.id == primary || (!track.locked && member(*clip)))
            })
            .map(|clip| clip.id)
            .collect()
    }

    /// The assignments that set `enabled` on `clips`.
    pub fn enable_plan(
        &self,
        clips: &[ClipId],
        enabled: bool,
    ) -> Result<Vec<EnableAssignment>, ModelError> {
        if clips.is_empty() {
            return Err(ModelError::ClipSet(ClipSetRefusal::NoClips));
        }
        Ok(self
            .distinct_clips(clips)?
            .into_iter()
            .map(|clip| (clip, enabled))
            .collect())
    }

    /// Apply an enable plan, returning the plan that undoes it, newest first
    /// as [`Self::set_clip_links`] returns it.
    pub fn set_clips_enabled(
        &mut self,
        plan: &[EnableAssignment],
    ) -> Result<Vec<EnableAssignment>, ModelError> {
        for &(clip, _) in plan {
            if self.clip(clip).is_none() {
                return Err(ModelError::UnknownClip(clip));
            }
        }
        let mut inverse = Vec::with_capacity(plan.len());
        for &(clip_id, enabled) in plan {
            let clip = self.timeline_mut().require_clip_mut(clip_id)?;
            inverse.push((clip_id, clip.enabled));
            clip.enabled = enabled;
        }
        inverse.reverse();
        Ok(inverse)
    }
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
