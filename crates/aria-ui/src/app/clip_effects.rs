//! Effect-chain commands: reordering the stack and setting parameters.
//!
//! A keyframeable slot reads and writes where every property row does: its value
//! at the playhead, held inside the clip, and a keyframe there when the slot is
//! animated. A constant scalar slot is set with `SetEffectParam`, the
//! non-animated quick edit; a constant colour, vector or point is set with
//! `SetParamConstant` on `ClipParam::Effect`, so an edit never grows a curve on a
//! slot that had none and never flattens one that had.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ClipId, ClipParam, EffectParamKind, EffectParamSpec, EffectSpec, ParamValue, effect_spec,
};

use super::AppState;

/// Colours worth one click on an effect parameter. A full picker is a separate
/// concern; these are the hues a key, a tint, or a border is actually set to.
pub const PARAM_COLORS: [(&str, &str, [u8; 4]); 8] = [
    ("black", "Black", [0, 0, 0, 255]),
    ("white", "White", [255, 255, 255, 255]),
    ("red", "Red", [220, 60, 60, 255]),
    ("green", "Green", [60, 200, 90, 255]),
    ("blue", "Blue", [60, 120, 230, 255]),
    ("cyan", "Cyan", [60, 210, 210, 255]),
    ("magenta", "Magenta", [210, 70, 190, 255]),
    ("clear", "Clear", [0, 0, 0, 0]),
];

/// The parameter slots of one effect that carry one kind, as (catalog index,
/// spec). The inspector walks `EffectSpec::params` in display order; the
/// effect-chain tests and censuses read one kind at a time through these.
///
/// Holds the spec rather than a copy of its rows, so listing slots allocates
/// nothing.
#[cfg(test)]
pub struct ParamSlots {
    spec: Option<Arc<EffectSpec>>,
    kind: EffectParamKind,
}

#[cfg(test)]
impl ParamSlots {
    pub fn iter(&self) -> ParamSlotIter<'_> {
        let params = self
            .spec
            .as_deref()
            .map_or(&[][..], |spec| &spec.params[..]);
        ParamSlotIter {
            params: params.iter().enumerate(),
            kind: self.kind,
        }
    }

    pub fn first(&self) -> Option<(usize, &EffectParamSpec)> {
        self.iter().next()
    }

    pub fn is_empty(&self) -> bool {
        self.first().is_none()
    }
    /// The catalog indices alone.
    pub fn slots(&self) -> impl Iterator<Item = usize> + '_ {
        self.iter().map(|(slot, _)| slot)
    }
}

#[cfg(test)]
impl<'a> IntoIterator for &'a ParamSlots {
    type Item = (usize, &'a EffectParamSpec);
    type IntoIter = ParamSlotIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
pub struct ParamSlotIter<'a> {
    params: std::iter::Enumerate<std::slice::Iter<'a, EffectParamSpec>>,
    kind: EffectParamKind,
}

#[cfg(test)]
impl<'a> Iterator for ParamSlotIter<'a> {
    type Item = (usize, &'a EffectParamSpec);

    fn next(&mut self) -> Option<Self::Item> {
        let kind = self.kind;
        self.params.find(|(_, param)| param.kind == kind)
    }
}

#[cfg(test)]
fn params_of_kind(effect_id: &str, kind: EffectParamKind) -> ParamSlots {
    ParamSlots {
        spec: effect_spec(effect_id),
        kind,
    }
}

#[cfg(test)]
pub fn scalar_params(effect_id: &str) -> ParamSlots {
    params_of_kind(effect_id, EffectParamKind::Scalar)
}

#[cfg(test)]
pub fn color_params(effect_id: &str) -> ParamSlots {
    params_of_kind(effect_id, EffectParamKind::Color)
}

#[cfg(test)]
pub fn vec2_params(effect_id: &str) -> ParamSlots {
    params_of_kind(effect_id, EffectParamKind::Vec2)
}

impl AppState {
    /// The Add list: every effect the model knows, with the label the button shows.
    ///
    /// `aria_models::effect_catalog` is the single owner of what is offerable — an
    /// id it does not carry would be refused by the engine, so offering one would
    /// be a lie. The mounted [`aria_plugin::EffectService`] decides only whether an
    /// entry has a GPU pass; one that does not is still offered and marked
    /// passthrough, because hiding it makes an effect the project format accepts
    /// look nonexistent.
    ///
    /// Asking the service rather than the compositor directly means a plugin that
    /// replaces the GPU catalog changes which entries are marked, and a session
    /// with no GPU plugin mounted marks all of them instead of claiming passes that
    /// nothing can dispatch.
    pub fn effect_browse_list(&self) -> Vec<(Arc<EffectSpec>, String)> {
        let service = self.ctx.try_get::<dyn aria_plugin::EffectService>();
        let renderable = |id: &str| service.as_ref().is_some_and(|svc| svc.renders_effect(id));
        aria_models::effect_catalog()
            .into_iter()
            .map(|spec| {
                let label = if renderable(&spec.id) {
                    spec.label.clone()
                } else {
                    format!("{} — passthrough", spec.label)
                };
                (spec, label)
            })
            .collect()
    }

