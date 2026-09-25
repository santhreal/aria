//! Transition commands: a timed blend at the cut between two abutting clips.
//!
//! A transition is keyed on the clip on the outgoing (left) side of the cut, so
//! every setter here addresses the selected clip's *right* junction. The engine
//! refuses a junction that does not abut, or a lane that cannot carry one, so the
//! projected `can_transition` flag gates the surface instead of a refused command.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, TransitionSpec, transition_catalog};

use super::AppState;
use super::decline::Decline;

/// The smallest and largest transition window the UI will send, in ticks. The
/// model clamps to at least one tick; the upper bound keeps a drag from opening a
/// window longer than either side of the cut can plausibly cover.
pub const MIN_TRANSITION_TICKS: i64 = 2;
pub const MAX_TRANSITION_TICKS: i64 = 600;

/// The step one nudge of the duration control takes, in ticks.
pub const TRANSITION_STEP_TICKS: i64 = 6;

/// Every transition the model can render, for a catalog list.
pub fn transition_choices() -> &'static [TransitionSpec] {
    transition_catalog()
}

/// The window a wedge edge drag implies.
///
/// The wedge is drawn centred on the junction, so moving one edge outward by
/// `delta_ticks` widens the window by twice that; the left edge reads the opposite
/// sign. A drag past the bounds stalls at them rather than being refused.
pub fn transition_ticks_after_drag(current: i64, delta_ticks: i64, right_edge: bool) -> i64 {
    // `-delta_ticks` panics on `i64::MIN`; a pointer delta never reaches it, but a
    // wedge drag has no business being the one arithmetic in the file that can.
    let outward = if right_edge {
        delta_ticks
    } else {
        delta_ticks.saturating_neg()
    };
    current
        .saturating_add(outward.saturating_mul(2))
        .clamp(MIN_TRANSITION_TICKS, MAX_TRANSITION_TICKS)
}

impl AppState {
    /// Whether the clip's right junction can carry a transition.
    pub fn clip_can_transition(&self, clip: ClipId) -> bool {
        self.ui_clip(clip)
            .map(|clip| clip.can_transition)
            .unwrap_or(false)
    }

    /// The catalog id of the transition at this clip's right junction.
    pub fn clip_transition_id(&self, clip: ClipId) -> Option<String> {
        self.ui_clip(clip)?
            .transition
            .as_ref()
            .map(|transition| transition.id.to_string())
    }

    /// The catalog id of the transition at the selected clip's right junction.
    pub fn selected_transition_id(&self) -> Option<String> {
        self.clip_transition_id(self.selected_clip()?)
    }

    /// The window length of the transition at this clip's junction, in ticks.
    pub fn clip_transition_ticks(&self, clip: ClipId) -> Option<i64> {
        self.ui_clip(clip)?
            .transition
            .as_ref()
            .map(|transition| transition.duration_ticks)
    }

    /// The window length of the transition at the selected junction, in ticks.
    pub fn selected_transition_ticks(&self) -> Option<i64> {
        self.clip_transition_ticks(self.selected_clip()?)
    }

    /// Apply (or replace) a transition at this clip's right junction.
    ///
    /// Declines an unknown id and a junction that cannot carry one: a stale
    /// surface must not raise an error the operator did not cause, and it must
    /// not swallow the press either, so each case names itself in the status
    /// line.
    pub fn set_clip_transition(
        &mut self,
        clip: ClipId,
        transition_id: &str,
    ) -> Result<(), EngineError> {
        if !self.clip_can_transition(clip) {
            self.decline(Decline::NoAbuttingCut);
            return Ok(());
        }
        if aria_models::transition_spec(transition_id).is_none() {
            self.decline(Decline::UnknownTransition);
            return Ok(());
        }
        // The engine upserts, so a second id at the same junction replaces the
        // first rather than stacking.
        self.apply_command(Command::Edit(EditCommand::AddTransition {
            clip,
            transition_id: transition_id.to_string(),
        }))?;
        Ok(())
    }

    pub fn set_selected_transition(&mut self, transition_id: &str) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.set_clip_transition(clip, transition_id)
    }

    /// Remove the transition at this clip's right junction, or say there is
    /// none.
    pub fn clear_clip_transition(&mut self, clip: ClipId) -> Result<(), EngineError> {
        if self.clip_transition_ticks(clip).is_none() {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RemoveTransition { clip }))?;
        Ok(())
    }

    pub fn clear_selected_transition(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.clear_clip_transition(clip)
    }

    /// Set the transition window length, clamped to the UI bounds. Says so when
    /// the junction carries no transition; stays silent when the length would
    /// not change, because a drag that ends where it started is not a press that
    /// found nothing, and it costs no undo entry either.
    pub fn set_clip_transition_ticks(
        &mut self,
        clip: ClipId,
        ticks: i64,
    ) -> Result<(), EngineError> {
        let Some(current) = self.clip_transition_ticks(clip) else {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        };
        let adjacent_limit = self
            .engine
            .lock()
            .project()
            .transition_duration_limit(clip)
            .map_err(EngineError::Model)?;
        let upper = MAX_TRANSITION_TICKS.min(adjacent_limit);
        let duration = ticks.clamp(MIN_TRANSITION_TICKS.min(upper), upper);
        if duration == current {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetTransition { clip, duration }))?;
        Ok(())
    }

    pub fn set_selected_transition_ticks(&mut self, ticks: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.set_clip_transition_ticks(clip, ticks)
    }

    /// Widen or narrow the transition window by `delta` ticks.
    pub fn nudge_selected_transition_ticks(&mut self, delta: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        let Some(current) = self.clip_transition_ticks(clip) else {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        };
        self.set_clip_transition_ticks(clip, current + delta)
    }
}
