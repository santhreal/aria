//! Every clip-relative keyframe curve a clip carries, walked once.
//!
//! A split, a roll and a rate change all have to move a clip's animation with
//! the clip. Each of those used to name the carriers it knew about at the call
//! site, and the two lists had already drifted: the rebase moved the transform,
//! the volume, the pan, the crop and the effects, while the resample moved those
//! plus the layer styles. Neither moved the grade, the mask, the chroma key, the
//! matte, the filter or the LUT, so splitting a clip with an animated grade slid
//! that grade against the picture — silently, because a curve that is merely in
//! the wrong place still renders.
//!
//! One walk answers it now. A type that carries curves implements
//! [`ParamWalk`] and lists them once; the rebase and the resample are two
//! visitors over that one list, so a carrier cannot be handled by one operation
//! and forgotten by the other.
//!
//! # Fail-by-default
//!
//! A curve added to a look type and not added to its `walk_params` is not a
//! compile error — Rust cannot see the field the list omits. It is a test
//! failure: `param_walk_tests` animates a clip through its serialized form,
//! rebases it, and asserts that every `"kf"` array in the whole document moved,
//! whatever nesting it was found at. A new carrier appears in that document by
//! existing, so it is covered the moment it is serializable, and the declared
//! exceptions are named in that file with the reason each one holds still.

use crate::ModelError;
use crate::param::Param;
use crate::time::Rational;

/// One animatable curve, addressed without naming the type of its value.
pub(crate) trait ParamCurve {
    /// `Ok` iff every keyframe on this curve can move by `delta` without
    /// running off the end of time.
    fn shift_fits(&self, delta: i64) -> Result<(), ModelError>;

    fn shift(&mut self, delta: i64) -> Result<(), ModelError>;

    fn resample(&mut self, old_rate: Rational, new_rate: Rational);
}

impl<T: Clone> ParamCurve for Param<T> {
    fn shift_fits(&self, delta: i64) -> Result<(), ModelError> {
        for key in self.keyframes() {
            key.tick
                .checked_add(delta)
                .ok_or(ModelError::TimeOverflow)?;
        }
        Ok(())
    }

    fn shift(&mut self, delta: i64) -> Result<(), ModelError> {
        self.shift_ticks(delta)
    }

    fn resample(&mut self, old_rate: Rational, new_rate: Rational) {
        self.resample_ticks(old_rate, new_rate);
    }
}

/// What a visitor does to one curve.
///
/// Boxed as a trait object rather than a generic so [`ParamWalk`] stays object
/// safe and a walk can nest into a carrier held behind `dyn`.
type Visit<'a> = &'a mut dyn FnMut(&mut dyn ParamCurve) -> Result<(), ModelError>;

/// Every clip-relative curve a value carries, in one list per type.
pub(crate) trait ParamWalk {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError>;
}

impl ParamWalk for crate::clip::AnimatedTransform {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.position)?;
        visit(&mut self.anchor_point)?;
        visit(&mut self.scale)?;
        visit(&mut self.rotation)?;
        visit(&mut self.opacity)
    }
}

impl ParamWalk for crate::look::ColorAdjustments {
    /// Derived from the slider table, so an adjustment added there rides along
    /// with no edit here.
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        for slider in Self::SLIDERS {
            visit((slider.get_mut)(self))?;
        }
        Ok(())
    }
}

impl ParamWalk for crate::look::Mask {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.feather)?;
        visit(&mut self.center)?;
        visit(&mut self.size)?;
        visit(&mut self.rotation)?;
        visit(&mut self.roundness)
    }
}

impl ParamWalk for crate::look::ChromaKey {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.strength)?;
        visit(&mut self.shadow)
    }
}

impl ParamWalk for crate::look::TrackMatte {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.softness)
    }
}

impl ParamWalk for crate::look::Filter {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.intensity)
    }
}

impl ParamWalk for crate::look::Lut {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.intensity)
    }
}

impl ParamWalk for crate::look::LayerStyles {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        if let Some(shadow) = &mut self.shadow {
            visit(&mut shadow.rgba)?;
            visit(&mut shadow.offset)?;
            visit(&mut shadow.blur)?;
        }
        if let Some(glow) = &mut self.glow {
            visit(&mut glow.rgba)?;
            visit(&mut glow.radius)?;
            visit(&mut glow.intensity)?;
        }
        if let Some(outline) = &mut self.outline {
            visit(&mut outline.rgba)?;
            visit(&mut outline.width)?;
        }
        if let Some(background) = &mut self.background {
            visit(&mut background.rgba)?;
            visit(&mut background.padding)?;
            visit(&mut background.radius)?;
        }
        Ok(())
    }
}

