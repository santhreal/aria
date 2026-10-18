use aria_compositor::ColorGrade;
use aria_models::{
    BlendMode, Generator, Scale2, TextAlignH, TextAlignV, TextCase, TextStyle as ModelTextStyle,
};
use aria_text::{
    FontFamily, TextAlign, TextBackground, TextRun, TextShadow, TextStroke, TextStyle,
    TextVerticalAlign,
};

use super::REFERENCE_HEIGHT;
use super::shape::resolve_shape;
use crate::scene::{LayerSource, ResolvedPass, SceneLayer, SceneLut, SizeSpec};

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_generator(
    generator: &Generator,
    center: [f32; 2],
    anchor_point: [f32; 2],
    rotation: f32,
    opacity: f32,
    uv: [f32; 4],
    color_grade: Option<ColorGrade>,
    lut: Option<SceneLut>,
    cw: f32,
    ch: f32,
    scale: Scale2,
    tick: i64,
    local_seconds: f64,
    effects: Vec<ResolvedPass>,
) -> Option<SceneLayer> {
    resolve_generator_pooled(
        generator,
        center,
        anchor_point,
        rotation,
        opacity,
        uv,
        color_grade,
        lut,
        cw,
        ch,
        scale,
        tick,
        local_seconds,
        effects,
        &mut super::sample::ResolvePool::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_generator_pooled(
    generator: &Generator,
    center: [f32; 2],
    anchor_point: [f32; 2],
    rotation: f32,
    opacity: f32,
    uv: [f32; 4],
    color_grade: Option<ColorGrade>,
    lut: Option<SceneLut>,
    cw: f32,
    ch: f32,
    scale: Scale2,
    tick: i64,
    local_seconds: f64,
    effects: Vec<ResolvedPass>,
    pool: &mut super::sample::ResolvePool,
) -> Option<SceneLayer> {
    let ref_scale = ch / REFERENCE_HEIGHT;
    let has_lut = lut.is_some();
    let mut layer = match generator {
        Generator::Text { content, style } => {
            let mut text = pool.take_string();
            apply_text_case_into(style.case, content, &mut text);
            if text.trim().is_empty() {
                return None;
            }
            let font_buf = if style.font.is_empty() {
                String::new()
            } else {
                pool.take_string()
            };
            Some(SceneLayer {
                clip: None,
                source: LayerSource::Text {
                    style: map_text_style_for(&text, style, cw, ch, tick, font_buf),
                    content: text,
                    animation: None,
                },
                center,
                anchor_point,
                // Per-axis placement of the text bitmap quad.
                size: SizeSpec::BitmapScaled([scale.x, scale.y]),
                rotation,
                opacity,
                uv,
                effects,
                masks: Vec::new(),
                chroma_key: None,
                color_grade,
                lut: None,
                blend_mode: BlendMode::Normal,
                styles: None,
                blur_passes: Vec::new(),
            })
        }
        Generator::SolidColor { rgba } => Some(SceneLayer {
            clip: None,
            source: LayerSource::Solid(*rgba),
            center,
            anchor_point,
            // Per-axis placement relative to the canvas.
            size: SizeSpec::Fixed([cw * scale.x, ch * scale.y]),
            rotation,
            opacity,
            uv,
            effects,
            masks: Vec::new(),
            chroma_key: None,
            color_grade,
            lut: None,
            blend_mode: BlendMode::Normal,
            styles: None,
            blur_passes: Vec::new(),
        }),
        Generator::Shape {
            shape,
            rgba,
            width,
            height,
            corner_radius,
            stroke,
        } => resolve_shape(
            shape,
            rgba,
            width,
            height,
            corner_radius,
            stroke.as_ref(),
            tick,
            ref_scale,
            scale,
            center,
            anchor_point,
            rotation,
            opacity,
            uv,
            color_grade,
            effects,
        ),
        // A lane bar covers the canvas and applies what it carries to
        // everything composited below it: its own effect stack, its grade, and
        // its LUT. The three variants differ only in what an operator reaches
        // for; dropping a bar's effects made the inspector's effect tab a
        // no-op on an adjustment layer.
        Generator::Effect | Generator::Filter | Generator::Adjustment => {
            canvas_pass(effects, color_grade, has_lut, cw, ch)
        }
        Generator::Lottie {
            path,
            width,
            height,
        } => {
            // Same placement convention as stickers: intrinsic pixels are
            // reference pixels. Per-axis transform scale stretches the quad.
            let px_x = ref_scale * scale.x;
            let px_y = ref_scale * scale.y;
            Some(SceneLayer {
                clip: None,
                source: LayerSource::Lottie {
                    path: path.clone(),
                    local_time: local_seconds,
                },
                center,
                anchor_point,
                size: SizeSpec::Fixed([*width as f32 * px_x, *height as f32 * px_y]),
                rotation,
                opacity,
                uv,
                effects,
                masks: Vec::new(),
                chroma_key: None,
                color_grade,
                lut: None,
                blend_mode: BlendMode::Normal,
                styles: None,
                blur_passes: Vec::new(),
            })
        }
        Generator::Sticker { asset } => {
            // Unknown/empty ids place nothing — the legacy payload-less
            // sticker behavior, never an error.
            let spec = aria_models::sticker_spec(asset)?;
            // Intrinsic pixels are *reference pixels* (1080p canvas), the
            // same convention as shapes: a 256 px sticker lands at a
            // CapCut-like overlay size and samples ~1:1 instead of being
            // blown up to canvas height like aspect-fit media.
            let px_x = ref_scale * scale.x;
            let px_y = ref_scale * scale.y;
            Some(SceneLayer {
                clip: None,
                source: LayerSource::Sticker {
                    asset: asset.clone(),
                    local_time: local_seconds,
                },
                center,
                anchor_point,
                size: SizeSpec::Fixed([spec.width as f32 * px_x, spec.height as f32 * px_y]),
                rotation,
                opacity,
                uv,
                effects,
                masks: Vec::new(),
                chroma_key: None,
                color_grade,
                lut: None,
                blend_mode: BlendMode::Normal,
                styles: None,
                blur_passes: Vec::new(),
            })
        }
    }?;
    layer.lut = lut;
    Some(layer)
}

fn canvas_pass(
    effects: Vec<ResolvedPass>,
    color_grade: Option<ColorGrade>,
    has_lut: bool,
    cw: f32,
    ch: f32,
) -> Option<SceneLayer> {
    (!effects.is_empty() || color_grade.is_some() || has_lut).then_some(SceneLayer {
        clip: None,
        source: LayerSource::CanvasPass,
        center: [cw * 0.5, ch * 0.5],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([cw, ch]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects,
        masks: Vec::new(),
        chroma_key: None,
        color_grade,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    })
}

fn apply_text_case_into(case: TextCase, src: &str, out: &mut String) {
    out.clear();
    out.reserve(src.len());
    match case {
        TextCase::Normal => out.push_str(src),
        TextCase::Upper => out.extend(src.chars().flat_map(char::to_uppercase)),
        TextCase::Lower => out.extend(src.chars().flat_map(char::to_lowercase)),
        TextCase::Title => {
            let mut start_of_word = true;
            for ch in src.chars() {
                if ch.is_whitespace() {
                    start_of_word = true;
                    out.push(ch);
                } else if start_of_word {
                    start_of_word = false;
                    out.extend(ch.to_uppercase());
                } else {
                    out.extend(ch.to_lowercase());
                }
            }
        }
    }
}

/// Map a model [`ModelTextStyle`] onto a [`aria_text`] render style.
///
/// Quantize layout-affecting metrics so slow keyframe ramps reuse shape /
/// atlas caches instead of reshaping every frame (0.25px steps).
fn quantize_layout_px(v: f32) -> f32 {
    const STEP: f32 = 0.25;
    (v / STEP).round() * STEP
}

/// Reference-pixel metrics are scaled against the 1080px authoring height.
/// The raster remains ink-tight even when wrapping uses the canvas width;
/// alignment is applied later to the finished bitmap's placement.
pub(super) fn map_text_style(style: &ModelTextStyle, cw: f32, ch: f32, tick: i64) -> TextStyle {
    map_text_style_pooled(style, cw, ch, tick, String::new())
}

fn map_text_style_pooled(
    style: &ModelTextStyle,
    cw: f32,
    ch: f32,
    tick: i64,
    mut font_buf: String,
) -> TextStyle {
    let scale = ch / REFERENCE_HEIGHT;
    let font_size = quantize_layout_px(style.size.sample(tick) * scale);
    let letter_spacing = quantize_layout_px(style.letter_spacing.sample(tick) * scale);
    let family = if style.font.is_empty() {
        FontFamily::SansSerif
    } else {
        font_buf.clear();
        font_buf.push_str(&style.font);
        FontFamily::Named(font_buf)
    };
    let align = match style.align_h {
        TextAlignH::Left => TextAlign::Left,
        TextAlignH::Center => TextAlign::Center,
        TextAlignH::Right => TextAlign::Right,
        TextAlignH::Justify => TextAlign::Justify,
    };
    let vertical_align = match style.align_v {
        TextAlignV::Top => TextVerticalAlign::Top,
        TextAlignV::Middle => TextVerticalAlign::Middle,
        TextAlignV::Bottom => TextVerticalAlign::Bottom,
    };
    let mut mapped = TextStyle::new(font_size)
        .with_color(style.fill.sample(tick))
        .with_family(family)
        .with_bold(style.bold)
        .with_italic(style.italic)
        .with_underline(style.underline)
        .with_letter_spacing(letter_spacing)
        .with_kerning(style.kerning)
        .with_align(align)
        .with_vertical_align(vertical_align)
        .with_line_height(quantize_layout_px(
            font_size * style.line_spacing.sample(tick),
        ));
    if style.wrap {
        mapped = mapped.with_max_width(cw.max(1.0));
    }
    if let Some(stroke) = &style.stroke {
        mapped = mapped.with_stroke(TextStroke {
            rgba: stroke.rgba.sample(tick),
            width: stroke.width.sample(tick) * scale,
        });
    }
    if let Some(background) = &style.background {
        mapped = mapped.with_background(TextBackground {
            rgba: background.rgba.sample(tick),
            radius: background.radius.sample(tick),
        });
    }
    if let Some(shadow) = &style.shadow {
        mapped = mapped.with_shadow(TextShadow {
            rgba: shadow.rgba.sample(tick),
            // Fraction of font size — keep relative; rasterizer multiplies.
            blur: shadow.blur.sample(tick),
            distance: shadow.distance.sample(tick) * scale,
        });
    }
    if !style.runs.is_empty() {
        mapped.runs = style
            .runs
            .iter()
            .map(|r| map_text_run(r.start..r.end, r, scale))
            .collect();
    }
    mapped
}

fn map_text_style_for(
    text: &str,
    style: &ModelTextStyle,
    cw: f32,
    ch: f32,
    tick: i64,
    font_buf: String,
) -> TextStyle {
    let mut mapped = map_text_style_pooled(style, cw, ch, tick, font_buf);
    if !style.runs.is_empty() {
        let scale = ch / REFERENCE_HEIGHT;
        mapped.runs = style
            .runs
            .iter()
            .filter_map(|r| {
                let start = char_to_byte(text, r.start);
                let end = char_to_byte(text, r.end);
                (start < end).then(|| map_text_run(start..end, r, scale))
            })
            .collect();
    }
    mapped
}

fn map_text_run(
    range: std::ops::Range<usize>,
    r: &aria_models::TextStyleRun,
    scale: f32,
) -> TextRun {
    TextRun {
        range,
        family: r.font.as_ref().map(|f| {
            if f.is_empty() {
                FontFamily::SansSerif
            } else {
                FontFamily::Named(f.clone())
            }
        }),
        font_size: r.size.map(|s| quantize_layout_px(s * scale)),
        bold: r.bold,
        italic: r.italic,
        underline: r.underline,
        color: r.fill,
        letter_spacing: r.letter_spacing.map(|ls| quantize_layout_px(ls * scale)),
    }
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(byte_idx, _)| byte_idx)
}
