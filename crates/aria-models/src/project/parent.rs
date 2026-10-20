//! Which clip a clip is parented to, and whether it may be.

use super::Project;
use crate::clip::{AnimatedTransform, Clip, ClipTransform, MAX_PARENT_DEPTH, ParentFrame};
use crate::error::ModelError;
use crate::ids::ClipId;
use crate::timeline::Timeline;

impl Project {
    /// The sequence `clip_id` is on: the open one first, then the root, then
    /// every other sequence.
    pub fn timeline_with_clip(&self, clip_id: ClipId) -> Option<&Timeline> {
        if self.timeline().clip(clip_id).is_some() {
            return Some(self.timeline());
        }
        if self.root_timeline().clip(clip_id).is_some() {
            return Some(self.root_timeline());
        }
        self.sequences
            .values()
            .map(|seq| &seq.timeline)
            .find(|timeline| timeline.clip(clip_id).is_some())
    }

    /// The clip `child` is placed in at timeline tick `tick`, which may fall
    /// between frames: its parent, when that clip is on the same sequence and
    /// covers `tick`. A deleted parent, or one moved to another sequence, is
    /// no parent, so the child is placed by its own transform.
    pub fn active_parent(&self, child: &Clip, tick: f64) -> Option<&Clip> {
        let parent = child.parent?;
        let parent = self.timeline_with_clip(child.id)?.clip(parent)?;
        let span = &parent.timeline;
        (span.start.value as f64 <= tick && tick < span.end_tick() as f64).then_some(parent)
    }

    /// The frame `child` is placed in at timeline tick `tick` on a canvas of
    /// `canvas` pixels: its parents' poses composed outermost first, each read
    /// by `pose`. `None` when no parent covers `tick`.
    ///
    /// `pose` is the parent's own transform at `tick` as the caller places
    /// it: the renderer passes its driven, animated transform, an editor its
    /// live one. A chain longer than [`MAX_PARENT_DEPTH`], which only a file
    /// edited by hand can hold, is read through its first that many parents.
    pub fn parent_frame(
        &self,
        child: &Clip,
        tick: f64,
        canvas: [f32; 2],
        mut pose: impl FnMut(&Clip, f64) -> ClipTransform,
    ) -> Option<ParentFrame> {
        let mut chain: Vec<&Clip> = Vec::new();
        let mut current = child;
        while chain.len() < MAX_PARENT_DEPTH {
            let Some(parent) = self.active_parent(current, tick) else {
                break;
            };
            chain.push(parent);
            current = parent;
        }
        chain.iter().rev().fold(None, |outer, parent| {
            let local = pose(parent, tick);
            let world = outer.map_or(local, |frame: ParentFrame| frame.compose(&local));
            Some(ParentFrame::of(&world, canvas))
        })
    }

    /// Whether `parent` may parent `child`: both on one sequence, overlapping
    /// in time, and `parent` neither `child` nor a clip parented to it.
    pub fn check_parent(&self, child: ClipId, parent: ClipId) -> Result<(), ModelError> {
        let timeline = self
            .timeline_with_clip(child)
            .ok_or(ModelError::UnknownClip(child))?;
        let child_clip = timeline.require_clip(child)?;
        let Some(parent_clip) = timeline.clip(parent) else {
            return Err(match self.clip(parent) {
                Some(_) => ModelError::ParentElsewhere { child, parent },
                None => ModelError::UnknownClip(parent),
            });
        };
        let mut ancestor = Some(parent);
        for _ in 0..MAX_PARENT_DEPTH {
            let Some(id) = ancestor else {
                break;
            };
            if id == child {
                return Err(ModelError::ParentCycle(child));
            }
            ancestor = timeline.clip(id).and_then(|clip| clip.parent);
        }
        if ancestor.is_some() {
            return Err(ModelError::ParentTooDeep(child));
        }
        let (a, b) = (&child_clip.timeline, &parent_clip.timeline);
        if a.start.value >= b.end_tick() || b.start.value >= a.end_tick() {
            return Err(ModelError::ParentApart { child, parent });
        }
        Ok(())
    }

    /// Parent `child` to `parent`, or with `None` release it, and give it
    /// `transform` in the same write. Returns the previous parent.
    pub fn set_clip_parent(
        &mut self,
        child: ClipId,
        parent: Option<ClipId>,
        transform: AnimatedTransform,
    ) -> Result<Option<ClipId>, ModelError> {
        if let Some(parent) = parent {
            self.check_parent(child, parent)?;
        }
        transform.validate()?;
        let clip = self.require_clip_mut(child)?;
        clip.transform = transform;
        Ok(std::mem::replace(&mut clip.parent, parent))
    }
}
