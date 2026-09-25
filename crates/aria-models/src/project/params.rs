//! Parameter commands: keyframes, constants and easing presets on any
//! animatable clip property, plus the effect chain.
//!
//! Every route here is one walk over the [`ClipParam`] registry
//! (`crate::clip::param_axis`). The registry row declares which lane the
//! parameter means anything on, how to validate a candidate value, and which
//! sub-axis dispatcher performs the write, so this file holds the
//! *placement* rules only: which clip, which lane, and whether the position
//! falls inside the clip. Nothing here matches on a `ClipParam` variant, which
//! is why a new property needs no arm in it.

use crate::clip::{
    AnimatedTransform, ClipParam, ClipTransform, ParamScope, ParamValue, effect_mut,
};
use crate::effects::{EffectInstance, EffectPreset};
use crate::error::ModelError;
use crate::ids::ClipId;
use crate::param::{Easing, PiecewiseEasingPreset, SpatialTangents};
use crate::time::{RationalTime, check_same_rate};

use super::Project;

impl Project {
    /// Set a clip's spatial transform (preview move/scale/rotate, inspector
    /// numerics). Errors if the clip is unknown, sits on an audio track
    /// (nothing to place), or the transform is invalid (non-finite, scale
    /// ≤ 0, opacity outside 0..=1).
    ///
    /// `at` composes the edit with animation CapCut-style: `Some(timeline
    /// tick)` writes a keyframe at that position on properties that already
    /// have keyframes (constants stay constant); `None` flattens every
    /// property to a constant, dropping keyframes. Never-animated clips
    /// behave identically either way.
    pub fn set_transform(
        &mut self,
        clip_id: ClipId,
        transform: ClipTransform,
        at: Option<RationalTime>,
    ) -> Result<(), ModelError> {
        transform.validate()?;
        self.check_visual_target(clip_id)?;
        if let Some(at) = at {
            check_same_rate(at.rate, self.timeline().frame_rate)?;
        }
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        match at {
            Some(at) => {
                let tick = clip.param_tick(at.value);
                clip.transform.compose_at(transform, tick);
            }
            None => clip.transform.set_constant(transform),
        }
        Ok(())
    }

    /// Replace a visual clip's complete animated transform.
    ///
    /// Unlike [`Self::set_transform`], this preserves every source keyframe.
    /// Clipboard attribute transfer uses it because a pasted Motion effect is a
    /// curve, not only its value at one frame.
    pub fn set_animated_transform(
        &mut self,
        clip_id: ClipId,
        transform: AnimatedTransform,
    ) -> Result<(), ModelError> {
        transform.validate()?;
        self.check_visual_target(clip_id)?;
        self.timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?
            .transform = transform;
        Ok(())
    }

    /// The target rule the parameter's registry row declares.
    ///
    /// A visual property needs a clip on a lane that composites pixels; an
    /// audio envelope needs a media-backed clip. Read off
    /// [`ClipParam::scope`], so a parameter gets the right precondition by
    /// declaring its lane rather than by being named in an `if` chain here.
    fn check_param_target(&self, clip_id: ClipId, param: ClipParam) -> Result<(), ModelError> {
        match param.scope() {
            ParamScope::Visual => self.check_visual_target(clip_id),
            ParamScope::Audio => self.check_audio_target(clip_id),
        }
    }

    /// The clip exists on a visual track. Audio lanes have no canvas
    /// placement to animate.
    fn check_visual_target(&self, clip_id: ClipId) -> Result<(), ModelError> {
        let track_id = self
            .timeline()
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let kind = self
            .timeline()
            .track(track_id)
            .ok_or(ModelError::UnknownTrack(track_id))?
            .kind;
        if !kind.is_visual() {
            return Err(ModelError::IncompatibleTrackKind {
                track: track_id,
                kind,
            });
        }
        Ok(())
    }

    /// The clip exists and is media-backed (generators have nothing to hear).
    /// Mirrors [`Self::set_clip_audio`]'s target rule — audio params ride any
    /// media clip (including video with sound), since linkage lands the
    /// audible half on an audio lane.
    fn check_audio_target(&self, clip_id: ClipId) -> Result<(), ModelError> {
        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        if clip.is_generated() {
            return Err(ModelError::InvalidParam(
                "audio parameters require a media-backed clip".into(),
            ));
        }
        Ok(())
    }

    /// Convert an absolute timeline position to a clip-relative animation
    /// tick, rejecting positions outside the clip (a keyframe must sit on
    /// the clip it animates).
    fn keyframe_tick(&self, clip_id: ClipId, at: RationalTime) -> Result<i64, ModelError> {
        check_same_rate(at.rate, self.timeline().frame_rate)?;
        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        if !clip.timeline.contains(at)? {
            return Err(ModelError::InvalidParam(format!(
                "keyframe position {} is outside clip {clip_id}",
                at.value
            )));
        }
        Ok(clip.param_tick(at.value))
    }

