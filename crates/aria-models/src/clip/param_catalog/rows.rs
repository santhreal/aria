//! The static catalog rows: one per property every clip declares, each with
//! its group, its range and the route to the field it names.

use std::sync::LazyLock;

use super::{ParamKind, ParamRange, ParamRoute, ParamScope, ParamSpec, Route};
use crate::clip::{
    Clip, ClipParam, ClipSource, Generator, LookParam, MAX_SHAPE_DIM, MAX_STROKE_WIDTH, Shape,
    ShapeParam, StyleParam, TextParam,
};
use crate::param::Param;

/// The style block a style property lives on, so a row states its owner once.
fn shadow(clip: &Clip) -> Option<&crate::look::LayerShadow> {
    clip.styles.shadow.as_ref()
}

fn text_style(clip: &Clip) -> Option<&crate::clip::TextStyle> {
    match &clip.content {
        ClipSource::Generated(Generator::Text { style, .. }) => Some(style),
        ClipSource::Generated(
            Generator::SolidColor { .. }
            | Generator::Shape { .. }
            | Generator::Sticker { .. }
            | Generator::Lottie { .. }
            | Generator::Effect
            | Generator::Filter
            | Generator::Adjustment,
        )
        | ClipSource::Media { .. }
        | ClipSource::Multicam { .. }
        | ClipSource::Sequence { .. } => None,
    }
}

/// The shape generator's fields, when this clip is one.
type ShapeFields<'a> = (
    &'a Shape,
    &'a Param<[u8; 4]>,
    &'a Param<f32>,
    &'a Param<f32>,
    &'a Param<f32>,
    Option<&'a crate::clip::ShapeStroke>,
);

fn shape_fields(clip: &Clip) -> Option<ShapeFields<'_>> {
    match &clip.content {
        ClipSource::Generated(Generator::Shape {
            shape,
            rgba,
            width,
            height,
            corner_radius,
            stroke,
        }) => Some((shape, rgba, width, height, corner_radius, stroke.as_ref())),
        ClipSource::Generated(
            Generator::Text { .. }
            | Generator::SolidColor { .. }
            | Generator::Sticker { .. }
            | Generator::Lottie { .. }
            | Generator::Effect
            | Generator::Filter
            | Generator::Adjustment,
        )
        | ClipSource::Media { .. }
        | ClipSource::Multicam { .. }
        | ClipSource::Sequence { .. } => None,
    }
}

/// Build one row. Keeps a row to its facts, its group, and its route.
fn spec(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    param: ClipParam,
    kind: ParamKind,
    scope: ParamScope,
    range: ParamRange,
    route: fn(&Clip) -> ParamRoute<'_>,
) -> ParamSpec {
    ParamSpec {
        id: std::borrow::Cow::Borrowed(id),
        label: std::borrow::Cow::Borrowed(label),
        group: std::borrow::Cow::Borrowed(group),
        param,
        kind,
        scope,
        range,
        route: Route::Fixed(route),
    }
}

fn look(param: LookParam) -> ClipParam {
    ClipParam::Look { param }
}

fn style(param: StyleParam) -> ClipParam {
    ClipParam::Style { param }
}

fn text(param: TextParam) -> ClipParam {
    ClipParam::Text { param }
}

fn shape(param: ShapeParam) -> ClipParam {
    ClipParam::Shape { param }
}

const UNIT: ParamRange = ParamRange::between(0.0, 1.0);
const SIGNED_UNIT: ParamRange = ParamRange::between(-1.0, 1.0);
const QUAD_UNIT: ParamRange = ParamRange::between(0.0, 4.0);

/// The rows the directly addressed properties contribute.
///
/// Read from the [`ClipParam`] registry, which owns each one's id, label,
/// group, kind, scope, range and route. A new direct property appears here by
/// declaring its row there — there is no second list to keep in step.
fn direct_rows() -> Vec<ParamSpec> {
    crate::clip::param_axis::direct_rows()
        .map(|(param, id, browse)| ParamSpec {
            id: std::borrow::Cow::Borrowed(id),
            label: std::borrow::Cow::Borrowed(browse.label),
            group: std::borrow::Cow::Borrowed(browse.group),
            param,
            kind: browse.kind,
            scope: param.scope(),
            range: browse.range,
            route: Route::Axis,
        })
        .collect()
}

/// The catalog range each slider shape draws on.
macro_rules! grade_range {
    (signed) => {
        SIGNED_UNIT
    };
    (positive) => {
        UNIT
    };
    (multiplicative) => {
        QUAD_UNIT
    };
}

