//! Audio insert chains as the inspector and the mixer draw them, and the edits
//! they make.
//!
//! A chain is addressed by [`AudioInsertTarget`]: the selected clip's in the
//! inspector's Audio block, a lane's or the master's in the mixer. Each row is
//! projected from the model with the catalog entry it names, so a plugin's
//! effect draws its own parameters, and an insert whose effect is not
//! installed draws as such with its stored values kept.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    AudioEffectInstance, AudioEffectSpec, AudioInsertTarget, Easing, Param, Timeline, TrackId,
};

use super::AppState;
use crate::types::{UiInsert, UiInsertParam, UiKeyChoice};

/// The rows of `chain`, in signal order.
pub fn rows(chain: &[AudioEffectInstance]) -> Vec<UiInsert> {
    chain.iter().map(row).collect()
}

fn row(instance: &AudioEffectInstance) -> UiInsert {
    let Some(spec) = instance.spec() else {
        return UiInsert {
            effect_id: instance.effect_id.clone().into(),
            label: format!("{} (not installed)", instance.effect_id).into(),
            installed: false,
            bypass: instance.bypass,
            params: Vec::new(),
            reads_key: false,
            key: instance.sidechain.map(TrackId::raw),
        };
    };
    UiInsert {
        effect_id: instance.effect_id.clone().into(),
        label: spec.label.clone().into(),
        installed: true,
        bypass: instance.bypass,
        params: params(instance, &spec),
        reads_key: spec.sidechain,
        key: instance.sidechain.map(TrackId::raw),
    }
}

fn params(instance: &AudioEffectInstance, spec: &AudioEffectSpec) -> Vec<UiInsertParam> {
    spec.params
        .iter()
        .enumerate()
        .map(|(slot, pspec)| {
            let curve = instance
                .param_curve(spec, slot)
                .unwrap_or(Param::Constant(pspec.default));
            UiInsertParam {
                label: pspec.label.clone().into(),
                value: curve.sample(0),
                min: pspec.min,
                max: pspec.max,
                automated: curve.is_animated(),
            }
        })
        .collect()
}

/// Every lane that may key an insert on the chain at `holder`, in lane order.
pub fn key_choices(timeline: &Timeline, holder: AudioInsertTarget) -> Vec<UiKeyChoice> {
    timeline
        .tracks_ordered()
        .filter(|lane| timeline.sidechain_refusal(lane.id, holder).is_ok())
        .map(|lane| UiKeyChoice {
            track: lane.id.raw(),
            name: lane.name.clone().into(),
        })
        .collect()
}

impl AppState {
    /// The chain at `target` as last projected, in signal order; empty when the
    /// target is gone. Read from the projection, so it takes no lock.
    pub fn audio_insert_rows(&self, target: AudioInsertTarget) -> &[UiInsert] {
        let rows = match target {
            AudioInsertTarget::Clip(clip) => self
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .find(|drawn| drawn.id == clip.raw())
                .map(|drawn| drawn.audio_inserts.as_slice()),
            AudioInsertTarget::Track(track) => self
                .facts
                .mixer
                .strips
                .as_ref()
                .ok()
                .and_then(|strips| strips.iter().find(|strip| strip.track == track.raw()))
                .map(|strip| strip.inserts.as_slice()),
            AudioInsertTarget::Master => Some(self.facts.mixer.master.inserts.as_slice()),
        };
        rows.unwrap_or_default()
    }

    /// How many inserts the chain at `target` holds; zero when it is gone.
    pub fn audio_insert_count(&self, target: AudioInsertTarget) -> usize {
        self.audio_insert_rows(target).len()
    }