    /// Insert or replace a keyframe on one animatable clip property. `at` is
    /// an absolute timeline position and must fall inside the clip.
    ///
    /// `tangents` shapes a cubic-bezier motion path and is accepted only on
    /// [`ClipParam::Position`] (canvas-fraction handles). Other params reject
    /// a non-`None` value with "spatial tangents are only supported on
    /// position". `None` clears/keeps straight-line motion on position.
    pub fn set_param_keyframe(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
        value: ParamValue,
        easing: Easing,
        tangents: Option<SpatialTangents>,
    ) -> Result<(), ModelError> {
        if tangents.is_some() && param != ClipParam::Position {
            return Err(ModelError::InvalidParam(
                "spatial tangents are only supported on position".into(),
            ));
        }
        if let Some(t) = tangents {
            t.validate()?;
        }
        // The registry validates again on the way in; running it here first is
        // what fixes the precedence: a malformed value is reported as a bad
        // value, not as a bad target, whichever clip the command names.
        easing.validate()?;
        param.validate_value(value)?;
        self.check_param_target(clip_id, param)?;
        let tick = self.keyframe_tick(clip_id, at)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        param.set_keyframe(clip, tick, value, easing)?;
        if param == ClipParam::Position {
            // Always write the tangents slot so a None command clears a
            // previous curve (EditCommand redo / AI wire).
            clip.transform
                .position
                .set_keyframe_tangents(tick, tangents)?;
        }
        Ok(())
    }

    /// Set or clear spatial bezier tangents on a position keyframe at `at`.
    /// Rejected for every param other than [`ClipParam::Position`].
    pub fn set_param_keyframe_tangents(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
        tangents: Option<SpatialTangents>,
    ) -> Result<(), ModelError> {
        if param != ClipParam::Position {
            return Err(ModelError::InvalidParam(
                "spatial tangents are only supported on position".into(),
            ));
        }
        if let Some(t) = tangents {
            t.validate()?;
        }
        self.check_visual_target(clip_id)?;
        let tick = self.keyframe_tick(clip_id, at)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        clip.transform
            .position
            .set_keyframe_tangents(tick, tangents)
    }

    /// Remove the keyframe at exactly `at` (absolute timeline position) on
    /// one property. Errors when no keyframe sits there.
    pub fn remove_param_keyframe(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
    ) -> Result<(), ModelError> {
        self.check_param_target(clip_id, param)?;
        let tick = self.keyframe_tick(clip_id, at)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        param.remove_keyframe(clip, tick)
    }

    /// Replace one animatable property with a constant, dropping keyframes.
    pub fn set_param_constant(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        value: ParamValue,
    ) -> Result<(), ModelError> {
        param.validate_value(value)?;
        self.check_param_target(clip_id, param)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        param.set_constant(clip, value)
    }

    /// Expand the outgoing keyframe segment at `at` with a piecewise easing
    /// preset (bounce / elastic / back). Scalar and vec2 params only — colour
    /// and crop params are rejected from the kind their registry row declares
    /// (neither implements [`crate::Extrapolate`]).
    pub fn apply_easing_preset(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
        preset: PiecewiseEasingPreset,
    ) -> Result<(), ModelError> {
        let tick = self.keyframe_tick(clip_id, at)?;
        self.check_param_target(clip_id, param)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        param.apply_easing_preset(clip, tick, preset)
    }

    /// Append an effect (M4) to a visual clip's chain; the id must exist in
    /// the catalog. Returns the new effect's index. Rejected on audio clips.
    pub fn add_effect(&mut self, clip_id: ClipId, effect_id: &str) -> Result<usize, ModelError> {
        let instance = EffectInstance::new(effect_id);
        // Reject unknown ids up front (validate also covers an empty chain).
        instance.validate()?;
        self.check_visual_target(clip_id)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        clip.effects.push(instance);
        Ok(clip.effects.len() - 1)
    }

    /// Remove the effect at `index` from a clip's chain.
    pub fn remove_effect(&mut self, clip_id: ClipId, index: usize) -> Result<(), ModelError> {
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        if index >= clip.effects.len() {
            return Err(ModelError::InvalidParam(format!(
                "effect index {index} out of range"
            )));
        }
        clip.effects.remove(index);
        Ok(())
    }
    /// Replace a visual clip's entire effect chain with `effects`.
    pub fn set_clip_effects(
        &mut self,
        clip_id: ClipId,
        effects: Vec<EffectInstance>,
    ) -> Result<(), ModelError> {
        for effect in &effects {
            effect.validate()?;
        }
        self.check_visual_target(clip_id)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        clip.effects = effects;
        Ok(())
    }

    /// Apply an effect preset to `clip_id`, replacing its effect chain.
    pub fn apply_effect_preset(
        &mut self,
        clip_id: ClipId,
        preset: &EffectPreset,
    ) -> Result<(), ModelError> {
        preset.validate()?;
        self.set_clip_effects(clip_id, preset.effects.clone())
    }

    /// Move one effect within a clip's chain. Both indices address the chain
    /// before the move; `to_index` is the effect's final index after removal
    /// and insertion. Moving an effect to its current index is a valid no-op.
    pub fn move_effect(
        &mut self,
        clip_id: ClipId,
        from_index: usize,
        to_index: usize,
    ) -> Result<(), ModelError> {
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let len = clip.effects.len();
        if from_index >= len {
            return Err(ModelError::InvalidParam(format!(
                "effect from index {from_index} out of range for chain length {len}"
            )));
        }
        if to_index >= len {
            return Err(ModelError::InvalidParam(format!(
                "effect to index {to_index} out of range for chain length {len}"
            )));
        }
        if from_index == to_index {
            return Ok(());
        }

        let effect = clip.effects.remove(from_index);
        clip.effects.insert(to_index, effect);
        Ok(())
    }

    /// Set one effect parameter to a constant (the non-animated quick edit;
    /// keyframes go through [`Self::set_param_keyframe`] with
    /// [`ClipParam::Effect`]).
    pub fn set_effect_param(
        &mut self,
        clip_id: ClipId,
        index: usize,
        param: usize,
        value: f32,
    ) -> Result<(), ModelError> {
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        effect_mut(clip, index as u32)?.set_param_constant(param, value)
    }
}
