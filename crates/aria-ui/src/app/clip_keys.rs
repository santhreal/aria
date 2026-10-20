//! Keyframe commands: writing, removing, and easing an animatable property.
//!
//! `SetParamKeyframe`, `RemoveParamKeyframe`, and `ApplyEasingPreset` had no
//! UI path, so every property was a constant in the editor. The keyframe
//! strip is driven from [`aria_models::clip_param_catalog`], and the value
//! written is the property's own sampled value at the playhead, so adding a
//! keyframe pins what is already on screen rather than snapping to a default.
//!
//! Nothing here restates the model's routing. This module used to carry its own
//! six-row table and three `_ =>` fallbacks, which silently swallowed every
//! property the table did not name: the model declared 58 animatable properties
//! and the editor offered 6, with a green suite. Every read now goes through the
//! catalog row, so a property the model can animate is a property the strip can
//! animate, and a property it cannot reach cannot be offered.
//!
//! Keyframe ticks are clip-local in the model. Everything here takes and returns
//! absolute timeline ticks, converting at the boundary, so no surface has to know
//! which convention it is holding.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Clip, ClipParam, Easing, EasingPreset, ParamKind, ParamScope, ParamSpec, ParamValue,
    PiecewiseEasingPreset, clip_params_for,
};

use super::AppState;
use super::property_rows::KeyframeIconShape;
use gpui::SharedString;

/// The multi-keyframe easing presets, with a label each. These expand one
/// segment into several keyframes rather than shaping a single curve.
pub const PIECEWISE_PRESETS: [(PiecewiseEasingPreset, &str); 3] = [
    (PiecewiseEasingPreset::BounceOut, "Bounce out"),
    (PiecewiseEasingPreset::ElasticOut, "Elastic out"),
    (PiecewiseEasingPreset::BackOut, "Back out"),
];

/// One keyframe of one property, in absolute timeline ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyMark {
    pub tick: i64,
    /// The named bezier preset on the segment leaving this keyframe, if the
    /// easing matches one.
    pub preset: Option<&'static str>,
    /// The icon the keyframe is drawn as, from its easing and whether it is
    /// first, last, or between two others.
    pub shape: KeyframeIconShape,
}

/// The id and the label of one component row of a vector property.
///
/// A stepper edits one number, so a `Vec2` property is drawn as two rows with
/// `-x` / `-y` ids and `X` / `Y` labels. That text is a property of the catalog
/// row, not of the frame, so it is built where the rows are projected rather
/// than formatted per row per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisText {
    pub id: SharedString,
    pub label: SharedString,
}

/// The component suffix and label suffix of each axis, in component order.
const AXES: [(&str, &str); 2] = [("x", "X"), ("y", "Y")];
const AXES_3: [(&str, &str); 3] = [("x", "X"), ("y", "Y"), ("z", "Z")];
/// One animatable property of a clip, as every surface reads it.
///
/// Built once per change by [`crate::app::selected_detail`], not once per
/// element per frame: the id and the label are shared text, the catalog row
/// travels with the property so no reader enumerates the catalog again, and the
/// keys are already converted to absolute timeline ticks. A frame clones a
/// handle.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyParam {
    /// The catalog id, for an element id and for a surface that names a
    /// property.
    pub id: SharedString,
    pub label: SharedString,
    /// The catalog row: value kind, scope, group, declared range, and the route
    /// that samples it.
    pub spec: ParamSpec,
    /// Every keyframe on the property, in absolute timeline ticks.
    pub marks: std::sync::Arc<[KeyMark]>,
    /// The component rows this property is drawn as: empty for anything a single
    /// stepper can edit, one per axis for a vector property.
    axes: std::sync::Arc<[AxisText]>,
}

impl KeyParam {
    /// Which property this row addresses.
    pub fn param(&self) -> ClipParam {
        self.spec.param
    }

    /// Whether a keyframe sits before absolute `tick`, and whether one sits
    /// after it: where a navigator's arrows can go.
    pub fn keys_around(&self, tick: i64) -> (bool, bool) {
        (
            self.marks.first().is_some_and(|mark| mark.tick < tick),
            self.marks.last().is_some_and(|mark| mark.tick > tick),
        )
    }