    /// The value of one scalar or integer slot at the playhead.
    pub fn selected_effect_param(&self, index: usize, param: usize) -> Option<f32> {
        match self.effect_slot_value(index, param) {
            Some(ParamValue::Scalar(value)) => Some(value),
            Some(_) => None,
            None => self.selected_ui_clip()?.effects.get(index)?.scalar(param),
        }
    }

    /// The colour of one slot at the playhead.
    pub fn selected_effect_color(&self, index: usize, param: usize) -> Option<[u8; 4]> {
        match self.effect_slot_value(index, param) {
            Some(ParamValue::Color(value)) => Some(value),
            Some(_) => None,
            None => self.selected_ui_clip()?.effects.get(index)?.color(param),
        }
    }

    /// The vec2 of one slot at the playhead.
    pub fn selected_effect_vec2(&self, index: usize, param: usize) -> Option<[f32; 2]> {
        match self.effect_slot_value(index, param) {
            Some(ParamValue::Vec2(value)) => Some(value),
            Some(_) => None,
            None => self.selected_ui_clip()?.effects.get(index)?.vec2(param),
        }
    }

    /// What one keyframeable slot holds at the playhead, or `None` when the slot
    /// is not one the clip can animate now: a missing effect or slot, a kind with
    /// no curve, or a slot its effect hides. The readers above fall back to the
    /// projection for those, which holds the slot's stored value.
    fn effect_slot_value(&self, index: usize, param: usize) -> Option<ParamValue> {
        self.param_value(effect_param(index, param))
    }

    /// Whether one slot carries keyframes, so an edit keys it rather than
    /// setting a constant.
    fn effect_slot_animated(&self, index: usize, param: usize) -> bool {
        self.key_param(effect_param(index, param))
            .is_some_and(|row| !row.marks.is_empty())
    }

    /// Read one slot on one effect of the selected clip, when the slot exists
    /// and carries `kind`.
    fn with_effect_slot<R>(
        &self,
        index: usize,
        param: usize,
        kind: EffectParamKind,
        read: impl FnOnce(&EffectParamSpec) -> R,
    ) -> Option<R> {
        let ui = self.selected_ui_clip()?;
        let effect = ui.effects.get(index)?;
        let spec = effect_spec(&effect.id)?;
        let slot = spec.param_at(param)?;
        (slot.kind == kind).then(|| read(slot))
    }

    fn with_effect_slot_kinds<R>(
        &self,
        index: usize,
        param: usize,
        kinds: &[EffectParamKind],
        read: impl FnOnce(&EffectParamSpec) -> R,
    ) -> Option<R> {
        let ui = self.selected_ui_clip()?;
        let effect = ui.effects.get(index)?;
        let spec = effect_spec(&effect.id)?;
        let slot = spec.param_at(param)?;
        kinds.contains(&slot.kind).then(|| read(slot))
    }

    /// Set one colour effect parameter. Inert on a missing effect, a missing slot,
    /// or a slot that is not a colour.
    pub fn set_selected_effect_color(
        &mut self,
        index: usize,
        param: usize,
        rgba: [u8; 4],
    ) -> Result<(), EngineError> {
        if self
            .with_effect_slot(index, param, EffectParamKind::Color, |_| ())
            .is_none()
        {
            return Ok(());
        }
        self.set_param_value(effect_param(index, param), ParamValue::Color(rgba))
    }