    /// Append `effect_id` to the chain at `target` and close the Add list.
    pub fn add_audio_insert(
        &mut self,
        target: AudioInsertTarget,
        effect_id: String,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::AddAudioEffect {
            target,
            effect_id,
        }))?;
        self.insert_picker = None;
        Ok(())
    }

    /// Effect > an audio category > `effect_id`: append it to the chain of
    /// every selected clip with sound, as one undo step.
    pub fn add_audio_effect_to_selection(&mut self, effect_id: &str) -> Result<(), EngineError> {
        let clips = self.sounding_selection();
        if clips.is_empty() {
            self.status_message = Some("Select a clip with sound.".into());
            return Ok(());
        }
        self.one_step(|this| {
            for &clip in &clips {
                this.apply_command(Command::Edit(EditCommand::AddAudioEffect {
                    target: AudioInsertTarget::Clip(clip),
                    effect_id: effect_id.to_owned(),
                }))?;
            }
            Ok(())
        })
    }

    /// Take the insert at `index` out of its chain. Its open parameters close;
    /// an open insert after it follows it down one place.
    pub fn remove_audio_insert(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::RemoveAudioEffect {
            target,
            index,
        }))?;
        self.open_insert = match self.open_insert {
            Some((open, at)) if open == target && at == index => None,
            Some((open, at)) if open == target && at > index => Some((open, at - 1)),
            other => other,
        };
        Ok(())
    }

    /// Move an insert within its chain. A move off either end, or onto
    /// itself, is inert. An open editor follows the insert it shows.
    pub fn move_audio_insert(
        &mut self,
        target: AudioInsertTarget,
        from: usize,
        to: usize,
    ) -> Result<(), EngineError> {
        let len = self.audio_insert_count(target);
        if from >= len || to >= len || from == to {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::MoveAudioEffect {
            target,
            from_index: from,
            to_index: to,
        }))?;
        self.open_insert = self.open_insert.map(|(open, at)| {
            if open != target {
                (open, at)
            } else if at == from {
                (open, to)
            } else if from < at && at <= to {
                (open, at - 1)
            } else if to <= at && at < from {
                (open, at + 1)
            } else {
                (open, at)
            }
        });
        Ok(())
    }

    /// Bypass the insert at `index`, or bring it back.
    pub fn toggle_audio_insert_bypass(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
    ) -> Result<(), EngineError> {
        let Some(bypass) = self
            .audio_insert_rows(target)
            .get(index)
            .map(|row| !row.bypass)
        else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetAudioEffectBypass {
            target,
            index,
            bypass,
        }))?;
        Ok(())
    }

    /// Step parameter `slot` of the insert at `index` by `by`, held in its
    /// range. A constant stays constant. A keyframed parameter gets a keyframe
    /// at the playhead, so stepping it never erases its automation.
    pub fn nudge_audio_insert_param(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
        slot: usize,
        by: f32,
    ) -> Result<(), EngineError> {
        let playhead = self.playhead.value;
        let value = {
            let engine = self.engine.lock();
            let project = engine.project();
            // A clip chain's keyframes are clip-relative; a lane's and the
            // master's are sequence ticks.
            let tick = match target.clip() {
                Some(clip) => project
                    .timeline()
                    .clip(clip)
                    .map_or(0, |clip| clip.animation_tick(playhead)),
                None => playhead,
            };
            let Some(instance) = project
                .audio_inserts(target)
                .ok()
                .and_then(|chain| chain.get(index))
            else {
                return Ok(());
            };
            let Some(spec) = instance.spec() else {
                return Ok(());
            };
            let (Some(pspec), Some(curve)) =
                (spec.params.get(slot), instance.param_curve(&spec, slot))
            else {
                return Ok(());
            };
            let stepped = |value: f32| (value + by).clamp(pspec.min, pspec.max);
            match curve {
                Param::Constant(value) => Param::Constant(stepped(value)),
                mut keyed @ Param::Keyframed { .. } => {
                    let at = stepped(keyed.sample(tick));
                    keyed.set_keyframe(tick, at, Easing::Linear);
                    keyed
                }
            }
        };
        self.apply_command(Command::Edit(EditCommand::SetAudioEffectParam {
            target,
            index,
            param: slot,
            value,
        }))?;
        Ok(())
    }

    /// Key the insert at `index` from lane `key`, or stop keying it.
    pub fn set_audio_insert_key(
        &mut self,
        target: AudioInsertTarget,
        index: usize,
        key: Option<TrackId>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetAudioEffectSidechain {
            target,
            index,
            key,
        }))?;
        Ok(())
    }

    /// Show or hide the Add list of the chain at `target`.
    pub fn toggle_insert_picker(&mut self, target: AudioInsertTarget) {
        self.insert_picker = if self.insert_picker == Some(target) {
            None
        } else {
            Some(target)
        };
    }

    /// Show the parameters of the insert at `index`, or hide them when they
    /// already show.
    pub fn toggle_open_insert(&mut self, target: AudioInsertTarget, index: usize) {
        self.open_insert = if self.open_insert == Some((target, index)) {
            None
        } else {
            Some((target, index))
        };
    }
}