    /// Whether a keyframe sits exactly on absolute `tick`.
    pub fn keyed_at(&self, tick: i64) -> bool {
        self.marks
            .binary_search_by_key(&tick, |mark| mark.tick)
            .is_ok()
    }

    /// The named bezier preset on the segment leaving the keyframe at absolute
    /// `tick`, when one sits there and its easing matches a preset.
    pub fn preset_at(&self, tick: i64) -> Option<&'static str> {
        self.marks
            .binary_search_by_key(&tick, |mark| mark.tick)
            .ok()
            .and_then(|index| self.marks[index].preset)
    }

    /// The component rows this property is drawn as, empty when it is a single
    /// value. Read by the stepper rows so the axis text is not rebuilt per frame.
    pub fn axes(&self) -> &[AxisText] {
        &self.axes
    }
}

/// A catalog string as shared text.
///
/// A shipped row's id and label are `'static` literals, so sharing one copies
/// nothing. An effect parameter's are owned by the effect catalog, so they are
/// copied once per projection instead of once per row per frame.
fn shared(text: std::borrow::Cow<'static, str>) -> SharedString {
    match text {
        std::borrow::Cow::Borrowed(text) => SharedString::new_static(text),
        std::borrow::Cow::Owned(text) => text.into(),
    }
}

/// Every animatable property of `clip`, in catalog order.
///
/// A row is offered when the model says the property is reachable on this clip
/// — the owning block exists, or the effect and slot exist — and the lane
/// allows it. `audio_lane` is the lane's answer, which the clip alone cannot
/// give: an audio-lane clip refuses the visual properties. Effect parameters
/// are part of the answer, so a plugin's effect is keyframable by the same
/// route as a shipped one.
///
/// This is the only place the catalog is enumerated for a clip. Everything else
/// reads the projected rows, which is what turns a per-element catalog clone
/// into a handle clone.
pub(crate) fn key_params_of(clip: &Clip, audio_lane: bool) -> std::sync::Arc<[KeyParam]> {
    clip_params_for(clip)
        .into_iter()
        .filter(|spec| !(audio_lane && matches!(spec.scope, ParamScope::Visual)))
        .map(|spec| {
            let keys = spec.keyframes(clip);
            let total = keys.len();
            let marks: std::sync::Arc<[KeyMark]> = keys
                .into_iter()
                .enumerate()
                .map(|(index, (tick, easing))| KeyMark {
                    tick: clip.param_timeline_tick(tick),
                    preset: easing.preset_id(),
                    shape: KeyframeIconShape::classify(index, total, easing),
                })
                .collect();
            let id = shared(spec.id.clone());
            let label = shared(spec.label.clone());
            let axes: std::sync::Arc<[AxisText]> = match spec.kind {
                ParamKind::Vec2 => AXES
                    .iter()
                    .map(|(suffix, axis)| AxisText {
                        id: format!("{id}-{suffix}").into(),
                        label: format!("{label} {axis}").into(),
                    })
                    .collect(),
                ParamKind::Vec3 => AXES_3
                    .iter()
                    .map(|(suffix, axis)| AxisText {
                        id: format!("{id}-{suffix}").into(),
                        label: format!("{label} {axis}").into(),
                    })
                    .collect(),
                // A colour needs a picker and a rectangle needs handles, so
                // neither is drawn as a row of steppers either.
                ParamKind::Scalar | ParamKind::Color | ParamKind::Rect => std::sync::Arc::from([]),
            };
            KeyParam {
                id,
                label,
                spec,
                marks,
                axes,
            }
        })
        .collect()
}

impl AppState {
    /// The properties the selected clip can actually animate.
    ///
    /// A handle on the projected rows, which are built once per change beside
    /// the store (see `key_params_of` and
    /// [`crate::app::selected_detail`]). Empty when nothing is selected.
    pub fn key_params(&self) -> std::sync::Arc<[KeyParam]> {
        self.selected
            .as_ref()
            .map(|detail| std::sync::Arc::clone(&detail.params))
            .unwrap_or_default()
    }

