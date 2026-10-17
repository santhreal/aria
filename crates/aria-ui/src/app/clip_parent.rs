//! Clip parenting from the editor.
//!
//! A child clip is drawn in its parent's frame while the parent covers it in
//! time. The inspector's Parent row picks a parent from the clips the model
//! accepts, its pick whip takes the next clip pressed on the timeline, and the
//! clip context menu releases one. Each ends in `SetClipParent` at the
//! playhead, which keeps the clip where it is drawn there or, with Shift held,
//! snaps it onto the parent's anchor.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, ClipId, ParentFrame, ParentKeep, Project, RationalTime};

use super::AppState;

/// The Parent row's choices for one clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentChoices {
    /// The clip's parent now.
    pub current: Option<ClipId>,
    /// Every clip the model accepts as its parent, in lane order and then by
    /// start: on the clip's sequence, overlapping it, and not parented to it.
    pub eligible: Arc<[ClipId]>,
}

impl ParentChoices {
    /// `clip`'s choices in `project`.
    pub fn of(project: &Project, clip: &Clip) -> Self {
        let eligible = project
            .timeline_with_clip(clip.id)
            .map(|timeline| {
                timeline
                    .tracks_ordered()
                    .flat_map(|track| track.clips())
                    .filter(|other| {
                        other.id != clip.id && project.check_parent(clip.id, other.id).is_ok()
                    })
                    .map(|other| other.id)
                    .collect()
            })
            .unwrap_or_else(|| Arc::from([]));
        Self {
            current: clip.parent,
            eligible,
        }
    }
}

/// Which way a parent is taken: Shift snaps onto the parent's anchor.
pub fn keep_for(snap: bool) -> ParentKeep {
    if snap {
        ParentKeep::Snap
    } else {
        ParentKeep::Place
    }
}

impl AppState {
    /// Parent `clip` to `parent` at the playhead, or with `None` release it.
    pub fn set_clip_parent(
        &mut self,
        clip: ClipId,
        parent: Option<ClipId>,
        keep: ParentKeep,
    ) -> Result<(), EngineError> {
        let rate = self.engine.lock().project().timeline().frame_rate;
        self.apply_command(Command::Edit(EditCommand::SetClipParent {
            clip,
            parent,
            at: RationalTime::new(self.playhead.value, rate),
            keep,
        }))?;
        Ok(())
    }

    /// The Parent row's pick: parent the selected clip, or with `None`
    /// release it. Inert with nothing selected.
    pub fn set_selected_parent(
        &mut self,
        parent: Option<ClipId>,
        keep: ParentKeep,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.set_clip_parent(clip, parent, keep)
    }

    /// The selected clip's parent and the clips it may be parented to, from
    /// the projection.
    pub fn selected_parent_choices(&self) -> Option<&ParentChoices> {
        self.selected.as_ref().map(|detail| &detail.parents)
    }

    /// The frame the selected clip's parents lend it at the playhead, from the
    /// projection. `None` while no parent covers the playhead.
    pub fn selected_parent_frame(&self) -> Option<ParentFrame> {
        self.selected.as_ref()?.frame
    }

    /// Arm the Parent row's pick whip for the selected clip, or put an armed
    /// one down.
    pub fn toggle_parent_whip(&mut self) {
        self.parent_whip = match self.parent_whip() {
            Some(_) => None,
            None => self.selected_clip(),
        };
        if self.parent_whip.is_some() {
            self.status_message = Some(
                "Press a clip on the timeline to parent the selected clip to it; \
                 hold Shift to snap onto its anchor"
                    .into(),
            );
        }
    }

    /// The clip a waiting pick whip parents. A whip armed for a clip that is
    /// no longer selected is down.
    pub fn parent_whip(&self) -> Option<ClipId> {
        self.parent_whip
            .filter(|clip| self.selected_clip() == Some(*clip))
    }

    /// The pick whip lands on `target`: the clip it was armed for is parented
    /// to it, snapped onto its anchor with `snap`. Landing on that clip itself
    /// puts the whip down.
    pub fn land_parent_whip(&mut self, target: ClipId, snap: bool) -> Result<(), EngineError> {
        let Some(child) = self.parent_whip() else {
            return Ok(());
        };
        self.parent_whip = None;
        if child == target {
            return Ok(());
        }
        self.set_clip_parent(child, Some(target), keep_for(snap))
    }
}

#[cfg(test)]
#[path = "clip_parent_tests.rs"]
mod tests;
