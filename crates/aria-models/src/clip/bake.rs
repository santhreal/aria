//! Render and replace: swapping a clip for a rendered intermediate, and
//! putting back what the render replaced.
//!
//! A clip with a deep effect stack is re-rendered on every frame of every
//! playback. Baking it once to a file and decoding that instead is the standard
//! answer, and the difficulty is the second half: an edit must be able to go
//! back, after having trimmed, moved, or split the baked clip.
//!
//! The rule that makes the return exact is that **the intermediate is the
//! clip's timeline span rendered one-to-one**. The start of the file is the tick
//! the clip started on when it was baked, so how far into the file the clip's
//! head now sits is how far into that stretch of programme it now starts —
//! whatever has happened to the clip since, and wherever on the timeline it has
//! been dragged to. [`Clip::unbaked`] maps that offset back through the
//! snapshot's own retiming with [`Clip::source_time_at`], which is the same
//! function the renderer used to produce the file.
//!
//! One-to-one is a claim about time, not about numbers. A picture intermediate
//! is written at the sequence rate and counts the same ticks the timeline does;
//! a sound intermediate has no frames and counts audio ticks, so the offset is
//! read as time rather than as a tick count.
//!
//! Two things are deliberately refused rather than approximated:
//!
//! - **A lane bar cannot be baked.** It has no picture of its own; it
//!   reprocesses what is beneath it, which is not a clip.
//! - **A retimed bake cannot be restored.** Composing a speed ramp applied
//!   after the bake with the ramp that was folded into it is not a
//!   multiplication, so the restore fails closed and names the retime.
//!
//! What a bake folds in is what the renderer applies to the clip: its
//! transform, crop, flips, effect chain, property drivers, mask, key, grade,
//! LUT, layer styles, motion blur, edge animations, retiming, and its audio
//! envelope. What it
//! cannot fold stays on the clip: the blend mode, the track matte, and the
//! parent all need the layers around this one, and stabilization is not
//! rendered at all. A baked clip keeps its parent: the file holds the clip
//! placed by its own transform, and the parent's frame places that picture.

use super::*;

impl Clip {
    /// Whether this clip is a rendered stand-in for something else.
    pub fn is_baked(&self) -> bool {
        self.bake.is_some()
    }

    /// Whether this clip's content is something a render can stand in for.
    ///
    /// Split out of [`Clip::baked`] because the render happens first: whoever
    /// is about to spend minutes encoding a file asks this before encoding it,
    /// and there is one answer rather than two.
    pub fn bakeable(&self) -> Result<(), ModelError> {
        if self.bake.is_some() {
            return Err(ModelError::InvalidParam(
                "that clip is already rendered; restore it before rendering it again".into(),
            ));
        }
        if let ClipSource::Generated(generator) = &self.content
            && !generator.spec().paints
        {
            return Err(ModelError::InvalidParam(
                "a lane bar has no picture of its own to render".into(),
            ));
        }
        // Generated, multicam and nested content is computed every frame, so a
        // render is worth having whatever else is on the clip. A media clip with
        // nothing on it is the one case where the render would be a re-encode of
        // the file that is already there: the same picture, one generation down,
        // for no saving at all.
        if matches!(self.content, ClipSource::Media { .. }) && !self.is_dressed() {
            return Err(ModelError::InvalidParam(
                "nothing is applied to that clip, so rendering it would only re-encode it".into(),
            ));
        }
        Ok(())
    }

    /// Whether the renderer applies anything to this clip beyond decoding it.
    ///
    /// Answered by the bake itself rather than by a list of fields: what a bake
    /// spends is exactly what makes a clip worth baking, so the two cannot
    /// disagree, and a look field added later is covered by whichever of them
    /// clears it.
    pub fn is_dressed(&self) -> bool {
        self.stripped() != *self
    }

