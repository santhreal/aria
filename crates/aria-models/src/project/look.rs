use crate::clip::{ClipSource, Generator, index_after_move, index_after_remove};
use crate::error::ModelError;
use crate::ids::{ClipId, TrackId};
use crate::look::mask::mask_mut;
use crate::look::{
    AnimationRef, AnimationSlot, AudioRole, BlendMode, ChromaKey, ColorAdjustments, Filter,
    LayerStyles, Lut, MAX_CLIP_MASKS, Mask, MotionBlur, StabilizeLevel, TrackMatte, animation_spec,
};
use crate::track::TrackKind;

use super::Project;

impl Project {
    // --- Phase I look properties (persisted + validated, render-neutral) ----

    /// The kind of the track hosting `clip_id`, or the unknown-clip error.
    fn track_kind_of(&self, clip_id: ClipId) -> Result<(TrackId, TrackKind), ModelError> {
        let track_id = self
            .timeline()
            .track_of(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let kind = self.timeline().require_track(track_id)?.kind;
        Ok((track_id, kind))
    }

    /// Guard for edits that only make sense where pixels show.
    fn require_visual(&self, clip_id: ClipId, what: &str) -> Result<(), ModelError> {
        let (track_id, kind) = self.track_kind_of(clip_id)?;
        if !kind.is_visual() {
            return Err(ModelError::IncompatibleTrackKind {
                track: track_id,
                kind,
            });
        }
        let _ = what;
        Ok(())
    }

    /// Guard for edits that need decoded source pixels (mask, chroma, …):
    /// a media-backed clip on a visual lane.
    fn require_visual_media(&self, clip_id: ClipId, what: &str) -> Result<(), ModelError> {
        self.require_visual(clip_id, what)?;
        let clip = self.timeline().require_clip(clip_id)?;
        if clip.is_generated() {
            return Err(ModelError::InvalidParam(format!(
                "{what} requires a media-backed clip"
            )));
        }
        Ok(())
    }

    /// Append `mask` to the bottom of a clip's mask stack and return its
    /// index. Visual clips only, footage or generated; the mask is validated
    /// first, and a stack holds at most [`MAX_CLIP_MASKS`].
    pub fn add_clip_mask(&mut self, clip_id: ClipId, mask: Mask) -> Result<u32, ModelError> {
        mask.validate()?;
        self.require_visual(clip_id, "a mask")?;
        self.edit_clip(clip_id, |clip| {
            if clip.masks.len() >= MAX_CLIP_MASKS {
                return Err(ModelError::InvalidParam(format!(
                    "clip {clip_id} already carries {MAX_CLIP_MASKS} masks"
                )));
            }
            clip.masks.push(mask);
            Ok((clip.masks.len() - 1) as u32)
        })
    }

    /// Replace the mask at `index` in a clip's stack. Drivers on its
    /// properties stay attached.
    pub fn set_clip_mask(
        &mut self,
        clip_id: ClipId,
        index: u32,
        mask: Mask,
    ) -> Result<(), ModelError> {
        mask.validate()?;
        self.require_visual(clip_id, "a mask")?;
        self.edit_clip(clip_id, |clip| {
            *mask_mut(&mut clip.masks, index)? = mask;
            Ok(())
        })
    }

    /// Remove the mask at `index` from a clip's stack and return it. Drivers on
    /// its properties are dropped; drivers on the masks below it follow them up
    /// one index.
    pub fn remove_clip_mask(&mut self, clip_id: ClipId, index: u32) -> Result<Mask, ModelError> {
        self.edit_clip(clip_id, |clip| {
            mask_mut(&mut clip.masks, index)?;
            let removed = clip.masks.remove(index as usize);
            clip.remap_mask_refs(index_after_remove(index));
            Ok(removed)
        })
    }

    /// Move one mask within a clip's stack. Both indices address the stack
    /// before the move; `to_index` is the mask's index after it. Moving a mask
    /// to its current index is a valid no-op. Drivers follow their masks.
    pub fn move_clip_mask(
        &mut self,
        clip_id: ClipId,
        from_index: u32,
        to_index: u32,
    ) -> Result<(), ModelError> {
        self.edit_clip(clip_id, |clip| {
            mask_mut(&mut clip.masks, from_index)?;
            mask_mut(&mut clip.masks, to_index)?;
            if from_index == to_index {
                return Ok(());
            }
            let mask = clip.masks.remove(from_index as usize);
            clip.masks.insert(to_index as usize, mask);
            clip.remap_mask_refs(index_after_move(from_index, to_index));
            Ok(())
        })
    }

    /// Set how the clip composites over the stack below (M-blend). Visual
    /// clips only; audio has nothing to blend.
    pub fn set_blend_mode(&mut self, clip_id: ClipId, mode: BlendMode) -> Result<(), ModelError> {
        self.require_visual(clip_id, "a blend mode")?;
        self.edit_clip(clip_id, |clip| {
            clip.blend_mode = mode;
            Ok(())
        })
    }

    /// Set per-clip motion blur (temporal supersampling of the animated
    /// transform). Visual clips only; validated before store. Params are
    /// plain values — not animatable.
    pub fn set_motion_blur(
        &mut self,
        clip_id: ClipId,
        motion_blur: MotionBlur,
    ) -> Result<(), ModelError> {
        self.require_visual(clip_id, "motion blur")?;
        motion_blur.validate()?;
        self.edit_clip(clip_id, |clip| {
            clip.motion_blur = motion_blur;
            Ok(())
        })
    }

    /// Set layer-quad styles (shadow/glow/outline/background) on a visual
    /// clip. Empty styles clear every block; values are validated first.
    pub fn set_layer_styles(
        &mut self,
        clip_id: ClipId,
        styles: LayerStyles,
    ) -> Result<(), ModelError> {
        self.require_visual(clip_id, "layer styles")?;
        styles.validate()?;
        self.edit_clip(clip_id, |clip| {
            clip.styles = styles;
            Ok(())
        })
    }

    /// Set (or clear) chroma keying (CapCut chroma key, Phase I —
    /// render-neutral). Media-backed visual clips only; strength/shadow
    /// validated to `0..=1`.
    pub fn set_clip_chroma_key(
        &mut self,
        clip_id: ClipId,
        chroma: Option<ChromaKey>,
    ) -> Result<(), ModelError> {
        if let Some(chroma) = &chroma {
            chroma.validate()?;
        }
        self.require_visual_media(clip_id, "chroma key")?;
        self.edit_clip(clip_id, |clip| {
            clip.chroma_key = chroma;
            Ok(())
        })
    }

    /// Set (or with `None` clear) a clip's track matte: the layer immediately
    /// above it becomes its stencil instead of being drawn. Visual clips,
    /// generated ones included — a shape or a title is the usual matte, and the
    /// clip that *takes* a matte is as often a solid as a piece of media.
    pub fn set_clip_matte(
        &mut self,
        clip_id: ClipId,
        matte: Option<TrackMatte>,
    ) -> Result<(), ModelError> {
        if let Some(matte) = &matte {
            matte.validate()?;
        }
        self.require_visual(clip_id, "a track matte")?;
        self.edit_clip(clip_id, |clip| {
            clip.matte = matte;
            Ok(())
        })
    }

    /// Set (or clear) stabilization (CapCut stabilize, Phase I —
    /// render-neutral). Media-backed *video* clips only: stills have no
    /// motion to smooth.
    pub fn set_clip_stabilize(
        &mut self,
        clip_id: ClipId,
        stabilize: Option<StabilizeLevel>,
    ) -> Result<(), ModelError> {
        self.require_visual_media(clip_id, "stabilization")?;
        let clip = self.timeline().require_clip(clip_id)?;
        if let ClipSource::Media { media, .. } = &clip.content
            && self.media(*media).is_some_and(|m| m.is_image)
        {
            return Err(ModelError::InvalidParam(
                "stabilization requires video (stills have no motion)".into(),
            ));
        }
        self.edit_clip(clip_id, |clip| {
            clip.stabilize = stabilize;
            Ok(())
        })
    }

    /// Set (or clear) a color-grade filter preset (CapCut filters, Phase I —
    /// render-neutral). Any visual clip, including `Generator::Filter` lane
    /// bars; the id must exist in the filter catalog.
    pub fn set_clip_filter(
        &mut self,
        clip_id: ClipId,
        filter: Option<Filter>,
    ) -> Result<(), ModelError> {
        if let Some(filter) = &filter {
            filter.validate()?;
        }
        self.require_visual(clip_id, "a filter")?;
        self.edit_clip(clip_id, |clip| {
            clip.filter = filter;
            Ok(())
        })
    }

    /// Set (or clear) a `.cube` 3D LUT (applied after filter + adjust). Any
    /// visual clip, including `Generator::Filter` lane bars, which apply it
    /// to everything composited beneath them.
    pub fn set_clip_lut(&mut self, clip_id: ClipId, lut: Option<Lut>) -> Result<(), ModelError> {
        if let Some(lut) = &lut {
            lut.validate()?;
        }
        self.require_visual(clip_id, "a LUT")?;
        self.edit_clip(clip_id, |clip| {
            clip.lut = lut;
            Ok(())
        })
    }

    /// Set a clip's manual color grade (CapCut adjust, Phase I —
    /// render-neutral). Any visual clip, including `Generator::Adjustment`
    /// lane bars; neutral values simply clear the grade.
    pub fn set_clip_adjustments(
        &mut self,
        clip_id: ClipId,
        adjust: ColorAdjustments,
    ) -> Result<(), ModelError> {
        adjust.validate()?;
        self.require_visual(clip_id, "adjustments")?;
        self.edit_clip(clip_id, |clip| {
            clip.adjust = adjust;
            Ok(())
        })
    }

    /// Set (or clear) one animation slot (CapCut animation In / Out / Combo,
    /// Phase I — render-neutral). Any visual clip. The preset must exist in
    /// the catalog, match the slot, and — for `text_only` presets — sit on a
    /// text clip. CapCut exclusivity is enforced here so every platform
    /// agrees: setting In or Out clears a combo, setting a combo clears both
    /// sides.
    pub fn set_clip_animation(
        &mut self,
        clip_id: ClipId,
        slot: AnimationSlot,
        animation: Option<AnimationRef>,
    ) -> Result<(), ModelError> {
        self.require_visual(clip_id, "animations")?;
        let animation = if let Some(animation) = animation {
            let spec = animation_spec(&animation.id).ok_or_else(|| {
                ModelError::InvalidParam(format!("unknown animation '{}'", animation.id))
            })?;
            if spec.slot != slot {
                return Err(ModelError::InvalidParam(format!(
                    "animation '{}' does not fit that slot",
                    animation.id
                )));
            }
            if spec.text_only {
                let clip = self.timeline().require_clip(clip_id)?;
                if !matches!(clip.content, ClipSource::Generated(Generator::Text { .. })) {
                    return Err(ModelError::InvalidParam(format!(
                        "animation '{}' is a text preset",
                        animation.id
                    )));
                }
            }
            Some(animation.normalized_for(spec)?)
        } else {
            None
        };
        self.edit_clip(clip_id, |clip| {
            match slot {
                AnimationSlot::In => {
                    clip.animation_in = animation;
                    if clip.animation_in.is_some() {
                        clip.animation_combo = None;
                    }
                }
                AnimationSlot::Out => {
                    clip.animation_out = animation;
                    if clip.animation_out.is_some() {
                        clip.animation_combo = None;
                    }
                }
                AnimationSlot::Combo => {
                    clip.animation_combo = animation;
                    if clip.animation_combo.is_some() {
                        clip.animation_in = None;
                        clip.animation_out = None;
                    }
                }
            }
            Ok(())
        })
    }

    /// Tag (or untag) what an audio-lane clip *is* (music / sound FX /
    /// voiceover / extracted, Phase I). Clips on audio tracks only.
    pub fn set_clip_audio_role(
        &mut self,
        clip_id: ClipId,
        role: Option<AudioRole>,
    ) -> Result<(), ModelError> {
        let (track_id, kind) = self.track_kind_of(clip_id)?;
        if kind != TrackKind::Audio {
            return Err(ModelError::IncompatibleTrackKind {
                track: track_id,
                kind,
            });
        }
        self.edit_clip(clip_id, |clip| {
            clip.audio_role = role;
            Ok(())
        })
    }
}