/// One catalog row per grade slider, from the adjustment row list: the id, the
/// label and the range come from the row, and the route is the field it names.
macro_rules! grade_rows {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// Every grade slider's catalog row, in row order.
        fn grade_specs() -> Vec<ParamSpec> {
            use ParamKind::Scalar;
            use ParamScope::Visual;
            vec![$(spec(
                $id,
                $label,
                "grade",
                look(LookParam::$variant),
                Scalar,
                Visual,
                grade_range!($shape),
                |c| ParamRoute::Scalar(&c.adjust.$field),
            ),)*]
        }
    };
}

crate::adjust_slider_rows!(grade_rows);

pub(super) static CATALOG: LazyLock<Vec<ParamSpec>> = LazyLock::new(|| {
    use ParamKind::{Color, Scalar, Vec2};
    use ParamScope::Visual;
    let mut rows = direct_rows();
    rows.extend([
        // --- look: filter and LUT -------------------------------------------
        spec(
            "filter-intensity",
            "Filter amount",
            "filter",
            look(LookParam::FilterIntensity),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.filter {
                Some(f) => ParamRoute::Scalar(&f.intensity),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "lut-intensity",
            "LUT amount",
            "lut",
            look(LookParam::LutIntensity),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.lut {
                Some(l) => ParamRoute::Scalar(&l.intensity),
                None => ParamRoute::Absent,
            },
        ),
    ]);
    // One row per adjustment, expanded from the grade slider row list, so an
    // adjustment reaches the inspector, the keyframe strip and the parameter
    // axis by existing rather than by being repeated here.
    rows.extend(grade_specs());
    rows.extend([
        // --- look: chroma key -----------------------------------------------
        spec(
            "chroma-strength",
            "Key strength",
            "chroma",
            look(LookParam::ChromaStrength),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.chroma_key {
                Some(k) => ParamRoute::Scalar(&k.strength),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "chroma-shadow",
            "Key shadow",
            "chroma",
            look(LookParam::ChromaShadow),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.chroma_key {
                Some(k) => ParamRoute::Scalar(&k.shadow),
                None => ParamRoute::Absent,
            },
        ),
        // --- look: track matte ----------------------------------------------
        spec(
            "matte-softness",
            "Matte softness",
            "matte",
            look(LookParam::MatteSoftness),
            Scalar,
            Visual,
            UNIT,
            |c| match &c.matte {
                Some(m) => ParamRoute::Scalar(&m.softness),
                None => ParamRoute::Absent,
            },
        ),
        // --- layer styles ---------------------------------------------------
        spec(
            "style-shadow-color",
            "Shadow colour",
            "shadow",
            style(StyleParam::ShadowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shadow(c) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-shadow-offset",
            "Shadow offset",
            "shadow",
            style(StyleParam::ShadowOffset),
            Vec2,
            Visual,
            ParamRange::ANY,
            |c| match shadow(c) {
                Some(s) => ParamRoute::Vec2(&s.offset),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-shadow-blur",
            "Shadow blur",
            "shadow",
            style(StyleParam::ShadowBlur),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match shadow(c) {
                Some(s) => ParamRoute::Scalar(&s.blur),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-color",
            "Glow colour",
            "glow",
            style(StyleParam::GlowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Color(&g.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-radius",
            "Glow radius",
            "glow",
            style(StyleParam::GlowRadius),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Scalar(&g.radius),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-glow-intensity",
            "Glow intensity",
            "glow",
            style(StyleParam::GlowIntensity),
            Scalar,
            Visual,
            ParamRange::between(0.0, 4.0),
            |c| match &c.styles.glow {
                Some(g) => ParamRoute::Scalar(&g.intensity),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-outline-color",
            "Outline colour",
            "outline",
            style(StyleParam::OutlineColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.outline {
                Some(o) => ParamRoute::Color(&o.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-outline-width",
            "Outline width",
            "outline",
            style(StyleParam::OutlineWidth),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.outline {
                Some(o) => ParamRoute::Scalar(&o.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-color",
            "Background colour",
            "background",
            style(StyleParam::BackgroundColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Color(&b.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-padding",
            "Background padding",
            "background",
            style(StyleParam::BackgroundPadding),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Scalar(&b.padding),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "style-background-radius",
            "Background radius",
            "background",
            style(StyleParam::BackgroundRadius),
            Scalar,
            Visual,
            ParamRange::above(0.0),
            |c| match &c.styles.background {
                Some(b) => ParamRoute::Scalar(&b.radius),
                None => ParamRoute::Absent,
            },
        ),
        // --- text style -----------------------------------------------------
        spec(
            "text-size",
            "Text size",
            "text",
            text(TextParam::Size),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, crate::clip::text::MAX_TEXT_SIZE),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.size),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-fill",
            "Text colour",
            "text",
            text(TextParam::Fill),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c) {
                Some(s) => ParamRoute::Color(&s.fill),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-letter-spacing",
            "Letter spacing",
            "text",
            text(TextParam::LetterSpacing),
            Scalar,
            Visual,
            ParamRange::between(
                -crate::clip::text::MAX_TEXT_LETTER_SPACING,
                crate::clip::text::MAX_TEXT_LETTER_SPACING,
            ),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.letter_spacing),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-line-spacing",
            "Line spacing",
            "text",
            text(TextParam::LineSpacing),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, crate::clip::text::MAX_TEXT_LINE_SPACING),
            |c| match text_style(c) {
                Some(s) => ParamRoute::Scalar(&s.line_spacing),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-stroke-width",
            "Text stroke width",
            "text",
            text(TextParam::StrokeWidth),
            Scalar,
            Visual,
            ParamRange::between(0.0, crate::clip::text::MAX_TEXT_STROKE_WIDTH),
            |c| match text_style(c).and_then(|s| s.stroke.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-stroke-color",
            "Text stroke colour",
            "text",
            text(TextParam::StrokeColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.stroke.as_ref()) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-blur",
            "Text shadow blur",
            "text",
            text(TextParam::ShadowBlur),
            Scalar,
            Visual,
            ParamRange::between(0.0, 1.0),
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.blur),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-distance",
            "Text shadow distance",
            "text",
            text(TextParam::ShadowDistance),
            Scalar,
            Visual,
            ParamRange::between(0.0, crate::clip::text::MAX_TEXT_SHADOW_DISTANCE),
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Scalar(&s.distance),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-shadow-color",
            "Text shadow colour",
            "text",
            text(TextParam::ShadowColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.shadow.as_ref()) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-background-color",
            "Text card colour",
            "text",
            text(TextParam::BackgroundColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match text_style(c).and_then(|s| s.background.as_ref()) {
                Some(b) => ParamRoute::Color(&b.rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "text-background-radius",
            "Text card radius",
            "text",
            text(TextParam::BackgroundRadius),
            Scalar,
            Visual,
            UNIT,
            |c| match text_style(c).and_then(|s| s.background.as_ref()) {
                Some(b) => ParamRoute::Scalar(&b.radius),
                None => ParamRoute::Absent,
            },
        ),
        // --- shape generator ------------------------------------------------
        spec(
            "shape-width",
            "Shape width",
            "shape",
            shape(ShapeParam::Width),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, width, ..)) => ParamRoute::Scalar(width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-height",
            "Shape height",
            "shape",
            shape(ShapeParam::Height),
            Scalar,
            Visual,
            ParamRange::between(f32::EPSILON, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, _, height, ..)) => ParamRoute::Scalar(height),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-corner-radius",
            "Corner radius",
            "shape",
            shape(ShapeParam::CornerRadius),
            Scalar,
            Visual,
            ParamRange::between(0.0, MAX_SHAPE_DIM),
            |c| match shape_fields(c) {
                Some((_, _, _, _, radius, _)) => ParamRoute::Scalar(radius),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-inner-ratio",
            "Star inner ratio",
            "shape",
            shape(ShapeParam::InnerRatio),
            Scalar,
            Visual,
            UNIT,
            |c| match shape_fields(c) {
                Some((Shape::Star { inner_ratio, .. }, ..)) => ParamRoute::Scalar(inner_ratio),
                _ => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-fill",
            "Shape fill",
            "shape",
            shape(ShapeParam::Fill),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shape_fields(c) {
                Some((_, rgba, ..)) => ParamRoute::Color(rgba),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-stroke-width",
            "Shape stroke width",
            "shape",
            shape(ShapeParam::StrokeWidth),
            Scalar,
            Visual,
            ParamRange::between(0.0, MAX_STROKE_WIDTH),
            |c| match shape_fields(c).and_then(|f| f.5) {
                Some(s) => ParamRoute::Scalar(&s.width),
                None => ParamRoute::Absent,
            },
        ),
        spec(
            "shape-stroke-color",
            "Shape stroke colour",
            "shape",
            shape(ShapeParam::StrokeColor),
            Color,
            Visual,
            ParamRange::ANY,
            |c| match shape_fields(c).and_then(|f| f.5) {
                Some(s) => ParamRoute::Color(&s.rgba),
                None => ParamRoute::Absent,
            },
        ),
    ]);
    rows
});