    /// One projected property of the selected clip.
    ///
    /// `None` when nothing is selected, and when the property is not reachable
    /// on the selection or its lane refuses it — the same rule that decides
    /// whether a row is offered decides whether it answers, because it is the
    /// same list.
    pub fn key_param(&self, param: ClipParam) -> Option<&KeyParam> {
        self.selected
            .as_ref()?
            .params
            .iter()
            .find(|row| row.spec.param == param)
    }

    /// Every keyframe on one property of the selected clip, in absolute ticks.
    pub fn keyframes_of(&self, param: ClipParam) -> std::sync::Arc<[KeyMark]> {
        self.key_param(param)
            .map(|row| std::sync::Arc::clone(&row.marks))
            .unwrap_or_default()
    }

    /// Whether a keyframe sits exactly under the playhead on `param`.
    pub fn has_keyframe_at_playhead(&self, param: ClipParam) -> bool {
        self.key_param(param)
            .is_some_and(|row| row.keyed_at(self.playhead.value))
    }

    /// The property's current value at the playhead, as the command's payload.
    /// `None` when the playhead is off the clip or the property is not reachable
    /// on it.
    fn value_at_playhead(&self, param: ClipParam) -> Option<ParamValue> {
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        if self.playhead.value < clip.timeline.start.value
            || self.playhead.value >= clip.timeline.end_tick()
        {
            return None;
        }
        spec.sample(clip, clip.param_tick(self.playhead.value))
    }

