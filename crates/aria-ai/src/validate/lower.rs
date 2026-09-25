use super::*;

/// Lower the agent-facing property selector to the model selector. Effect
/// parameters retain the wire's stable catalog names and resolve to their
/// instance slot here.
pub(super) fn clip_param(
    param: &WireClipParam,
    clip: &Clip,
    wire_clip: u64,
) -> Result<ClipParam, Rejection> {
    use crate::wire::{WireLookParam, WireShapeParam, WireStyleParam, WireTextParam};

    Ok(match param {
        WireClipParam::Position => ClipParam::Position,
        WireClipParam::AnchorPoint => ClipParam::AnchorPoint,
        WireClipParam::Scale => ClipParam::Scale,
        WireClipParam::Rotation => ClipParam::Rotation,
        WireClipParam::Opacity => ClipParam::Opacity,
        WireClipParam::Crop => ClipParam::Crop,
        WireClipParam::Volume => ClipParam::Volume,
        WireClipParam::Pan => ClipParam::Pan,
        WireClipParam::Speed => ClipParam::Speed,
        WireClipParam::Effect { index, param } => {
            let (effect, slot, _) = effect_param_slot(clip, *index, param, wire_clip)?;
            ClipParam::Effect {
                effect: effect as u32,
                param: slot as u32,
            }
        }
        WireClipParam::Shape { param } => ClipParam::Shape {
            param: match param {
                WireShapeParam::Width => aria_models::ShapeParam::Width,
                WireShapeParam::Height => aria_models::ShapeParam::Height,
                WireShapeParam::CornerRadius => aria_models::ShapeParam::CornerRadius,
                WireShapeParam::InnerRatio => aria_models::ShapeParam::InnerRatio,
                WireShapeParam::Fill => aria_models::ShapeParam::Fill,
                WireShapeParam::StrokeColor => aria_models::ShapeParam::StrokeColor,
                WireShapeParam::StrokeWidth => aria_models::ShapeParam::StrokeWidth,
            },
        },
        WireClipParam::Text { param } => ClipParam::Text {
            param: match param {
                WireTextParam::Size => aria_models::TextParam::Size,
                WireTextParam::Fill => aria_models::TextParam::Fill,
                WireTextParam::LetterSpacing => aria_models::TextParam::LetterSpacing,
                WireTextParam::LineSpacing => aria_models::TextParam::LineSpacing,
                WireTextParam::StrokeWidth => aria_models::TextParam::StrokeWidth,
                WireTextParam::StrokeColor => aria_models::TextParam::StrokeColor,
                WireTextParam::ShadowBlur => aria_models::TextParam::ShadowBlur,
                WireTextParam::ShadowDistance => aria_models::TextParam::ShadowDistance,
                WireTextParam::ShadowColor => aria_models::TextParam::ShadowColor,
                WireTextParam::BackgroundColor => aria_models::TextParam::BackgroundColor,
                WireTextParam::BackgroundRadius => aria_models::TextParam::BackgroundRadius,
            },
        },
        WireClipParam::Look { param } => ClipParam::Look {
            param: match param {
                WireLookParam::FilterIntensity => aria_models::LookParam::FilterIntensity,
                WireLookParam::LutIntensity => aria_models::LookParam::LutIntensity,
                WireLookParam::AdjustBrightness => aria_models::LookParam::AdjustBrightness,
                WireLookParam::AdjustContrast => aria_models::LookParam::AdjustContrast,
                WireLookParam::AdjustSaturation => aria_models::LookParam::AdjustSaturation,
                WireLookParam::AdjustExposure => aria_models::LookParam::AdjustExposure,
                WireLookParam::AdjustTemperature => aria_models::LookParam::AdjustTemperature,
                WireLookParam::AdjustTint => aria_models::LookParam::AdjustTint,
                WireLookParam::AdjustHue => aria_models::LookParam::AdjustHue,
                WireLookParam::AdjustHighlights => aria_models::LookParam::AdjustHighlights,
                WireLookParam::AdjustShadows => aria_models::LookParam::AdjustShadows,
                WireLookParam::AdjustSharpness => aria_models::LookParam::AdjustSharpness,
                WireLookParam::AdjustVignette => aria_models::LookParam::AdjustVignette,
                WireLookParam::AdjustLiftR => aria_models::LookParam::AdjustLiftR,
                WireLookParam::AdjustLiftG => aria_models::LookParam::AdjustLiftG,
                WireLookParam::AdjustLiftB => aria_models::LookParam::AdjustLiftB,
                WireLookParam::AdjustLiftMaster => aria_models::LookParam::AdjustLiftMaster,
                WireLookParam::AdjustGammaR => aria_models::LookParam::AdjustGammaR,
                WireLookParam::AdjustGammaG => aria_models::LookParam::AdjustGammaG,
                WireLookParam::AdjustGammaB => aria_models::LookParam::AdjustGammaB,
                WireLookParam::AdjustGammaMaster => aria_models::LookParam::AdjustGammaMaster,
                WireLookParam::AdjustGainR => aria_models::LookParam::AdjustGainR,
                WireLookParam::AdjustGainG => aria_models::LookParam::AdjustGainG,
                WireLookParam::AdjustGainB => aria_models::LookParam::AdjustGainB,
                WireLookParam::AdjustGainMaster => aria_models::LookParam::AdjustGainMaster,
                WireLookParam::MaskFeather => aria_models::LookParam::MaskFeather,
                WireLookParam::MaskCenter => aria_models::LookParam::MaskCenter,
                WireLookParam::MaskSize => aria_models::LookParam::MaskSize,
                WireLookParam::MaskRotation => aria_models::LookParam::MaskRotation,
                WireLookParam::MaskRoundness => aria_models::LookParam::MaskRoundness,
                WireLookParam::ChromaStrength => aria_models::LookParam::ChromaStrength,
                WireLookParam::ChromaShadow => aria_models::LookParam::ChromaShadow,
                WireLookParam::MatteSoftness => aria_models::LookParam::MatteSoftness,
            },
        },
        WireClipParam::Style { param } => ClipParam::Style {
            param: match param {
                WireStyleParam::ShadowColor => aria_models::StyleParam::ShadowColor,
                WireStyleParam::ShadowOffset => aria_models::StyleParam::ShadowOffset,
                WireStyleParam::ShadowBlur => aria_models::StyleParam::ShadowBlur,
                WireStyleParam::GlowColor => aria_models::StyleParam::GlowColor,
                WireStyleParam::GlowRadius => aria_models::StyleParam::GlowRadius,
                WireStyleParam::GlowIntensity => aria_models::StyleParam::GlowIntensity,
                WireStyleParam::OutlineColor => aria_models::StyleParam::OutlineColor,
                WireStyleParam::OutlineWidth => aria_models::StyleParam::OutlineWidth,
                WireStyleParam::BackgroundColor => aria_models::StyleParam::BackgroundColor,
                WireStyleParam::BackgroundPadding => aria_models::StyleParam::BackgroundPadding,
                WireStyleParam::BackgroundRadius => aria_models::StyleParam::BackgroundRadius,
            },
        },
    })
}

