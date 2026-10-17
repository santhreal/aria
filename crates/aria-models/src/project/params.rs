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
    AnimatedTransform, Clip, ClipParam, ClipSource, ClipTransform, DriverCode, Generator,
    ParamScope, ParamValue, effect_mut, index_after_move, index_after_remove,
};
use crate::effects::EffectInstance;
use crate::error::ModelError;
use crate::ids::ClipId;
use crate::param::{Easing, PiecewiseEasingPreset, SpatialTangents, TemporalEase};
use crate::param_walk::ParamWalk;
use crate::preset::Preset;
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
        if param == ClipParam::Position {
            // Write value, easing, and tangents together before solving roving
            // ticks and temporal control points.
            clip.transform.position.set_keyframe_with_tangents(
                tick,
                value.vec2()?,
                easing,
                tangents,
            );
        } else {
            param.set_keyframe(clip, tick, value, easing)?;
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

    /// Set incoming/outgoing temporal ease and/or hold state on the keyframe at
    /// `at` (absolute timeline position). Errors when no keyframe sits there.
    pub fn set_param_keyframe_ease(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    ) -> Result<(), ModelError> {
        if let Some(inp) = in_ease {
            inp.validate()?;
        }
        if let Some(out) = out_ease {
            out.validate()?;
        }
        self.check_param_target(clip_id, param)?;
        let tick = self.keyframe_tick(clip_id, at)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        param.set_keyframe_temporal_ease(clip, tick, in_ease, out_ease, hold)
    }

    /// Set or clear roving on the keyframe at `at` (absolute timeline position)
    /// and return its solved absolute timeline position. Roving is ignored on
    /// the first and last keyframe of a property.
    pub fn set_param_keyframe_roving(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        at: RationalTime,
        roving: bool,
    ) -> Result<RationalTime, ModelError> {
        self.check_param_target(clip_id, param)?;
        let rate = self.timeline().frame_rate;
        let tick = self.keyframe_tick(clip_id, at)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let start_tick = clip.timeline.start.value;
        let solved_local = param.set_keyframe_roving(clip, tick, roving)?;
        Ok(RationalTime::new(start_tick + solved_local, rate))
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

    /// The name a driver addresses `clip` by: the clip's own name when it has
    /// one, else its source's: the media's display name, the nested
    /// sequence's or multicam group's name, a text clip's words, or the
    /// generator's catalog label.
    pub fn clip_title<'a>(&'a self, clip: &'a Clip) -> std::borrow::Cow<'a, str> {
        use std::borrow::Cow;
        if let Some(name) = &clip.name {
            return Cow::Borrowed(name);
        }
        match &clip.content {
            ClipSource::Media { media, .. } => self
                .media(*media)
                .map_or(Cow::Borrowed("Untitled"), |source| {
                    Cow::Borrowed(source.display_name())
                }),
            ClipSource::Sequence { sequence, .. } => self.sequence(*sequence).map_or_else(
                || Cow::Owned(format!("Sequence {}", sequence.raw())),
                |found| Cow::Borrowed(found.name.as_str()),
            ),
            ClipSource::Multicam { group, .. } => self.multicam_group(*group).map_or_else(
                || Cow::Owned(format!("Multicam {}", group.raw())),
                |found| Cow::Borrowed(found.name.as_str()),
            ),
            ClipSource::Generated(Generator::Text { content, .. }) => Cow::Borrowed(content),
            ClipSource::Generated(generator) => Cow::Borrowed(generator.spec().label),
        }
    }

    /// Attach `code` as the driver of one property, or detach the driver with
    /// `None`. Returns the code attached before.
    ///
    /// The property must be reachable on the clip. Compiling the code and
    /// checking its inputs for cycles is the caller's: the model stores code
    /// in any language, installed or not.
    pub fn set_param_driver(
        &mut self,
        clip_id: ClipId,
        param: ClipParam,
        code: Option<DriverCode>,
    ) -> Result<Option<DriverCode>, ModelError> {
        if let Some(code) = &code {
            code.validate()?;
            if param == ClipParam::Speed {
                return Err(ModelError::InvalidParam(
                    "a speed ramp is drawn with the speed curve and cannot take a driver".into(),
                ));
            }
            self.check_param_target(clip_id, param)?;
            let clip = self.clip(clip_id).ok_or(ModelError::UnknownClip(clip_id))?;
            if param.sample_at(clip, 0.0).is_none() {
                return Err(ModelError::InvalidParam(format!(
                    "{param:?} is not a property of that clip, so it cannot take a driver"
                )));
            }
        }
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        Ok(clip.put_driver(param, code))
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
        clip.remap_effect_refs(index_after_remove(index as u32));
        Ok(())
    }

    /// Replace a visual clip's entire effect chain with `effects`. Drivers on
    /// the old chain's parameters are dropped: an index into the new chain
    /// addresses a different effect.
    pub fn set_clip_effects(
        &mut self,
        clip_id: ClipId,
        mut effects: Vec<EffectInstance>,
    ) -> Result<(), ModelError> {
        for effect in &mut effects {
            effect.validate()?;
            effect.mark_from_catalog();
        }
        self.check_visual_target(clip_id)?;
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        clip.effects = effects;
        clip.remap_effect_refs(|_| None);
        Ok(())
    }

    /// Apply an animation preset to `clip_id` anchored at `at`.
    pub fn apply_preset(
        &mut self,
        clip_id: ClipId,
        preset: &Preset,
        at: RationalTime,
    ) -> Result<(), ModelError> {
        preset.validate()?;
        let anchor_local = self.keyframe_tick(clip_id, at)?;
        let mut shifted = preset.clone();
        shifted.walk_params(&mut |curve| curve.shift_fits(anchor_local))?;
        shifted.walk_params(&mut |curve| curve.shift(anchor_local))?;

        let mut resolved_props = Vec::with_capacity(shifted.properties.len());
        for prop in &shifted.properties {
            let spec = crate::clip::property_row(&prop.id).ok_or_else(|| {
                ModelError::InvalidParam(format!("property `{}`: unknown property id", prop.id))
            })?;
            resolved_props.push((prop.id.as_str(), spec, &prop.curve));
        }

        let needs_visual = !shifted.effects.is_empty()
            || resolved_props
                .iter()
                .any(|(_, spec, _)| spec.scope == ParamScope::Visual);
        if needs_visual {
            self.check_visual_target(clip_id)?;
        }
        if !shifted.audio_effects.is_empty() {
            self.insert_target_refusal(crate::AudioInsertTarget::Clip(clip_id))?;
        }
        if resolved_props
            .iter()
            .any(|(_, spec, _)| spec.scope == ParamScope::Audio)
        {
            self.check_audio_target(clip_id)?;
        }

        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let mut staged = clip.clone();

        for (id, spec, curve) in resolved_props {
            if !spec.available(&staged) {
                return Err(ModelError::InvalidParam(format!(
                    "property `{id}` is not available on clip {clip_id}; enable its block or select a compatible clip"
                )));
            }
            match curve {
                crate::param::Param::Constant(value) => {
                    if !spec.keyframes(&staged).is_empty() {
                        spec.param
                            .set_keyframe(&mut staged, anchor_local, *value, Easing::Linear)
                            .map_err(|err| {
                                ModelError::InvalidParam(format!("property `{id}`: {err}"))
                            })?;
                    } else {
                        spec.param
                            .set_constant(&mut staged, *value)
                            .map_err(|err| {
                                ModelError::InvalidParam(format!("property `{id}`: {err}"))
                            })?;
                    }
                }
                crate::param::Param::Keyframed { keyframes } => {
                    for kf in keyframes {
                        spec.param
                            .set_keyframe(&mut staged, kf.tick, kf.value, kf.easing)
                            .map_err(|err| {
                                ModelError::InvalidParam(format!("property `{id}`: {err}"))
                            })?;
                        if spec.param == ClipParam::Position {
                            staged
                                .transform
                                .position
                                .set_keyframe_tangents(kf.tick, kf.tangents)?;
                        }
                    }
                }
            }
        }

        for effect in &mut shifted.effects {
            effect.mark_from_catalog();
        }
        staged.effects.extend(shifted.effects);
        staged.audio_effects.extend(shifted.audio_effects);

        *self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))? = staged;
        Ok(())
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
        clip.remap_effect_refs(index_after_move(from_index as u32, to_index as u32));
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

    /// Switch the effect at `index` on `clip_id` off, or back on. A bypassed
    /// effect keeps its parameters and its place in the chain.
    pub fn set_effect_bypass(
        &mut self,
        clip_id: ClipId,
        index: usize,
        bypass: bool,
    ) -> Result<(), ModelError> {
        self.effect_instance_mut(clip_id, index)?.bypass = bypass;
        Ok(())
    }

    /// Borrow an effect instance on `clip_id` at `index`.
    pub fn effect_instance(
        &self,
        clip_id: ClipId,
        index: usize,
    ) -> Result<&EffectInstance, ModelError> {
        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        clip.effects
            .get(index)
            .ok_or_else(|| ModelError::InvalidParam(format!("effect index {index} out of range")))
    }

    /// Mutably borrow an effect instance on `clip_id` at `index`.
    pub fn effect_instance_mut(
        &mut self,
        clip_id: ClipId,
        index: usize,
    ) -> Result<&mut EffectInstance, ModelError> {
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        effect_mut(clip, index as u32)
    }

    /// Validate layer-reference parameters on `effect` attached to `clip_id`.
    ///
    /// Returns `Some(reason)` when `effect` references `clip_id` itself, a
    /// clip missing from the active sequence, or forms a cycle across clips'
    /// layer-reference parameters.
    pub fn effect_layer_ref_error(
        &self,
        clip_id: ClipId,
        effect: &EffectInstance,
    ) -> Option<String> {
        for (param_name, &target_raw) in &effect.layer_params {
            let target_id = ClipId::from_raw(target_raw);
            if target_id == clip_id {
                return Some(format!(
                    "effect `{}` parameter `{param_name}` on clip {} cannot reference itself",
                    effect.effect_id,
                    clip_id.raw()
                ));
            }
            if self.timeline().clip(target_id).is_none() {
                return Some(format!(
                    "effect `{}` parameter `{param_name}` references missing clip {}",
                    effect.effect_id, target_raw
                ));
            }
            // The new reference closes a cycle when `target_id` reaches
            // `clip_id` through existing layer-reference parameters. A clip
            // reached twice is a diamond, not a cycle, and is walked once.
            let mut stack = vec![target_id];
            let mut visited = std::collections::BTreeSet::new();
            while let Some(curr) = stack.pop() {
                if !visited.insert(curr) {
                    continue;
                }
                if let Some(curr_clip) = self.timeline().clip(curr) {
                    for fx in &curr_clip.effects {
                        for &next_raw in fx.layer_params.values() {
                            let next_id = ClipId::from_raw(next_raw);
                            if next_id == clip_id {
                                return Some(format!(
                                    "effect `{}` parameter `{param_name}` on clip {} forms a layer-reference cycle with clip {}",
                                    effect.effect_id,
                                    clip_id.raw(),
                                    target_raw
                                ));
                            }
                            stack.push(next_id);
                        }
                    }
                }
            }
        }
        None
    }

    /// Replace one effect instance at `index` on `clip_id` after validating it.
    pub fn set_effect_instance(
        &mut self,
        clip_id: ClipId,
        index: usize,
        mut effect: EffectInstance,
    ) -> Result<(), ModelError> {
        effect.validate()?;
        effect.mark_from_catalog();
        self.check_visual_target(clip_id)?;
        if let Some(err) = self.effect_layer_ref_error(clip_id, &effect) {
            return Err(ModelError::InvalidParam(err));
        }
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let slot = effect_mut(clip, index as u32)?;
        *slot = effect;
        Ok(())
    }
}
