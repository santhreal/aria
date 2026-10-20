use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::transition::{Transition, TransitionAlignment};

use super::Project;

impl Project {
    // --- transitions (M4) -------------------------------------------------

    /// Add (or replace) a transition at the junction where `left` abuts the
    /// next clip on its track. The id must name a catalog blend on a picture
    /// lane or an [`AudioTransition`](crate::AudioTransition) on an audio lane,
    /// and `left` must abut a following clip. Uses the default window length.
    pub fn add_transition(&mut self, left: ClipId, transition_id: &str) -> Result<(), ModelError> {
        let picture = crate::transition::transition_spec(transition_id);
        let audio = crate::AudioTransition::from_id(transition_id);
        if picture.is_none() && audio.is_none() {
            return Err(ModelError::InvalidParam(format!(
                "unknown transition '{transition_id}'"
            )));
        }
        let track_id = self
            .timeline()
            .track_of(left)
            .ok_or(ModelError::UnknownClip(left))?;
        let kind = self
            .timeline()
            .track(track_id)
            .ok_or(ModelError::UnknownClip(left))?
            .kind;
        match (kind.transition_media(), picture, audio) {
            (None, _, _) => {
                return Err(ModelError::InvalidParam(format!(
                    "transitions aren't supported on {kind:?} lanes"
                )));
            }
            (Some(crate::TransitionMedia::Picture), None, Some(audio)) => {
                return Err(ModelError::InvalidParam(format!(
                    "{} is an audio transition; it goes on a cut between clips on an audio lane",
                    audio.label()
                )));
            }
            (Some(crate::TransitionMedia::Audio), Some(spec), _) => {
                return Err(ModelError::InvalidParam(format!(
                    "{} is a picture transition; a cut on an audio lane takes {}",
                    spec.label,
                    audio_transition_labels()
                )));
            }
            _ => {}
        }
        let right = self.right_neighbor(track_id, left).ok_or_else(|| {
            ModelError::InvalidParam("clip has no abutting clip to its right".into())
        })?;
        let transition = Transition::new(
            left,
            right,
            transition_id,
            crate::transition::DEFAULT_TRANSITION_TICKS,
        );
        self.timeline_mut()
            .track_mut(track_id)
            .ok_or(ModelError::UnknownClip(left))?
            .upsert_transition(transition);
        Ok(())
    }

    /// Remove the transition at the `left` junction. Errors if none exists.
    pub fn remove_transition(&mut self, left: ClipId) -> Result<(), ModelError> {
        let track_id = self
            .timeline()
            .track_of(left)
            .ok_or(ModelError::UnknownClip(left))?;
        let removed = self
            .timeline_mut()
            .track_mut(track_id)
            .and_then(|t| t.remove_transition(left));
        if removed.is_none() {
            return Err(ModelError::InvalidParam(
                "clip has no transition at its right junction".into(),
            ));
        }
        Ok(())
    }

    /// Largest transition window the clips at `left`'s right junction can hold
    /// under its current alignment (or [`TransitionAlignment::Center`] when no
    /// transition is applied).
    pub fn transition_duration_limit(&self, left: ClipId) -> Result<i64, ModelError> {
        let alignment = self
            .transition_at(left)
            .map(|t| t.alignment)
            .unwrap_or_default();
        self.transition_junction_for(left, alignment)
            .map(|(_, limit)| limit)
    }

    /// The track and duration limit at `left`'s right junction for `alignment`.
    fn transition_junction_for(
        &self,
        left: ClipId,
        alignment: TransitionAlignment,
    ) -> Result<(TrackId, i64), ModelError> {
        let track_id = self
            .timeline()
            .track_of(left)
            .ok_or(ModelError::UnknownClip(left))?;
        let left_clip = self.timeline().require_clip(left)?;
        let right_id = self.right_neighbor(track_id, left).ok_or_else(|| {
            ModelError::InvalidParam("clip has no abutting clip to its right".into())
        })?;
        let right_clip = self.timeline().require_clip(right_id)?;
        let limit = match alignment {
            TransitionAlignment::Center => left_clip
                .timeline
                .duration
                .value
                .min(right_clip.timeline.duration.value),
            TransitionAlignment::Start => right_clip.timeline.duration.value,
            TransitionAlignment::End => left_clip.timeline.duration.value,
        };
        Ok((track_id, limit))
    }

    /// Set the window length (timeline ticks) of an existing transition.
    pub fn set_transition_duration(
        &mut self,
        left: ClipId,
        duration: i64,
    ) -> Result<(), ModelError> {
        if duration <= 0 {
            return Err(ModelError::InvalidRange);
        }
        let alignment = self
            .transition_at(left)
            .map(|t| t.alignment)
            .unwrap_or_default();
        let (track_id, max_duration) = self.transition_junction_for(left, alignment)?;
        if duration > max_duration {
            return Err(ModelError::InvalidParam(format!(
                "transition duration {duration} exceeds maximum adjacent clip duration {max_duration}"
            )));
        }
        let transition = self
            .timeline_mut()
            .track_mut(track_id)
            .and_then(|t| t.transition_at_mut(left))
            .ok_or_else(|| {
                ModelError::InvalidParam("clip has no transition at its right junction".into())
            })?;
        transition.duration = duration;
        Ok(())
    }