/// A grade slider value, checked against the range its own catalog row
/// declares — so the refusal names the same bounds the model validates
/// against, for a slider nobody had to list here.
pub(super) fn adjust_slider(
    value: f64,
    slider: &aria_models::AdjustSlider,
) -> Result<f32, Rejection> {
    let (min, max) = (f64::from(slider.min), f64::from(slider.max));
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(Rejection::new(format!(
            "{} must be between {min} and {max} (got {value})",
            slider.name
        )));
    }
    Ok(value as f32)
}

pub(super) fn lower_mask_kind(kind: WireMaskKind) -> MaskKind {
    match kind {
        WireMaskKind::Linear => MaskKind::Linear,
        WireMaskKind::Mirror => MaskKind::Mirror,
        WireMaskKind::Circle => MaskKind::Circle,
        WireMaskKind::Rectangle => MaskKind::Rectangle,
        WireMaskKind::Heart => MaskKind::Heart,
        WireMaskKind::Star => MaskKind::Star,
    }
}

pub(super) fn lower_mask(wire: &WireMask) -> Result<Mask, Rejection> {
    let feather = wire.feather.unwrap_or(0.0);
    if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
        return Err(Rejection::new(format!(
            "mask feather must be between 0 and 1 (got {feather})"
        )));
    }
    let mut mask = Mask::new(lower_mask_kind(wire.kind));
    mask.feather = Param::Constant(feather as f32);
    mask.invert = wire.invert.unwrap_or(false);
    if let Some(center) = wire.center {
        if !center[0].is_finite() || !center[1].is_finite() {
            return Err(Rejection::new("mask center coordinates must be finite"));
        }
        mask.center = Param::Constant(center);
    }
    if let Some(size) = wire.size {
        if !size[0].is_finite() || !size[1].is_finite() {
            return Err(Rejection::new("mask size coordinates must be finite"));
        }
        mask.size = Param::Constant(size);
    }
    if let Some(rotation) = wire.rotation {
        if !rotation.is_finite() {
            return Err(Rejection::new("mask rotation must be finite"));
        }
        mask.rotation = Param::Constant(rotation);
    }
    if let Some(roundness) = wire.roundness {
        if !roundness.is_finite() {
            return Err(Rejection::new("mask roundness must be finite"));
        }
        mask.roundness = Param::Constant(roundness);
    }
    // Model `Mask::validate` range-checks every geometry field.
    mask.validate().map_err(|e| Rejection::new(e.to_string()))?;
    Ok(mask)
}

