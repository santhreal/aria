use aria_models::{
    Clip, ClipId, ClipParam, ClipSource, ColorAdjustments, CropRect, Easing, EffectInstance,
    LayerStyles, LookParam, Mask, ModelError, Param, ParamValue, PiecewiseEasingPreset,
    RationalTime, Scale2, SpatialTangents,
};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

#[derive(Debug, Clone, PartialEq)]
pub enum LookState {
    Mask(Option<Mask>),
    Adjust(ColorAdjustments),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamState {
    Position(Param<[f32; 2]>),
    AnchorPoint(Param<[f32; 2]>),
    Scale(Param<Scale2>),
    Rotation(Param<f32>),
    Opacity(Param<f32>),
    Crop(Param<CropRect>),
    Volume(Param<f32>),
    Pan(Param<f32>),
    Look(Box<LookState>),
    Style(Box<LayerStyles>),
    TextOrShape(Box<ClipSource>),
    Effect {
        effect_idx: usize,
        instance: EffectInstance,
    },
}

fn is_mask_look_param(param: LookParam) -> bool {
    matches!(
        param,
        LookParam::MaskFeather
            | LookParam::MaskCenter
            | LookParam::MaskSize
            | LookParam::MaskRotation
            | LookParam::MaskRoundness
    )
}

impl ParamState {
    pub fn capture(clip: &Clip, param: ClipParam) -> Result<Self, ModelError> {
        match param {
            ClipParam::Position => Ok(ParamState::Position(clip.transform.position.clone())),
            ClipParam::AnchorPoint => {
                Ok(ParamState::AnchorPoint(clip.transform.anchor_point.clone()))
            }
            ClipParam::Scale => Ok(ParamState::Scale(clip.transform.scale.clone())),
            ClipParam::Rotation => Ok(ParamState::Rotation(clip.transform.rotation.clone())),
            ClipParam::Opacity => Ok(ParamState::Opacity(clip.transform.opacity.clone())),
            ClipParam::Crop => Ok(ParamState::Crop(clip.crop.clone())),
            ClipParam::Volume => Ok(ParamState::Volume(clip.volume.clone())),
            ClipParam::Pan => Ok(ParamState::Pan(clip.pan.clone())),
            ClipParam::Look { param: look } => {
                if is_mask_look_param(look) {
                    Ok(ParamState::Look(Box::new(LookState::Mask(
                        clip.mask.clone(),
                    ))))
                } else {
                    Ok(ParamState::Look(Box::new(LookState::Adjust(
                        clip.adjust.clone(),
                    ))))
                }
            }
            ClipParam::Style { .. } => Ok(ParamState::Style(Box::new(clip.styles.clone()))),
            ClipParam::Text { .. } | ClipParam::Shape { .. } => {
                Ok(ParamState::TextOrShape(Box::new(clip.content.clone())))
            }
            ClipParam::Effect { effect, .. } => {
                let instance = clip.effects.get(effect as usize).cloned().ok_or_else(|| {
                    ModelError::InvalidParam(format!("effect index {effect} out of range"))
                })?;
                Ok(ParamState::Effect {
                    effect_idx: effect as usize,
                    instance,
                })
            }
            ClipParam::Speed => Err(ModelError::InvalidParam(
                "speed is not edited via set_param".into(),
            )),
        }
    }

    pub fn restore(self, clip: &mut Clip, param: ClipParam) -> Result<ParamState, ModelError> {
        let current = ParamState::capture(clip, param)?;
        match (self, param) {
            (ParamState::Position(p), ClipParam::Position) => {
                clip.transform.position = p;
            }
            (ParamState::AnchorPoint(p), ClipParam::AnchorPoint) => {
                clip.transform.anchor_point = p;
            }
            (ParamState::Scale(p), ClipParam::Scale) => {
                clip.transform.scale = p;
            }
            (ParamState::Rotation(p), ClipParam::Rotation) => {
                clip.transform.rotation = p;
            }
            (ParamState::Opacity(p), ClipParam::Opacity) => {
                clip.transform.opacity = p;
            }
            (ParamState::Crop(p), ClipParam::Crop) => {
                clip.crop = p;
            }
            (ParamState::Volume(p), ClipParam::Volume) => {
                clip.volume = p;
            }
            (ParamState::Pan(p), ClipParam::Pan) => {
                clip.pan = p;
            }
            (ParamState::Look(look_state), ClipParam::Look { .. }) => match *look_state {
                LookState::Mask(mask) => {
                    clip.mask = mask;
                }
                LookState::Adjust(adjust) => {
                    clip.adjust = adjust;
                }
            },
            (ParamState::Style(styles), ClipParam::Style { .. }) => {
                clip.styles = *styles;
            }
            (
                ParamState::TextOrShape(content),
                ClipParam::Text { .. } | ClipParam::Shape { .. },
            ) => {
                clip.content = *content;
            }
            (
                ParamState::Effect {
                    effect_idx,
                    instance,
                },
                ClipParam::Effect { .. },
            ) => {
                if let Some(slot) = clip.effects.get_mut(effect_idx) {
                    *slot = instance;
                }
            }
            _ => {
                return Err(ModelError::InvalidParam(format!(
                    "mismatched param restore for {param:?}"
                )));
            }
        }
        Ok(current)
    }
}

pub struct RestoreParamAction {
    pub clip: ClipId,
    pub param: ClipParam,
    pub state: ParamState,
}

impl EditAction for RestoreParamAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let clip_id = self.clip;
        let param = self.param;
        let clip = ctx
            .project
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let current_state = self.state.restore(clip, param)?;
        Ok(Box::new(RestoreParamAction {
            clip: clip_id,
            param,
            state: current_state,
        }))
    }
}

pub fn set_keyframe(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    at: RationalTime,
    value: ParamValue,
    easing: Easing,
    tangents: Option<SpatialTangents>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project
        .set_param_keyframe(clip, param, at, value, easing, tangents)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

pub fn remove_keyframe(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    at: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project.remove_param_keyframe(clip, param, at)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

pub fn set_constant(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    value: ParamValue,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project.set_param_constant(clip, param, value)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

pub fn apply_easing_preset(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    at: RationalTime,
    preset: PiecewiseEasingPreset,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project.apply_easing_preset(clip, param, at, preset)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}