impl ParamWalk for crate::effects::EffectInstance {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        for param in self.params.values_mut() {
            visit(param)?;
        }
        for param in self.color_params.values_mut() {
            visit(param)?;
        }
        for param in self.vec2_params.values_mut() {
            visit(param)?;
        }
        Ok(())
    }
}

impl ParamWalk for crate::audio_effects::AudioEffectInstance {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        for param in self.params.values_mut() {
            visit(param)?;
        }
        Ok(())
    }
}

impl ParamWalk for crate::clip::TextStyle {
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        visit(&mut self.size)?;
        visit(&mut self.fill)?;
        visit(&mut self.letter_spacing)?;
        visit(&mut self.line_spacing)?;
        if let Some(stroke) = &mut self.stroke {
            visit(&mut stroke.rgba)?;
            visit(&mut stroke.width)?;
        }
        // The background card was the carrier the old per-variant rebase
        // forgot: a title with an animated card colour or corner radius kept
        // its curve where the split found it, so the card animated against the
        // words it sits behind.
        if let Some(background) = &mut self.background {
            visit(&mut background.rgba)?;
            visit(&mut background.radius)?;
        }
        if let Some(shadow) = &mut self.shadow {
            visit(&mut shadow.rgba)?;
            visit(&mut shadow.blur)?;
            visit(&mut shadow.distance)?;
        }
        Ok(())
    }
}

impl ParamWalk for crate::clip::Shape {
    /// Exhaustive: a geometry that gains an animatable dimension has to say so
    /// here, because the compiler asks it to.
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        match self {
            Self::Star { inner_ratio, .. } => visit(inner_ratio),
            Self::Rectangle
            | Self::Ellipse
            | Self::Polygon { .. }
            | Self::Line
            | Self::Arrow
            | Self::Heart
            | Self::Path(_) => Ok(()),
        }
    }
}

impl ParamWalk for crate::clip::Generator {
    /// Every curve generated content carries, per kind.
    ///
    /// Exhaustive rather than defaulted: a generator that paints something new
    /// does not compile until it says which of its parameters travel with the
    /// clip. The rebase and the resample are the same two visitors that walk
    /// the rest of the clip, so generated content cannot be rebased by one and
    /// missed by the other, which is what a second hand-written list here used
    /// to allow.
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        match self {
            Self::Text { style, .. } => style.walk_params(visit),
            Self::Shape {
                shape,
                rgba,
                width,
                height,
                corner_radius,
                stroke,
            } => {
                visit(rgba)?;
                visit(width)?;
                visit(height)?;
                visit(corner_radius)?;
                shape.walk_params(visit)?;
                if let Some(stroke) = stroke {
                    visit(&mut stroke.rgba)?;
                    visit(&mut stroke.width)?;
                }
                Ok(())
            }
            // A colour, a catalog id, a file path and a lane bar: content with
            // nothing addressed in time.
            Self::SolidColor { .. }
            | Self::Sticker { .. }
            | Self::Lottie { .. }
            | Self::Effect
            | Self::Filter
            | Self::Adjustment => Ok(()),
        }
    }
}

impl ParamWalk for crate::clip::Clip {
    /// Every curve addressed in clip-relative ticks.
    ///
    /// [`Clip::speed_curve`] is deliberately absent: its keys live on the
    /// normalized `SPEED_CURVE_SCALE` domain, so a trim rescales the ramp's
    /// shape instead of sliding its keys, and a split segments it.
    ///
    /// [`Clip::speed_curve`]: crate::Clip::speed_curve
    fn walk_params(&mut self, visit: Visit<'_>) -> Result<(), ModelError> {
        self.transform.walk_params(visit)?;
        visit(&mut self.volume)?;
        visit(&mut self.pan)?;
        visit(&mut self.crop)?;
        for effect in &mut self.effects {
            effect.walk_params(visit)?;
        }
        for insert in &mut self.audio_effects {
            insert.walk_params(visit)?;
        }
        self.styles.walk_params(visit)?;
        self.adjust.walk_params(visit)?;
        if let Some(mask) = &mut self.mask {
            mask.walk_params(visit)?;
        }
        if let Some(chroma) = &mut self.chroma_key {
            chroma.walk_params(visit)?;
        }
        if let Some(matte) = &mut self.matte {
            matte.walk_params(visit)?;
        }
        if let Some(filter) = &mut self.filter {
            filter.walk_params(visit)?;
        }
        if let Some(lut) = &mut self.lut {
            lut.walk_params(visit)?;
        }
        if let crate::clip::ClipSource::Generated(generator) = &mut self.content {
            generator.walk_params(visit)?;
        }
        // What a render-and-replace put aside is the same clip at the same
        // origin, so its curves are clip-relative too: an unbake after a split
        // has to land on the picture the split produced.
        if let Some(bake) = &mut self.bake {
            bake.walk_params(visit)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "param_walk_tests.rs"]
mod shift_carriers;