pub(super) fn lower_chroma(wire: &WireChromaKey) -> Result<ChromaKey, Rejection> {
    let strength = wire.strength.unwrap_or(0.0);
    let shadow = wire.shadow.unwrap_or(0.0);
    for (name, value) in [("strength", strength), ("shadow", shadow)] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(Rejection::new(format!(
                "chroma {name} must be between 0 and 1 (got {value})"
            )));
        }
    }
    let chroma = ChromaKey {
        rgb: wire.rgb,
        strength: (strength as f32).into(),
        shadow: (shadow as f32).into(),
    };
    chroma
        .validate()
        .map_err(|e| Rejection::new(e.to_string()))?;
    Ok(chroma)
}

pub(super) fn lower_matte(wire: &WireTrackMatte) -> Result<TrackMatte, Rejection> {
    let softness = wire.softness.unwrap_or(0.0);
    if !softness.is_finite() || !(0.0..=1.0).contains(&softness) {
        return Err(Rejection::new(format!(
            "matte softness must be between 0 and 1 (got {softness})"
        )));
    }
    let matte = TrackMatte {
        source: match wire.source {
            WireMatteSource::Alpha => MatteSource::Alpha,
            WireMatteSource::Luma => MatteSource::Luma,
        },
        invert: wire.invert.unwrap_or(false),
        softness: (softness as f32).into(),
    };
    matte
        .validate()
        .map_err(|e| Rejection::new(e.to_string()))?;
    Ok(matte)
}

pub(super) fn lower_stabilize(level: WireStabilizeLevel) -> StabilizeLevel {
    match level {
        WireStabilizeLevel::Recommended => StabilizeLevel::Recommended,
        WireStabilizeLevel::Smooth => StabilizeLevel::Smooth,
        WireStabilizeLevel::MaxSmooth => StabilizeLevel::MaxSmooth,
    }
}

pub(super) fn lower_blend_mode(mode: WireBlendMode) -> BlendMode {
    match mode {
        WireBlendMode::Normal => BlendMode::Normal,
        WireBlendMode::Darken => BlendMode::Darken,
        WireBlendMode::Multiply => BlendMode::Multiply,
        WireBlendMode::ColorBurn => BlendMode::ColorBurn,
        WireBlendMode::Lighten => BlendMode::Lighten,
        WireBlendMode::Screen => BlendMode::Screen,
        WireBlendMode::ColorDodge => BlendMode::ColorDodge,
        WireBlendMode::Add => BlendMode::Add,
        WireBlendMode::Overlay => BlendMode::Overlay,
        WireBlendMode::SoftLight => BlendMode::SoftLight,
        WireBlendMode::HardLight => BlendMode::HardLight,
        WireBlendMode::Difference => BlendMode::Difference,
        WireBlendMode::Exclusion => BlendMode::Exclusion,
    }
}

