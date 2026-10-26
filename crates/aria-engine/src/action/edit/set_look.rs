//! Look edits: blend mode, motion blur, layer styles, chroma key, track
//! matte, stabilization, filter, LUT, adjustments, animations, and the audio
//! role tag.
//!
//! All look edits use fine-grained undo inverses restoring only the mutated
//! property, preserving other concurrent or unrelated clip changes.

use aria_models::{
    AnimationRef, AnimationSlot, AudioRole, BlendMode, ChromaKey, ClipId, ColorAdjustments, Filter,
    LayerStyles, Lut, MotionBlur, StabilizeLevel, TrackMatte,
};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// The three animation slots as one value.
///
/// A combo animation evicts the entrance and the exit, so the inverse of
/// setting any one slot is the whole group. Snapshotting a single slot loses the
/// two a combo displaced, and writing a single slot back can produce a
/// combination the model refuses — a combo standing beside an entrance — because
/// the restore assigns the field directly rather than going through
/// `set_clip_animation`. Carrying all three means the value written back is a
/// state the model already accepted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationSlots {
    pub entrance: Option<AnimationRef>,
    pub exit: Option<AnimationRef>,
    pub combo: Option<AnimationRef>,
}

impl AnimationSlots {
    fn of(clip: &aria_models::Clip) -> Self {
        Self {
            entrance: clip.animation_in.clone(),
            exit: clip.animation_out.clone(),
            combo: clip.animation_combo.clone(),
        }
    }

    fn write_to(self, clip: &mut aria_models::Clip) {
        clip.animation_in = self.entrance;
        clip.animation_out = self.exit;
        clip.animation_combo = self.combo;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LookProperty {
    BlendMode(BlendMode),
    MotionBlur(MotionBlur),
    LayerStyles(LayerStyles),
    ChromaKey(Option<ChromaKey>),
    Matte(Option<TrackMatte>),
    Stabilize(Option<StabilizeLevel>),
    Filter(Option<Filter>),
    Lut(Option<Lut>),
    Adjustments(ColorAdjustments),
    Animations(AnimationSlots),
    AudioRole(Option<AudioRole>),
}

pub struct RestoreLookPropertyAction {
    pub clip: ClipId,
    pub property: LookProperty,
}

impl EditAction for RestoreLookPropertyAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let Self {
            clip: clip_id,
            property,
        } = *self;
        let current = ctx.project.edit_clip(clip_id, |clip| {
            Ok(match property {
                LookProperty::BlendMode(mode) => {
                    LookProperty::BlendMode(std::mem::replace(&mut clip.blend_mode, mode))
                }
                LookProperty::MotionBlur(mb) => {
                    LookProperty::MotionBlur(std::mem::replace(&mut clip.motion_blur, mb))
                }
                LookProperty::LayerStyles(styles) => {
                    LookProperty::LayerStyles(std::mem::replace(&mut clip.styles, styles))
                }
                LookProperty::ChromaKey(chroma) => {
                    LookProperty::ChromaKey(std::mem::replace(&mut clip.chroma_key, chroma))
                }
                LookProperty::Matte(matte) => {
                    LookProperty::Matte(std::mem::replace(&mut clip.matte, matte))
                }
                LookProperty::Stabilize(stab) => {
                    LookProperty::Stabilize(std::mem::replace(&mut clip.stabilize, stab))
                }
                LookProperty::Filter(filter) => {
                    LookProperty::Filter(std::mem::replace(&mut clip.filter, filter))
                }
                LookProperty::Lut(lut) => LookProperty::Lut(std::mem::replace(&mut clip.lut, lut)),
                LookProperty::Adjustments(adjust) => {
                    LookProperty::Adjustments(std::mem::replace(&mut clip.adjust, adjust))
                }
                LookProperty::Animations(slots) => {
                    let prev = AnimationSlots::of(clip);
                    slots.write_to(clip);
                    LookProperty::Animations(prev)
                }
                LookProperty::AudioRole(role) => {
                    LookProperty::AudioRole(std::mem::replace(&mut clip.audio_role, role))
                }
            })
        })?;

        Ok(Box::new(RestoreLookPropertyAction {
            clip: clip_id,
            property: current,
        }))
    }
}

pub fn set_blend_mode(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    mode: BlendMode,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.blend_mode;
    ctx.project.set_blend_mode(clip, mode)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::BlendMode(prev),
    }))
}

pub fn set_motion_blur(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    motion_blur: MotionBlur,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.motion_blur;
    ctx.project.set_motion_blur(clip, motion_blur)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::MotionBlur(prev),
    }))
}

pub fn set_layer_styles(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    styles: LayerStyles,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.styles.clone();
    ctx.project.set_layer_styles(clip, styles)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::LayerStyles(prev),
    }))
}

pub fn set_chroma(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    chroma: Option<ChromaKey>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.chroma_key.clone();
    ctx.project.set_clip_chroma_key(clip, chroma)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::ChromaKey(prev),
    }))
}

pub fn set_matte(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    matte: Option<TrackMatte>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.matte.clone();
    ctx.project.set_clip_matte(clip, matte)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Matte(prev),
    }))
}

pub fn set_stabilize(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    stabilize: Option<StabilizeLevel>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.stabilize;
    ctx.project.set_clip_stabilize(clip, stabilize)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Stabilize(prev),
    }))
}

pub fn set_filter(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    filter: Option<Filter>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.filter.clone();
    ctx.project.set_clip_filter(clip, filter)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Filter(prev),
    }))
}

pub fn set_lut(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    lut: Option<Lut>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.lut.clone();
    ctx.project.set_clip_lut(clip, lut)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Lut(prev),
    }))
}

pub fn set_adjustments(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    adjust: ColorAdjustments,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.adjust.clone();
    ctx.project.set_clip_adjustments(clip, adjust)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Adjustments(prev),
    }))
}

pub fn set_animation(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    slot: AnimationSlot,
    animation: Option<AnimationRef>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let prev = AnimationSlots::of(ctx.project.require_clip(clip)?);
    ctx.project.set_clip_animation(clip, slot, animation)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::Animations(prev),
    }))
}

pub fn set_audio_role(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    role: Option<AudioRole>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev = clip_obj.audio_role;
    ctx.project.set_clip_audio_role(clip, role)?;
    Ok(Box::new(RestoreLookPropertyAction {
        clip,
        property: LookProperty::AudioRole(prev),
    }))
}