    /// This clip with everything the renderer folds into the picture cleared.
    fn stripped(&self) -> Self {
        let mut bare = Clip::clone(self);
        bare.freeze_frame = false;
        bare.transform = AnimatedTransform::identity();
        bare.crop = default_crop();
        bare.flip_h = false;
        bare.flip_v = false;
        bare.effects.clear();
        bare.drivers.clear();
        bare.masks.clear();
        bare.chroma_key = None;
        bare.filter = None;
        bare.lut = None;
        bare.adjust = crate::look::ColorAdjustments::default();
        bare.styles = crate::look::LayerStyles::default();
        bare.motion_blur = crate::look::MotionBlur::default();
        bare.animation_in = None;
        bare.animation_out = None;
        bare.animation_combo = None;
        bare.speed = unit_speed();
        bare.reversed = false;
        bare.speed_curve = default_speed_curve();
        bare.preserve_pitch = default_preserve_pitch();
        // The file carries the mixed sound, so the envelope that shaped it is
        // spent. Beat positions were ticks into the original media and mean
        // nothing in the intermediate.
        bare.volume = default_volume();
        bare.pan = default_pan();
        bare.fade_in = 0;
        bare.fade_out = 0;
        bare.denoise = false;
        bare.beats.clear();
        bare.text_editable = false;
        bare
    }

    /// This clip as a decode of `media` over `source`, keeping what it replaced.
    ///
    /// `source` is the window into the rendered file, which the caller has
    /// already checked runs at the sequence rate.
    pub fn baked(&self, media: MediaId, source: TimeRange) -> Result<Self, ModelError> {
        self.bakeable()?;
        check_same_rate(source.start.rate, source.duration.rate)?;
        if source.is_empty() {
            return Err(ModelError::InvalidRange);
        }

        let mut baked = self.stripped();
        baked.bake = Some(Box::new(Clip::clone(self)));
        // The property the bar drew may be one the bake spent; the original
        // keeps it for an unbake.
        baked.line_param = None;
        // Everything the renderer applied is in the file's pixels now.
        baked.content = ClipSource::Media { media, source };
        Ok(baked)
    }

    /// What this baked clip replaced, over the span the baked clip now covers.
    ///
    /// Keeps the clip's identity, its place on the timeline, and its link, and
    /// takes everything else back from the snapshot. A head trim on the baked
    /// clip becomes the same head trim on the original: the offset into the
    /// rendered file is the offset into the stretch of programme it holds, so
    /// the original's source in-point is read from its own retiming at that
    /// position and its clip-local animation is rebased by the same amount.
    pub fn unbaked(&self) -> Result<Self, ModelError> {
        let Some(original) = self.bake.as_deref() else {
            return Err(ModelError::InvalidParam(
                "that clip was not rendered and replaced".into(),
            ));
        };
        if self.speed != unit_speed() || self.reversed || self.has_speed_curve() {
            return Err(ModelError::InvalidParam(
                "that clip was retimed after it was rendered; undo the retime before restoring it"
                    .into(),
            ));
        }
        let Some(baked_source) = self.source_range() else {
            return Err(ModelError::InvalidParam(
                "a rendered clip decodes a file; this one does not".into(),
            ));
        };
        // How far into the rendered file the clip's head now sits, in the
        // timeline's own ticks. The file is one-to-one with the stretch of
        // programme it holds, but it counts in its own rate: a picture
        // intermediate at the sequence rate is the same number, a sound
        // intermediate counting audio ticks is not.
        let into_bake = crate::time::resample(baked_source.start, self.timeline.start.rate).value;

        let mut restored = Clip::clone(original);
        restored.id = self.id;
        restored.link = self.link;
        restored.group = self.group;
        restored.enabled = self.enabled;
        restored.timeline = self.timeline;
        restored.bake = None;
        if let Some(source) = original.source_range() {
            let at = RationalTime::new(
                original.timeline.start.value + into_bake,
                original.timeline.start.rate,
            );
            let start = original
                .source_time_at(at)?
                .ok_or(ModelError::SourceOutOfBounds)?;
            let span = RationalTime::new(
                original.scale_by_speed(self.timeline.duration.value),
                original.timeline.duration.rate,
            );
            let available = crate::time::time_sub(&source.end()?, &start)?;
            let duration = crate::time::resample(span, source.start.rate)
                .value
                .clamp(1, available.value.max(1));
            let mapped = TimeRange::at_rate(start.value, duration, source.start.rate);
            match &mut restored.content {
                ClipSource::Media { source, .. }
                | ClipSource::Multicam { source, .. }
                | ClipSource::Sequence { source, .. } => *source = mapped,
                ClipSource::Generated(_) => {}
            }
        }
        if into_bake != 0 {
            // The original's animation was addressed from where it used to
            // start; the restored clip starts that far in.
            restored.shift_timeline_params(-into_bake)?;
        }
        Ok(restored)
    }
}