pub(super) fn lower_filter(wire: &crate::wire::WireFilter) -> Result<Filter, Rejection> {
    let intensity = wire.intensity.unwrap_or(0.8);
    if !intensity.is_finite() || !(0.0..=1.0).contains(&intensity) {
        return Err(Rejection::new(format!(
            "filter intensity must be between 0 and 1 (got {intensity})"
        )));
    }
    let filter = Filter {
        id: wire.id.clone(),
        intensity: (intensity as f32).into(),
    };
    filter.validate().map_err(|e| {
        let ids = filter_catalog()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if filter_spec(&wire.id).is_none() {
            Rejection::new(format!(
                "unknown filter '{}'; available filters: {ids}",
                wire.id
            ))
        } else {
            Rejection::new(e.to_string())
        }
    })?;
    Ok(filter)
}

pub(super) fn lower_animation_slot(slot: WireAnimationSlot) -> AnimationSlot {
    match slot {
        WireAnimationSlot::In => AnimationSlot::In,
        WireAnimationSlot::Out => AnimationSlot::Out,
        WireAnimationSlot::Combo => AnimationSlot::Combo,
    }
}

pub(super) fn lower_audio_role(role: WireAudioRole) -> AudioRole {
    match role {
        WireAudioRole::Music => AudioRole::Music,
        WireAudioRole::Sfx => AudioRole::Sfx,
        WireAudioRole::Voiceover => AudioRole::Voiceover,
        WireAudioRole::Extracted => AudioRole::Extracted,
    }
}

/// Lower a wire generator. When replacing the content of an existing text
/// clip, the current style is preserved (the agent edits words, not looks).
pub(super) fn lower_generator(wire: &WireGenerator, current: Option<&Generator>) -> Generator {
    match wire {
        WireGenerator::Text { content } => {
            // Payload extraction, not a per-variant decision: only a text
            // generator carries a style to keep, so a new generator has no
            // answer to give here and needs no arm.
            let style = match current {
                Some(Generator::Text { style, .. }) => style.clone(),
                _ => Default::default(),
            };
            Generator::Text {
                content: content.clone(),
                style,
            }
        }
        WireGenerator::Solid { rgba } => Generator::SolidColor { rgba: *rgba },
        WireGenerator::Shape {
            shape,
            rgba,
            width,
            height,
        } => {
            // Payload extraction, as above: only a shape generator carries the
            // geometry a replacement inherits.
            let (shape_w, shape_h, corner_radius, stroke) = match current {
                Some(Generator::Shape {
                    width: w,
                    height: h,
                    corner_radius,
                    stroke,
                    ..
                }) => (
                    w.sample(0),
                    h.sample(0),
                    corner_radius.clone(),
                    stroke.clone(),
                ),
                _ => (
                    aria_models::SHAPE_DROP_WIDTH,
                    aria_models::SHAPE_DROP_HEIGHT,
                    Param::Constant(0.0),
                    None,
                ),
            };
            Generator::Shape {
                shape: match shape {
                    WireShape::Rectangle => aria_models::Shape::Rectangle,
                    WireShape::Ellipse => aria_models::Shape::Ellipse,
                },
                rgba: Param::Constant(*rgba),
                width: Param::Constant(width.unwrap_or(shape_w)),
                height: Param::Constant(height.unwrap_or(shape_h)),
                corner_radius,
                stroke,
            }
        }
    }
}

pub(super) fn generated_content(clip: &Clip) -> Option<&Generator> {
    match &clip.content {
        aria_models::ClipSource::Generated(g) => Some(g),
        aria_models::ClipSource::Media { .. }
        | aria_models::ClipSource::Multicam { .. }
        | aria_models::ClipSource::Sequence { .. } => None,
    }
}
