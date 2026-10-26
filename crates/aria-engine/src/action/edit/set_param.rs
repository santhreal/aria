use aria_models::{
    ChromaKey, Clip, ClipId, ClipParam, ClipSource, ColorAdjustments, CropRect, Easing,
    EffectInstance, Filter, LayerStyles, LookParam, Lut, Mask, ModelError, Param, ParamValue,
    PiecewiseEasingPreset, RationalTime, Scale2, SpatialTangents, TemporalEase, TrackMatte,
};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// The look block a [`LookParam`] is stored in.
#[derive(Debug, Clone, PartialEq)]
pub enum LookState {
    Filter(Option<Filter>),
    Lut(Option<Lut>),
    Adjust(ColorAdjustments),
    Chroma(Option<ChromaKey>),
    Matte(Option<TrackMatte>),
}

impl LookState {
    fn capture(clip: &Clip, param: LookParam) -> Self {
        match param {
            LookParam::FilterIntensity => LookState::Filter(clip.filter.clone()),
            LookParam::LutIntensity => LookState::Lut(clip.lut.clone()),
            LookParam::ChromaStrength | LookParam::ChromaShadow => {
                LookState::Chroma(clip.chroma_key.clone())
            }
            LookParam::MatteSoftness => LookState::Matte(clip.matte.clone()),
            _ => LookState::Adjust(clip.adjust.clone()),
        }
    }

    fn restore(self, clip: &mut Clip) {
        match self {
            LookState::Filter(filter) => clip.filter = filter,
            LookState::Lut(lut) => clip.lut = lut,
            LookState::Adjust(adjust) => clip.adjust = adjust,
            LookState::Chroma(chroma) => clip.chroma_key = chroma,
            LookState::Matte(matte) => clip.matte = matte,
        }
    }
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
    Mask {
        index: usize,
        mask: Box<Mask>,
    },
    Style(Box<LayerStyles>),
    TextOrShape(Box<ClipSource>),
    Effect {
        effect_idx: usize,
        instance: EffectInstance,
    },
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
                Ok(ParamState::Look(Box::new(LookState::capture(clip, look))))
            }
            ClipParam::Mask { mask, .. } => {
                let index = mask as usize;
                let mask = clip.masks.get(index).cloned().ok_or_else(|| {
                    ModelError::InvalidParam(format!("mask index {index} out of range"))
                })?;
                Ok(ParamState::Mask {
                    index,
                    mask: Box::new(mask),
                })
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
            (ParamState::Look(look_state), ClipParam::Look { .. }) => look_state.restore(clip),
            (ParamState::Mask { index, mask }, ClipParam::Mask { .. }) => {
                if let Some(slot) = clip.masks.get_mut(index) {
                    *slot = *mask;
                }
            }
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
        let state = self.state;
        let current_state = ctx
            .project
            .edit_clip(clip_id, |clip| state.restore(clip, param))?;
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
    let clip_obj = ctx.project.require_clip(clip)?;
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
    let clip_obj = ctx.project.require_clip(clip)?;
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
    let clip_obj = ctx.project.require_clip(clip)?;
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
    let clip_obj = ctx.project.require_clip(clip)?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project.apply_easing_preset(clip, param, at, preset)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

pub fn set_keyframe_ease(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    at: RationalTime,
    in_ease: Option<TemporalEase>,
    out_ease: Option<TemporalEase>,
    hold: bool,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project
        .set_param_keyframe_ease(clip, param, at, in_ease, out_ease, hold)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

pub fn set_keyframe_roving(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: ClipParam,
    at: RationalTime,
    roving: bool,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let before = ParamState::capture(clip_obj, param)?;
    ctx.project
        .set_param_keyframe_roving(clip, param, at, roving)?;
    Ok(Box::new(RestoreParamAction {
        clip,
        param,
        state: before,
    }))
}

#[cfg(test)]
mod tests {
    use aria_commands::{Command, EditCommand};
    use aria_models::{
        ClipParam, Easing, Generator, ParamValue, Project, Rational, RationalTime, TemporalEase,
        TimeRange, TrackKind,
    };

    use crate::{Engine, EngineConfig};

    fn rt(v: i64) -> RationalTime {
        RationalTime::new(v, Rational::FPS_24)
    }

    #[test]
    fn set_keyframe_ease_and_roving_roundtrip_undo_and_redo() {
        let mut project = Project::new("kf", Rational::FPS_24);
        let track = project.add_track(TrackKind::Sticker, "S1");
        let clip = project
            .add_generated(
                track,
                Generator::SolidColor {
                    rgba: [255, 255, 255, 255],
                },
                TimeRange::at_rate(0, 120, Rational::FPS_24),
            )
            .unwrap();
        let mut engine = Engine::with_project(EngineConfig { undo_limit: 32 }, project).unwrap();

        for (tick, pos) in [(0, [0.0, 0.0]), (10, [1.0, 0.0]), (100, [4.0, 0.0])] {
            engine
                .apply(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param: ClipParam::Position,
                    at: rt(tick),
                    value: ParamValue::Vec2(pos),
                    easing: Easing::Linear,
                    tangents: None,
                }))
                .unwrap();
        }

        // Rove middle keyframe from tick 10 -> solved tick 25.
        engine
            .apply(Command::Edit(EditCommand::SetKeyframeRoving {
                clip,
                param: ClipParam::Position,
                at: rt(10),
                roving: true,
            }))
            .unwrap();
        let kfs = engine
            .project()
            .clip(clip)
            .unwrap()
            .transform
            .position
            .keyframes();
        assert_eq!(kfs[1].tick, 25);
        assert!(kfs[1].easing.is_roving());

        // Undo restores anchored tick 10; redo re-solves to 25.
        assert!(engine.undo());
        assert_eq!(
            engine
                .project()
                .clip(clip)
                .unwrap()
                .transform
                .position
                .keyframes()[1]
                .tick,
            10
        );
        assert!(engine.redo());
        assert_eq!(
            engine
                .project()
                .clip(clip)
                .unwrap()
                .transform
                .position
                .keyframes()[1]
                .tick,
            25
        );

        // Set temporal ease at tick 0 and verify undo/redo.
        engine
            .apply(Command::Edit(EditCommand::SetKeyframeEase {
                clip,
                param: ClipParam::Position,
                at: rt(0),
                in_ease: None,
                out_ease: Some(TemporalEase::new(0.0, 0.75)),
                hold: false,
            }))
            .unwrap();
        assert_eq!(
            engine
                .project()
                .clip(clip)
                .unwrap()
                .transform
                .position
                .keyframes()[0]
                .easing
                .out_ease(),
            Some(TemporalEase::new(0.0, 0.75))
        );
        assert!(engine.undo());
        assert_eq!(
            engine
                .project()
                .clip(clip)
                .unwrap()
                .transform
                .position
                .keyframes()[0]
                .easing
                .out_ease(),
            None
        );
        assert!(engine.redo());
        assert_eq!(
            engine
                .project()
                .clip(clip)
                .unwrap()
                .transform
                .position
                .keyframes()[0]
                .easing
                .out_ease(),
            Some(TemporalEase::new(0.0, 0.75))
        );

        // Missing keyframe at tick 55 is rejected without recording undo.
        let depth = engine.history_depth();
        assert!(
            engine
                .apply(Command::Edit(EditCommand::SetKeyframeEase {
                    clip,
                    param: ClipParam::Position,
                    at: rt(55),
                    in_ease: Some(TemporalEase::EASY_EASE),
                    out_ease: None,
                    hold: false,
                }))
                .is_err()
        );
        assert_eq!(engine.history_depth(), depth);
    }
}