    /// Pin the property's current value at the playhead as a keyframe. Inert
    /// when the playhead is off the clip, so a click outside a clip's extent does
    /// not raise an error.
    pub fn add_keyframe_at_playhead(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(value) = self.value_at_playhead(param) else {
            return Ok(());
        };
        let easing = self.keyframe_easing(param).unwrap_or(Easing::Linear);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at: self.playhead,
            value,
            easing,
            tangents: None,
        }))?;
        Ok(())
    }

    /// The easing on the keyframe under the playhead, if there is one.
    fn keyframe_easing(&self, param: ClipParam) -> Option<Easing> {
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        spec.easing_at(clip, clip.param_tick(self.playhead.value))
    }

    /// Remove the keyframe under the playhead. Inert when there is none, so a
    /// Delete press with nothing under the playhead is not an error.
    pub fn remove_keyframe_at_playhead(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.has_keyframe_at_playhead(param) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RemoveParamKeyframe {
            clip,
            param,
            at: self.playhead,
        }))?;
        Ok(())
    }

    /// The easing on the keyframe at absolute `tick`, if there is one.
    ///
    /// A drag that moves a keyframe in time reads this before the move and
    /// writes it back after, so the shape of the segment leaving it survives a
    /// gesture that only meant to change when it happens.
    pub fn keyframe_easing_at(&self, param: ClipParam, tick: i64) -> Option<Easing> {
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        spec.easing_at(clip, clip.param_tick(tick))
    }

    /// Remove the keyframe at absolute `tick`. Inert when none sits there.
    pub fn remove_keyframe_at(&mut self, param: ClipParam, tick: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self
            .keyframes_of(param)
            .iter()
            .any(|mark| mark.tick == tick)
        {
            return Ok(());
        }
        let at = aria_models::RationalTime::new(tick, self.store.sequence.frame_rate);
        self.apply_command(Command::Edit(EditCommand::RemoveParamKeyframe {
            clip,
            param,
            at,
        }))?;
        Ok(())
    }

    /// Write one scalar or one axis of a vector property at absolute `tick`.
    ///
    /// `component` is the axis a two-dimensional property is being edited on;
    /// `None` is a scalar property. The other axis keeps the value it already
    /// samples to, because a graph edits one axis at a time and a payload that
    /// carried a default for the other would silently reset it.
    pub fn set_keyframe_value_at(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        tick: i64,
        value: f32,
        easing: Easing,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let model = self.selected_model().ok_or(EngineError::Model(
            aria_models::ModelError::InvalidParam("no clip selected".into()),
        ))?;
        let spec = &self
            .key_param(param)
            .ok_or(EngineError::Model(aria_models::ModelError::InvalidParam(
                "property not reachable".into(),
            )))?
            .spec;
        let clamped = spec.range.clamp(value);
        let payload = match (component, spec.sample(model, model.param_tick(tick))) {
            (Some(axis), Some(ParamValue::Vec2(mut axes))) => {
                if let Some(slot) = axes.get_mut(axis as usize) {
                    *slot = clamped;
                }
                ParamValue::Vec2(axes)
            }
            _ => ParamValue::Scalar(clamped),
        };
        let at = aria_models::RationalTime::new(tick, self.store.sequence.frame_rate);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at,
            value: payload,
            easing,
            tangents: None,
        }))?;
        Ok(())
    }

    /// Shape the segment leaving the keyframe under the playhead with a named
    /// bezier preset, keeping the keyframe's own value. Inert without a keyframe
    /// there, and on an unknown preset id.
    pub fn set_keyframe_easing_preset(
        &mut self,
        param: ClipParam,
        preset_id: &str,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.has_keyframe_at_playhead(param) {
            return Ok(());
        }
        let Some(easing) = Easing::from_preset_id(preset_id) else {
            return Ok(());
        };
        let Some(value) = self.value_at_playhead(param) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at: self.playhead,
            value,
            easing,
            tangents: None,
        }))?;
        Ok(())
    }

    /// Set custom or preset easing on the keyframe at `tick`.
    pub fn set_keyframe_easing(
        &mut self,
        param: ClipParam,
        tick: i64,
        easing: Easing,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let model = self.selected_model().ok_or(EngineError::Model(
            aria_models::ModelError::InvalidParam("no clip selected".into()),
        ))?;
        let spec = &self
            .key_param(param)
            .ok_or(EngineError::Model(aria_models::ModelError::InvalidParam(
                "property not reachable".into(),
            )))?
            .spec;
        let Some(value) = spec.sample(model, model.param_tick(tick)) else {
            return Ok(());
        };
        let at = aria_models::RationalTime::new(tick, self.store.sequence.frame_rate);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at,
            value,
            easing,
            tangents: None,
        }))?;
        Ok(())
    }

    /// Set keyframe scalar value at `tick`, keeping the easing already there.
    pub fn set_keyframe_scalar(
        &mut self,
        param: ClipParam,
        tick: i64,
        value: f32,
    ) -> Result<(), EngineError> {
        let easing = self
            .keyframe_easing_at(param, tick)
            .unwrap_or(Easing::Linear);
        self.set_keyframe_value_at(param, None, tick, value, easing)
    }

    /// Expand the segment leaving the keyframe under the playhead into a bounce,
    /// elastic, or back approximation.
    pub fn apply_piecewise_preset(
        &mut self,
        param: ClipParam,
        preset: PiecewiseEasingPreset,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.has_keyframe_at_playhead(param) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::ApplyEasingPreset {
            clip,
            param,
            at: self.playhead,
            preset,
        }))?;
        Ok(())
    }

    /// Drop every keyframe on a property, holding the value it has at the
    /// playhead. Inert when the property is already a constant.
    pub fn flatten_param(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if self.keyframes_of(param).is_empty() {
            return Ok(());
        }
        let Some(value) = self.value_at_playhead(param) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetParamConstant {
            clip,
            param,
            value,
        }))?;
        Ok(())
    }

    /// Move the playhead to the next keyframe on `param` after it, if any.
    pub fn go_to_next_keyframe(&mut self, param: ClipParam) {
        let at = self.playhead.value;
        if let Some(mark) = self.keyframes_of(param).iter().find(|mark| mark.tick > at) {
            self.set_playhead(mark.tick);
        }
    }

    /// Move the playhead to the previous keyframe on `param`, if any.
    pub fn go_to_prev_keyframe(&mut self, param: ClipParam) {
        let at = self.playhead.value;
        if let Some(mark) = self
            .keyframes_of(param)
            .iter()
            .rev()
            .find(|mark| mark.tick < at)
        {
            self.set_playhead(mark.tick);
        }
    }
}

/// The named bezier presets, for a picker.
pub fn easing_presets() -> &'static [EasingPreset] {
    &aria_models::EASING_PRESETS
}
