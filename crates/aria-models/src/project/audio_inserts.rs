//! Insert chains on the open timeline: a clip's, a lane's, and the master's.
//!
//! Every edit addresses a chain by [`AudioInsertTarget`] and an insert by its
//! index in signal order. A chain on a clip or lane that carries no sound is
//! refused, so an insert never sits where the mixer cannot reach it.

use crate::audio_effects::{AudioEffectInstance, AudioInsertTarget, audio_effect_spec};
use crate::error::ModelError;
use crate::ids::TrackId;
use crate::param::Param;

use super::Project;

impl Project {
    /// The chain at `target` on the open timeline, in signal order.
    pub fn audio_inserts(
        &self,
        target: AudioInsertTarget,
    ) -> Result<&[AudioEffectInstance], ModelError> {
        let timeline = self.timeline();
        match target {
            AudioInsertTarget::Clip(clip) => timeline
                .clip(clip)
                .map(|clip| clip.audio_effects.as_slice())
                .ok_or(ModelError::UnknownClip(clip)),
            AudioInsertTarget::Track(track) => timeline
                .track(track)
                .map(|lane| lane.audio_effects.as_slice())
                .ok_or(ModelError::UnknownTrack(track)),
            AudioInsertTarget::Master => Ok(timeline.master_audio_effects()),
        }
    }

    /// The chain at `target`, mutably, once the target is known to carry
    /// sound.
    fn audio_inserts_mut(
        &mut self,
        target: AudioInsertTarget,
    ) -> Result<&mut Vec<AudioEffectInstance>, ModelError> {
        self.insert_target_refusal(target)?;
        let timeline = self.timeline_mut();
        match target {
            AudioInsertTarget::Clip(clip) => timeline
                .clip_mut(clip)
                .map(|clip| &mut clip.audio_effects)
                .ok_or(ModelError::UnknownClip(clip)),
            AudioInsertTarget::Track(track) => timeline
                .track_mut(track)
                .map(|lane| &mut lane.audio_effects)
                .ok_or(ModelError::UnknownTrack(track)),
            AudioInsertTarget::Master => Ok(timeline.master_audio_effects_mut()),
        }
    }

    /// Why an insert chain at `target` would be refused, if it would be. A
    /// clip carries sound when it sits on a lane in the mix, is not generated,
    /// and its media (if it names one) has an audio stream; a lane when its
    /// kind is in the mix. The master always does. Public so a surface offers
    /// a chain only where the setters accept one.
    pub fn insert_target_refusal(&self, target: AudioInsertTarget) -> Result<(), ModelError> {
        let timeline = self.timeline();
        match target {
            AudioInsertTarget::Clip(clip_id) => {
                let clip = timeline
                    .clip(clip_id)
                    .ok_or(ModelError::UnknownClip(clip_id))?;
                let lane = timeline
                    .track_of(clip_id)
                    .ok_or(ModelError::UnknownClip(clip_id))?;
                let silent_media = clip
                    .media()
                    .and_then(|media| self.media(media))
                    .is_some_and(|media| !media.has_audio);
                if clip.is_generated() || silent_media || !timeline.lane_mixes_audio(lane)? {
                    return Err(ModelError::ClipCarriesNoSound(clip_id));
                }
                Ok(())
            }
            AudioInsertTarget::Track(track) => {
                if !timeline.lane_mixes_audio(track)? {
                    return Err(ModelError::LaneCarriesNoSound(track));
                }
                Ok(())
            }
            AudioInsertTarget::Master => Ok(()),
        }
    }