    /// Set one vec2 effect parameter, each component clamped to the catalog range.
    pub fn set_selected_effect_vec2(
        &mut self,
        index: usize,
        param: usize,
        value: [f32; 2],
    ) -> Result<(), EngineError> {
        let Some((min, max)) = self.with_effect_slot_kinds(
            index,
            param,
            &[EffectParamKind::Vec2, EffectParamKind::Size],
            |s| (s.min, s.max),
        ) else {
            return Ok(());
        };
        if !value.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        let clamped = [value[0].clamp(min, max), value[1].clamp(min, max)];
        self.set_param_value(effect_param(index, param), ParamValue::Vec2(clamped))
    }

    /// Step one component of a vec2 parameter. `axis` is 0 for x and 1 for y.
    pub fn nudge_selected_effect_vec2(
        &mut self,
        index: usize,
        param: usize,
        axis: usize,
        step: f32,
    ) -> Result<(), EngineError> {
        let Some(mut current) = self.selected_effect_vec2(index, param) else {
            return Ok(());
        };
        let Some(slot) = current.get_mut(axis) else {
            return Ok(());
        };
        *slot += step;
        self.set_selected_effect_vec2(index, param, current)
    }

    /// Return a colour or vec2 parameter to its catalog default.
    pub fn reset_selected_effect_non_scalar(
        &mut self,
        index: usize,
        param: usize,
    ) -> Result<(), EngineError> {
        let color = |s: &EffectParamSpec| s.default_color;
        if let Some(default) = self.with_effect_slot(index, param, EffectParamKind::Color, color) {
            return self.set_selected_effect_color(index, param, default);
        }
        let vec2 = |s: &EffectParamSpec| s.default_vec2;
        if let Some(default) = self.with_effect_slot_kinds(
            index,
            param,
            &[EffectParamKind::Vec2, EffectParamKind::Size],
            vec2,
        ) {
            return self.set_selected_effect_vec2(index, param, default);
        }
        let point3 = |s: &EffectParamSpec| s.default_vec3;
        if let Some(default) = self.with_effect_slot(index, param, EffectParamKind::Point3, point3)
        {
            return self.set_selected_effect_point3(index, param, default);
        }
        Ok(())
    }

    /// Set one scalar effect parameter, clamped to the catalog range: a keyframe
    /// where the row reads when the slot is animated, `SetEffectParam` when it is
    /// not. Inert on a missing effect, a missing slot, or a non-scalar slot.
    pub fn set_selected_effect_param(
        &mut self,
        index: usize,
        param: usize,
        value: f32,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some((min, max)) = self.with_effect_slot_kinds(
            index,
            param,
            &[EffectParamKind::Scalar, EffectParamKind::Angle],
            |s| (s.min, s.max),
        ) else {
            return Ok(());
        };
        if !value.is_finite() {
            return Ok(());
        }
        let value = value.clamp(min, max);
        if self.effect_slot_animated(index, param) {
            return self.set_param_value(effect_param(index, param), ParamValue::Scalar(value));
        }
        self.apply_command(Command::Edit(EditCommand::SetEffectParam {
            clip,
            index,
            param,
            value,
        }))?;
        Ok(())
    }

    pub fn nudge_selected_effect_param(
        &mut self,
        index: usize,
        param: usize,
        step: f32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_effect_param(index, param) else {
            return Ok(());
        };
        self.set_selected_effect_param(index, param, current + step)
    }

    /// Return one parameter to its catalog default.
    pub fn reset_selected_effect_param(
        &mut self,
        index: usize,
        param: usize,
    ) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(effect) = ui.effects.get(index) else {
            return Ok(());
        };
        let Some(default) =
            effect_spec(&effect.id).and_then(|spec| spec.param_at(param).map(|p| p.default))
        else {
            return Ok(());
        };
        self.set_selected_effect_param(index, param, default)
    }