    /// Set how the transition window at `left`'s right junction sits relative to the cut.
    pub fn set_transition_alignment(
        &mut self,
        left: ClipId,
        alignment: TransitionAlignment,
    ) -> Result<(), ModelError> {
        let (track_id, max_duration) = self.transition_junction_for(left, alignment)?;
        let duration = self
            .timeline()
            .track(track_id)
            .and_then(|t| t.transition_at(left))
            .ok_or_else(|| {
                ModelError::InvalidParam("clip has no transition at its right junction".into())
            })?
            .duration;
        if duration > max_duration {
            return Err(ModelError::InvalidParam(format!(
                "transition duration {duration} exceeds maximum adjacent clip duration {max_duration}"
            )));
        }
        let transition = self
            .timeline_mut()
            .track_mut(track_id)
            .and_then(|t| t.transition_at_mut(left))
            .ok_or_else(|| {
                ModelError::InvalidParam("clip has no transition at its right junction".into())
            })?;
        transition.alignment = alignment;
        Ok(())
    }

    /// Borrow the transition at `left`'s right junction, if any.
    pub fn transition_at(&self, left: ClipId) -> Option<&Transition> {
        let track_id = self.timeline().track_of(left)?;
        self.timeline().track(track_id)?.transition_at(left)
    }

    /// Mutably borrow the transition at `left`'s right junction.
    pub fn transition_at_mut(&mut self, left: ClipId) -> Result<&mut Transition, ModelError> {
        let track_id = self
            .timeline()
            .track_of(left)
            .ok_or(ModelError::UnknownClip(left))?;
        self.timeline_mut()
            .track_mut(track_id)
            .and_then(|t| t.transition_at_mut(left))
            .ok_or_else(|| {
                ModelError::InvalidParam("clip has no transition at its right junction".into())
            })
    }

    /// Replace the transition at `left`'s right junction after validating its
    /// junction bounds and parameter state.
    pub fn set_transition_state(
        &mut self,
        left: ClipId,
        mut transition: Transition,
    ) -> Result<(), ModelError> {
        transition.validate()?;
        let (track_id, max_duration) = self.transition_junction_for(left, transition.alignment)?;
        let right = self.right_neighbor(track_id, left).ok_or_else(|| {
            ModelError::InvalidParam("clip has no abutting clip to its right".into())
        })?;
        transition.left = left;
        transition.right = right;
        if transition.duration > max_duration {
            return Err(ModelError::InvalidParam(format!(
                "transition duration {} exceeds maximum adjacent clip duration {max_duration}",
                transition.duration
            )));
        }
        let existing = self
            .timeline_mut()
            .track_mut(track_id)
            .and_then(|t| t.transition_at_mut(left))
            .ok_or_else(|| {
                ModelError::InvalidParam("clip has no transition at its right junction".into())
            })?;
        *existing = transition;
        Ok(())
    }

    /// Set one scalar/angle/int transition parameter at `left`'s right junction.
    pub fn set_transition_param(
        &mut self,
        left: ClipId,
        param: usize,
        value: f32,
    ) -> Result<(), ModelError> {
        self.transition_at_mut(left)?
            .set_param_constant(param, value)
    }
    /// The clip on `track` whose start abuts the end of `left`, if any.
    fn right_neighbor(&self, track_id: TrackId, left: ClipId) -> Option<ClipId> {
        let track = self.timeline().track(track_id)?;
        let left_end = track.clip(left)?.timeline.end_tick();
        track
            .clips()
            .find(|c| c.id != left && c.timeline.start.value == left_end)
            .map(|c| c.id)
    }

    /// Whether any track carries a transition (cheap guard for prune).
    pub fn has_transitions(&self) -> bool {
        self.timeline()
            .tracks_ordered()
            .any(|t| !t.transitions().is_empty())
    }

    /// Snapshot every track's transitions (for undo of structural edits that
    /// prune dead junctions).
    pub fn transitions_snapshot(&self) -> Vec<(TrackId, Vec<Transition>)> {
        self.timeline()
            .tracks_ordered()
            .map(|t| (t.id, t.transitions().to_vec()))
            .collect()
    }

    /// Restore a [`Self::transitions_snapshot`] across tracks.
    pub fn restore_transitions(&mut self, snapshot: Vec<(TrackId, Vec<Transition>)>) {
        for (track_id, transitions) in snapshot {
            if let Some(track) = self.timeline_mut().track_mut(track_id) {
                track.set_transitions(transitions);
            }
        }
    }

    /// Drop transitions whose junction no longer abuts, across all tracks.
    /// Returns whether anything was pruned.
    pub fn prune_dead_transitions(&mut self) -> bool {
        let track_ids: Vec<TrackId> = self.timeline().tracks_ordered().map(|t| t.id).collect();
        let mut pruned = false;
        for id in track_ids {
            if let Some(track) = self.timeline_mut().track_mut(id) {
                pruned |= track.prune_dead_transitions();
            }
        }
        pruned
    }
}

/// Every audio transition's name, as one phrase: `A, B or C`.
fn audio_transition_labels() -> String {
    let labels: Vec<&str> = crate::AudioTransition::ALL
        .iter()
        .map(|transition| transition.label())
        .collect();
    match labels.split_last() {
        Some((last, [])) => (*last).to_string(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}