    /// Append a registered audio effect to the chain at `target`, with every
    /// parameter at its default. Returns the new insert's index. A lane or the
    /// master holds at most [`STRIP_INSERTS`](crate::STRIP_INSERTS).
    pub fn add_audio_effect(
        &mut self,
        target: AudioInsertTarget,
        effect_id: &str,
    ) -> Result<usize, ModelError> {
        if audio_effect_spec(effect_id).is_none() {
            return Err(ModelError::UnknownAudioEffect(effect_id.to_owned()));
        }
        let chain = self.audio_inserts_mut(target)?;
        if let Some(limit) = target.limit().filter(|limit| chain.len() >= *limit) {
            return Err(ModelError::InsertsFull(limit));
        }
        chain.push(AudioEffectInstance::new(effect_id));
        Ok(chain.len() - 1)
    }

    /// Take the insert at `index` out of the chain at `target`, returning it.
    pub fn remove_audio_effect(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
    ) -> Result<AudioEffectInstance, ModelError> {
        let chain = self.audio_inserts_mut(target)?;
        check_index(chain, index)?;
        Ok(chain.remove(index))
    }

    /// Move one insert within its chain. Both indices address the chain before
    /// the move; `to_index` is the insert's index afterwards.
    pub fn move_audio_effect(
        &mut self,
        target: AudioInsertTarget,
        from_index: usize,
        to_index: usize,
    ) -> Result<(), ModelError> {
        let chain = self.audio_inserts_mut(target)?;
        check_index(chain, from_index)?;
        check_index(chain, to_index)?;
        let insert = chain.remove(from_index);
        chain.insert(to_index, insert);
        Ok(())
    }

    /// Set parameter slot `param` of the insert at `index`, constant or
    /// keyframed. Keyframes on a clip insert are clip-relative ticks; on a lane
    /// or master insert they are sequence ticks. Refused for an effect that is
    /// not registered, a slot it does not have, and a value outside the slot's
    /// range.
    pub fn set_audio_effect_param(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
        param: usize,
        value: Param<f32>,
    ) -> Result<(), ModelError> {
        let chain = self.audio_inserts_mut(target)?;
        check_index(chain, index)?;
        chain[index].set_param(param, value)
    }

    /// Bypass or restore the insert at `index`.
    pub fn set_audio_effect_bypass(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
        bypass: bool,
    ) -> Result<(), ModelError> {
        let chain = self.audio_inserts_mut(target)?;
        check_index(chain, index)?;
        chain[index].bypass = bypass;
        Ok(())
    }

    /// Key the insert at `index` from lane `key`, or stop keying it. Refused
    /// as [`Timeline::sidechain_refusal`](crate::Timeline::sidechain_refusal)
    /// refuses, and for an effect that reads no key.
    pub fn set_audio_effect_sidechain(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
        key: Option<TrackId>,
    ) -> Result<(), ModelError> {
        if let Some(key) = key {
            self.timeline().sidechain_refusal(key, target)?;
        }
        let chain = self.audio_inserts_mut(target)?;
        check_index(chain, index)?;
        chain[index].set_sidechain(key)
    }

    /// Replace the whole chain at `target`, as an undo restores it. Instances
    /// of effects that are not registered are kept. Refused past the target's
    /// insert limit, for a malformed keyframe curve, and for a sidechain key on
    /// a clip insert.
    pub fn set_audio_inserts(
        &mut self,
        target: AudioInsertTarget,
        inserts: Vec<AudioEffectInstance>,
    ) -> Result<(), ModelError> {
        if let Some(limit) = target.limit().filter(|limit| inserts.len() > *limit) {
            return Err(ModelError::InsertsFull(limit));
        }
        for insert in &inserts {
            for curve in insert.params.values() {
                curve.validate_shape()?;
            }
            if target.clip().is_some() && insert.sidechain.is_some() {
                return Err(ModelError::InvalidParam(
                    "a clip insert takes no sidechain key".into(),
                ));
            }
        }
        *self.audio_inserts_mut(target)? = inserts;
        Ok(())
    }
}

fn check_index(chain: &[AudioEffectInstance], index: usize) -> Result<(), ModelError> {
    if index >= chain.len() {
        return Err(ModelError::UnknownInsert {
            index,
            len: chain.len(),
        });
    }
    Ok(())
}