    /// Move an effect within the chain. Both indices address the pre-move chain;
    /// a move that would leave the chain, or go nowhere, is inert.
    pub fn move_selected_effect(&mut self, from: usize, to: usize) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        if from == to || from >= ui.effects.len() || to >= ui.effects.len() {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::MoveEffect {
            clip,
            from_index: from,
            to_index: to,
        }))?;
        Ok(())
    }

    /// Switch the effect at `index` off, or back on. Inert on an index the
    /// chain does not hold.
    pub fn toggle_selected_effect_bypass(&mut self, index: usize) -> Result<(), EngineError> {
        let Some(bypass) = self.selected_effect_bypass(index) else {
            return Ok(());
        };
        self.set_selected_effect_bypass(index, !bypass)
    }

    /// Whether the selected clip's effect at `index` is switched off; `None`
    /// past the end of the chain.
    pub fn selected_effect_bypass(&self, index: usize) -> Option<bool> {
        self.selected_ui_clip_ref()?
            .effects
            .get(index)
            .map(|effect| effect.bypass)
    }

    /// Switch the selected clip's effect at `index` off with `bypass`, or back
    /// on without it. Inert when it already reads that way and past the end of
    /// the chain.
    pub fn set_selected_effect_bypass(
        &mut self,
        index: usize,
        bypass: bool,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if self
            .selected_effect_bypass(index)
            .is_none_or(|now| now == bypass)
        {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetEffectBypass {
            clip,
            index,
            bypass,
        }))?;
        Ok(())
    }

    /// Move an effect one step earlier in the render order.
    pub fn move_selected_effect_up(&mut self, index: usize) -> Result<(), EngineError> {
        if index == 0 {
            return Ok(());
        }
        self.move_selected_effect(index, index - 1)
    }

    /// Move an effect one step later in the render order.
    pub fn move_selected_effect_down(&mut self, index: usize) -> Result<(), EngineError> {
        self.move_selected_effect(index, index + 1)
    }

    /// Add `effect_id` to the selected clip's chain before the effect at `at`,
    /// or at the end when `at` is `None` or past the end, as one undo step.
    pub fn insert_effect_in_selected(
        &mut self,
        effect_id: &str,
        at: Option<usize>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        let len = self.selected_ui_clip_ref().map_or(0, |ui| ui.effects.len());
        let at = at.filter(|&at| at < len);
        self.one_step(|state| {
            state.apply_command(Command::Edit(EditCommand::AddEffect {
                clip,
                effect_id: effect_id.to_owned(),
            }))?;
            if let Some(at) = at {
                state.apply_command(Command::Edit(EditCommand::MoveEffect {
                    clip,
                    from_index: len,
                    to_index: at,
                }))?;
            }
            Ok(())
        })?;
        self.last_effect = Some(effect_id.to_owned());
        Ok(())
    }

    /// The 3D point of one slot at the playhead.
    pub fn selected_effect_point3(&self, index: usize, param: usize) -> Option<[f32; 3]> {
        match self.effect_slot_value(index, param) {
            Some(ParamValue::Vec3(value)) => Some(value),
            Some(_) => None,
            None => self
                .selected_ui_clip_ref()?
                .effects
                .get(index)?
                .point3(param),
        }
    }

    /// Set a 3D point parameter on the selected clip's effect at `index`.
    pub fn set_selected_effect_point3(
        &mut self,
        index: usize,
        param: usize,
        value: [f32; 3],
    ) -> Result<(), EngineError> {
        let Some((min, max)) =
            self.with_effect_slot(index, param, EffectParamKind::Point3, |s| (s.min, s.max))
        else {
            return Ok(());
        };
        if !value.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        let clamped = [
            value[0].clamp(min, max),
            value[1].clamp(min, max),
            value[2].clamp(min, max),
        ];
        self.set_param_value(effect_param(index, param), ParamValue::Vec3(clamped))
    }

    /// Nudge one axis (`0` = X, `1` = Y, `2` = Z) of a 3D point parameter.
    pub fn nudge_selected_effect_point3(
        &mut self,
        index: usize,
        param: usize,
        axis: usize,
        by: f32,
    ) -> Result<(), EngineError> {
        let Some(mut point) = self.selected_effect_point3(index, param) else {
            return Ok(());
        };
        let Some(slot) = point.get_mut(axis) else {
            return Ok(());
        };
        *slot += by;
        self.set_selected_effect_point3(index, param, point)
    }

    fn modify_selected_effect_instance(
        &mut self,
        index: usize,
        edit: impl FnOnce(
            &mut aria_models::EffectInstance,
            &EffectSpec,
        ) -> Result<(), aria_models::ModelError>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(mut effect) = self
            .selected_model()
            .and_then(|model| model.effects.get(index).cloned())
        else {
            return Ok(());
        };
        let Some(spec) = effect_spec(&effect.effect_id) else {
            return Ok(());
        };
        let before = effect.clone();
        if edit(&mut effect, &spec).is_err() || effect == before {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetEffectInstance {
            clip,
            index,
            effect,
        }))?;
        Ok(())
    }

    /// The integer of one slot at the playhead, rounded as the renderer rounds
    /// it.
    pub fn selected_effect_int(&self, index: usize, param: usize) -> Option<i32> {
        match self.effect_slot_value(index, param) {
            Some(ParamValue::Scalar(value)) => Some(value.round() as i32),
            Some(_) => None,
            None => self.selected_ui_clip_ref()?.effects.get(index)?.int(param),
        }
    }

    /// Set an integer parameter on the selected clip's effect at `index`,
    /// clamped to its range: a keyframe where the row reads when the slot is
    /// animated, a constant when it is not.
    pub fn set_selected_effect_int(
        &mut self,
        index: usize,
        param: usize,
        value: i32,
    ) -> Result<(), EngineError> {
        if self.effect_slot_animated(index, param) {
            let Some((min, max)) = self.with_effect_slot(index, param, EffectParamKind::Int, |s| {
                (s.int_min, s.int_max)
            }) else {
                return Ok(());
            };
            let value = i64::from(value).clamp(min, max) as f32;
            return self.set_param_value(effect_param(index, param), ParamValue::Scalar(value));
        }
        self.modify_selected_effect_instance(index, |effect, spec| {
            let pspec = spec
                .params
                .get(param)
                .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
            effect
                .set_int_param_constant(param, i64::from(value).clamp(pspec.int_min, pspec.int_max))
        })
    }

    /// Step an integer parameter on the selected clip's effect at `index` by `by`.
    pub fn nudge_selected_effect_int(
        &mut self,
        index: usize,
        param: usize,
        by: i32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_effect_int(index, param) else {
            return Ok(());
        };
        self.set_selected_effect_int(index, param, current.saturating_add(by))
    }

    /// Current value of a boolean parameter on the selected clip's effect at `index`.
    pub fn selected_effect_bool(&self, index: usize, param: usize) -> Option<bool> {
        self.selected_ui_clip_ref()?
            .effects
            .get(index)?
            .bool_val(param)
    }

    /// Toggle a boolean parameter on the selected clip's effect at `index`.
    pub fn toggle_selected_effect_bool(
        &mut self,
        index: usize,
        param: usize,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_effect_bool(index, param) else {
            return Ok(());
        };
        self.set_selected_effect_bool(index, param, !current)
    }

    /// Set a boolean parameter on the selected clip's effect at `index` to
    /// `to`. Inert when it already reads `to` and on a slot that is no switch.
    pub fn set_selected_effect_bool(
        &mut self,
        index: usize,
        param: usize,
        to: bool,
    ) -> Result<(), EngineError> {
        if self
            .selected_effect_bool(index, param)
            .is_none_or(|now| now == to)
        {
            return Ok(());
        }
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_bool_param(param, to)
        })
    }

    /// Current value of a choice parameter on the selected clip's effect at `index`.
    pub fn selected_effect_choice(&self, index: usize, param: usize) -> Option<String> {
        self.selected_ui_clip_ref()?
            .effects
            .get(index)?
            .choice(param)
            .map(str::to_owned)
    }

    /// Set a choice parameter on the selected clip's effect at `index`.
    pub fn set_selected_effect_choice(
        &mut self,
        index: usize,
        param: usize,
        choice: &str,
    ) -> Result<(), EngineError> {
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_choice_param(param, choice)
        })
    }

    /// Set a 1D curve parameter on the selected clip's effect at `index`.
    pub fn set_selected_effect_curve(
        &mut self,
        index: usize,
        param: usize,
        points: Vec<aria_models::EffectCurvePoint>,
    ) -> Result<(), EngineError> {
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_curve_param(param, points)
        })
    }

    /// Set a gradient parameter on the selected clip's effect at `index`.
    pub fn set_selected_effect_gradient(
        &mut self,
        index: usize,
        param: usize,
        stops: Vec<aria_models::EffectGradientStop>,
    ) -> Result<(), EngineError> {
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_gradient_param(param, stops)
        })
    }

    /// Set a text/font/file parameter on the selected clip's effect at `index`.
    pub fn set_selected_effect_text(
        &mut self,
        index: usize,
        param: usize,
        text: &str,
    ) -> Result<(), EngineError> {
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_text_param(param, text)
        })
    }

    /// Set a layer reference parameter on the selected clip's effect at `index`.
    /// The inspector reaches it through `pick_selected_effect_layer`, which
    /// records a refusal for the row.
    pub(super) fn set_selected_effect_layer(
        &mut self,
        index: usize,
        param: usize,
        layer: Option<ClipId>,
    ) -> Result<(), EngineError> {
        self.modify_selected_effect_instance(index, |effect, _spec| {
            effect.set_layer_param(param, layer)
        })
    }

    /// Whether parameter group `group` on effect `index` is currently collapsed.
    pub fn is_effect_group_collapsed(
        &self,
        index: usize,
        group: &str,
        default_collapsed: bool,
    ) -> bool {
        let key = (index, group.to_string());
        if default_collapsed {
            !self.prop_rows.open_effect_groups.contains(&key)
        } else {
            self.prop_rows.collapsed_effect_groups.contains(&key)
        }
    }

    /// Flip the collapsed state of parameter group `group` on effect `index`.
    pub fn toggle_effect_group(&mut self, index: usize, group: &str) {
        let default_collapsed = self
            .selected_ui_clip_ref()
            .and_then(|clip| clip.effects.get(index))
            .and_then(|fx| effect_spec(&fx.id))
            .and_then(|spec| {
                spec.groups
                    .iter()
                    .find(|g| g.id == group)
                    .map(|g| g.collapsed)
            })
            .unwrap_or(false);
        let key = (index, group.to_string());
        if default_collapsed {
            if !self.prop_rows.open_effect_groups.remove(&key) {
                self.prop_rows.open_effect_groups.insert(key);
            }
        } else if !self.prop_rows.collapsed_effect_groups.remove(&key) {
            self.prop_rows.collapsed_effect_groups.insert(key);
        }
    }

    /// Apply a [`crate::surfaces::effects_rack::SlotEdit`] to parameter `param`
    /// on effect `index` of the selected clip.
    pub fn edit_selected_effect_slot(
        &mut self,
        index: usize,
        param: usize,
        edit: crate::surfaces::effects_rack::SlotEdit,
    ) -> Result<(), EngineError> {
        use crate::surfaces::effects_rack::SlotEdit;
        match edit {
            // An animated slot keys its default under the playhead, by the
            // route every property row's reset takes.
            SlotEdit::Reset if self.effect_slot_animated(index, param) => {
                self.reset_param(effect_param(index, param))
            }
            SlotEdit::Reset => self.modify_selected_effect_instance(index, |effect, spec| {
                let pspec = spec
                    .params
                    .get(param)
                    .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
                let name = pspec.name.as_str();
                effect.params.remove(name);
                effect.color_params.remove(name);
                effect.linear_color_params.remove(name);
                effect.vec2_params.remove(name);
                effect.vec3_params.remove(name);
                effect.bool_params.remove(name);
                effect.choice_params.remove(name);
                effect.curve_params.remove(name);
                effect.gradient_params.remove(name);
                effect.text_params.remove(name);
                effect.layer_params.remove(name);
                Ok(())
            }),
            SlotEdit::NudgeInt(by) => self.nudge_selected_effect_int(index, param, by),
            SlotEdit::ToggleBool => self.toggle_selected_effect_bool(index, param),
            SlotEdit::Choice(choice) => self.set_selected_effect_choice(index, param, &choice),
            SlotEdit::NudgePoint3 { axis, by } => {
                self.nudge_selected_effect_point3(index, param, axis, by)
            }
            SlotEdit::Curve(points) => self.set_selected_effect_curve(index, param, points),
            SlotEdit::Gradient(stops) => self.set_selected_effect_gradient(index, param, stops),
            SlotEdit::Text(text) => self.set_selected_effect_text(index, param, &text),
            SlotEdit::Layer(layer) => self.pick_selected_effect_layer(index, param, layer),
        }
    }
}

/// The address of slot `slot` on effect `index`, as every keyframe command and
/// property row names it.
pub(crate) fn effect_param(index: usize, slot: usize) -> ClipParam {
    ClipParam::Effect {
        effect: index as u32,
        param: slot as u32,
    }
}
