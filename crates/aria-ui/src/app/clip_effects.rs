//! Effect-chain commands: reordering the stack and setting parameters.
//!
//! `SetEffectParam` is the non-animated quick edit and takes a scalar, so scalar
//! slots are set through it. A colour or vec2 slot has no constant command: the
//! model reaches those through `SetParamKeyframe` with `ClipParam::Effect`, so a
//! single keyframe is what "the value" means for them.

use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    ClipParam, Easing, EffectParamKind, EffectParamSpec, EffectSpec, ParamValue, RationalTime,
    effect_spec,
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
/// spec).
///
/// Read from the catalog when built, so a new parameter appears in the
/// inspector without a second list to update. Holds the spec rather than a
/// copy of its rows, so listing slots allocates nothing.
pub struct ParamSlots {
    spec: Option<Arc<EffectSpec>>,
    kind: EffectParamKind,
}

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

impl<'a> IntoIterator for &'a ParamSlots {
    type Item = (usize, &'a EffectParamSpec);
    type IntoIter = ParamSlotIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct ParamSlotIter<'a> {
    params: std::iter::Enumerate<std::slice::Iter<'a, EffectParamSpec>>,
    kind: EffectParamKind,
}

impl<'a> Iterator for ParamSlotIter<'a> {
    type Item = (usize, &'a EffectParamSpec);

    fn next(&mut self) -> Option<Self::Item> {
        let kind = self.kind;
        self.params.find(|(_, param)| param.kind == kind)
    }
}

pub fn params_of_kind(effect_id: &str, kind: EffectParamKind) -> ParamSlots {
    ParamSlots {
        spec: effect_spec(effect_id),
        kind,
    }
}

pub fn scalar_params(effect_id: &str) -> ParamSlots {
    params_of_kind(effect_id, EffectParamKind::Scalar)
}

pub fn color_params(effect_id: &str) -> ParamSlots {
    params_of_kind(effect_id, EffectParamKind::Color)
}

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

    /// The value of one effect parameter slot, as projected.
    pub fn selected_effect_param(&self, index: usize, param: usize) -> Option<f32> {
        self.selected_ui_clip()?.effects.get(index)?.scalar(param)
    }

    /// The colour of one effect parameter slot, as projected.
    pub fn selected_effect_color(&self, index: usize, param: usize) -> Option<[u8; 4]> {
        self.selected_ui_clip()?.effects.get(index)?.color(param)
    }

    /// The vec2 of one effect parameter slot, as projected.
    pub fn selected_effect_vec2(&self, index: usize, param: usize) -> Option<[f32; 2]> {
        self.selected_ui_clip()?.effects.get(index)?.vec2(param)
    }

    /// The tick a non-scalar parameter edit lands on: the playhead when it is over
    /// the selected clip, otherwise the clip's first tick.
    ///
    /// A colour has no constant command, so setting one writes a keyframe. Writing
    /// it at the clip start when the playhead is elsewhere is what "the clip's
    /// colour" means; a tick the clip does not own would be refused.
    fn effect_param_tick(&self) -> Option<RationalTime> {
        let clip = self.selected_model()?;
        let start = clip.timeline.start;
        let inside =
            self.playhead.value >= start.value && self.playhead.value < clip.timeline.end_tick();
        Some(if inside { self.playhead } else { start })
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

    /// Set one colour effect parameter. Inert on a missing effect, a missing slot,
    /// or a slot that is not a colour.
    pub fn set_selected_effect_color(
        &mut self,
        index: usize,
        param: usize,
        rgba: [u8; 4],
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if self
            .with_effect_slot(index, param, EffectParamKind::Color, |_| ())
            .is_none()
        {
            return Ok(());
        }
        let Some(at) = self.effect_param_tick() else {
            return Ok(());
        };
        if self.selected_effect_color(index, param) == Some(rgba) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param: ClipParam::Effect {
                effect: index as u32,
                param: param as u32,
            },
            at,
            value: ParamValue::Color(rgba),
            easing: Easing::Linear,
            tangents: None,
        }))?;
        Ok(())
    }

    /// Set one vec2 effect parameter, each component clamped to the catalog range.
    pub fn set_selected_effect_vec2(
        &mut self,
        index: usize,
        param: usize,
        value: [f32; 2],
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some((min, max)) =
            self.with_effect_slot(index, param, EffectParamKind::Vec2, |s| (s.min, s.max))
        else {
            return Ok(());
        };
        if !value.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        let clamped = [value[0].clamp(min, max), value[1].clamp(min, max)];
        let Some(at) = self.effect_param_tick() else {
            return Ok(());
        };
        if self.selected_effect_vec2(index, param) == Some(clamped) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param: ClipParam::Effect {
                effect: index as u32,
                param: param as u32,
            },
            at,
            value: ParamValue::Vec2(clamped),
            easing: Easing::Linear,
            tangents: None,
        }))?;
        Ok(())
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
        if let Some(default) = self.with_effect_slot(index, param, EffectParamKind::Vec2, vec2) {
            return self.set_selected_effect_vec2(index, param, default);
        }
        Ok(())
    }

    /// Set one scalar effect parameter, clamped to the catalog range. Inert on a
    /// missing effect, a missing slot, or a non-scalar slot.
    pub fn set_selected_effect_param(
        &mut self,
        index: usize,
        param: usize,
        value: f32,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some((min, max)) =
            self.with_effect_slot(index, param, EffectParamKind::Scalar, |s| (s.min, s.max))
        else {
            return Ok(());
        };
        if !value.is_finite() {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetEffectParam {
            clip,
            index,
            param,
            value: value.clamp(min, max),
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
        let len = ui.effects.len();
        if from >= len || to >= len || from == to {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::MoveEffect {
            clip,
            from_index: from,
            to_index: to,
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
}
