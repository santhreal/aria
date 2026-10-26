use aria_models::{ClipId, CropRect, Param, RationalTime};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Restore only the crop and flip framing of a clip.
pub struct RestoreCropAction {
    pub clip: ClipId,
    pub crop: Param<CropRect>,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl EditAction for RestoreCropAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let Self {
            clip: clip_id,
            crop,
            flip_h,
            flip_v,
        } = *self;
        let (crop, flip_h, flip_v) = ctx.project.edit_clip(clip_id, |clip| {
            Ok((
                std::mem::replace(&mut clip.crop, crop),
                std::mem::replace(&mut clip.flip_h, flip_h),
                std::mem::replace(&mut clip.flip_v, flip_v),
            ))
        })?;

        Ok(Box::new(RestoreCropAction {
            clip: clip_id,
            crop,
            flip_h,
            flip_v,
        }))
    }
}

/// Set a clip's framing (CapCut crop + flips, M1). The model validates the
/// rect and the visual-track requirement.
///
/// `at: Some(playhead)` keyframes an already-animated crop (M2 compose).
pub fn set_crop(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    crop: CropRect,
    flip_h: bool,
    flip_v: bool,
    at: Option<RationalTime>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx.project.require_clip(clip)?;
    let prev_crop = clip_obj.crop.clone();
    let prev_flip_h = clip_obj.flip_h;
    let prev_flip_v = clip_obj.flip_v;

    ctx.project.set_clip_crop(clip, crop, flip_h, flip_v, at)?;
    Ok(Box::new(RestoreCropAction {
        clip,
        crop: prev_crop,
        flip_h: prev_flip_h,
        flip_v: prev_flip_v,
    }))
}
