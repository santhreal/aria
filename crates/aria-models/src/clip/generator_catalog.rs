//! Every generated clip content kind, as data.
//!
//! A generator used to be an enum variant plus a hand-written arm in each place
//! that cared: the lane it belongs on, the wire name a describer prints, the
//! label a clip card shows, the structural check the project runs before
//! accepting it, and the keyframe rebase a split performs. Nothing tied those
//! together, so a generator could exist in the model and be unplaceable,
//! unnamed, or silently unvalidated, and no test went red.
//!
//! A row here carries the generator's identity, the lane that accepts it, the
//! renderer topologies it resolves to, the inspector blocks it opens, and
//! routes to its structural check and its keyframe rebase. Surfaces ask the row
//! instead of keeping their own match.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`Generator`], so a new generator
//! cannot compile until it names a row. `a_row_per_generator_variant` proves the
//! arm points at that generator's own row rather than reusing a neighbour's, by
//! round-tripping every row's own [`GeneratorSpec::census`] sample.

use crate::error::ModelError;
use crate::track::TrackKind;

use super::{
    Generator, MAX_SHAPE_DIM, MAX_STAR_POINTS, MAX_STROKE_WIDTH, ParamTime, Shape, ShapeStroke,
};

/// One generated content kind.
///
/// Every column is a question some surface used to answer with its own match
/// arm. `renders_with` names the renderer's layer topologies by id rather than
/// by type: the model cannot depend on the renderer, and the renderer's own
/// layer catalog pins the two lists together.
pub struct GeneratorSpec {
    /// Stable wire name. What a describer, a projection, or an agent prints.
    pub id: &'static str,
    /// Fallback operator-facing label for a clip that carries no better name of
    /// its own (a sticker's catalog label, a Lottie's file stem).
    pub label: &'static str,
    /// The one lane kind that accepts this generator.
    pub lane: TrackKind,
    /// The layer topology ids this generator can resolve to. More than one when
    /// the geometry decides: a shape is a GPU signed-distance field unless its
    /// geometry is a pen path, which rasterizes on the CPU.
    pub renders_with: &'static [&'static str],
    /// Whether the clip opens the inspector's text block.
    pub has_text: bool,
    /// Whether the clip opens the inspector's shape block.
    pub has_shape: bool,
    /// Whether the content points at something outside the project: a catalog
    /// id or a file on disk. Those resolve to nothing when missing rather than
    /// failing a frame, so a surface offering a relink asks this.
    pub needs_asset: bool,
    /// Whether the generator draws its own pixels. A lane bar has no geometry:
    /// it reprocesses whatever is already composited beneath its track.
    pub paints: bool,
    /// Which time this generator's parameter keyframes are addressed in. A
    /// lane bar's look belongs to the programme under it, so it keys in
    /// sequence ticks; anything that paints keys in its own.
    pub param_time: ParamTime,
    /// Whether the content carries clip-relative keyframes, so a split or a
    /// trim has to rebase them.
    pub animatable: bool,
    /// One representative value of this variant, for tests and catalog sweeps
    /// that need a generator of every kind without hand-listing them.
    pub census: fn() -> Generator,
    /// Structural check run before a project accepts the content.
    pub validate: fn(&Generator) -> Result<(), ModelError>,
}

impl std::fmt::Debug for GeneratorSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratorSpec")
            .field("id", &self.id)
            .field("lane", &self.lane)
            .field("renders_with", &self.renders_with)
            .finish_non_exhaustive()
    }
}

/// The shipped generators, in declaration order.
pub(crate) const SHIPPED: &[GeneratorSpec] = &[
    GeneratorSpec {
        id: "text",
        label: "Text",
        lane: TrackKind::Text,
        renders_with: &["text"],
        has_text: true,
        has_shape: false,
        needs_asset: false,
        paints: true,
        param_time: ParamTime::ClipLocal,
        animatable: true,
        census: census_text,
        validate: validate_text,
    },
    GeneratorSpec {
        id: "solid",
        label: "Color",
        lane: TrackKind::Sticker,
        renders_with: &["solid"],
        has_text: false,
        has_shape: false,
        needs_asset: false,
        paints: true,
        param_time: ParamTime::ClipLocal,
        animatable: false,
        census: census_solid,
        validate: validate_nothing,
    },
    GeneratorSpec {
        id: "shape",
        label: "Shape",
        lane: TrackKind::Sticker,
        renders_with: &["shape", "path-shape"],
        has_text: false,
        has_shape: true,
        needs_asset: false,
        paints: true,
        param_time: ParamTime::ClipLocal,
        animatable: true,
        census: census_shape,
        validate: validate_shape,
    },
    GeneratorSpec {
        id: "sticker",
        label: "Sticker",
        lane: TrackKind::Sticker,
        renders_with: &["sticker"],
        has_text: false,
        has_shape: false,
        needs_asset: true,
        paints: true,
        param_time: ParamTime::ClipLocal,
        animatable: false,
        census: census_sticker,
        validate: validate_sticker,
    },
    GeneratorSpec {
        id: "lottie",
        label: "Animation",
        lane: TrackKind::Sticker,
        renders_with: &["lottie"],
        has_text: false,
        has_shape: false,
        needs_asset: true,
        paints: true,
        param_time: ParamTime::ClipLocal,
        animatable: false,
        census: census_lottie,
        validate: validate_lottie,
    },
    GeneratorSpec {
        id: "effect",
        label: "Effect",
        lane: TrackKind::Effect,
        renders_with: &["canvas-pass"],
        has_text: false,
        has_shape: false,
        needs_asset: false,
        paints: false,
        param_time: ParamTime::Sequence,
        animatable: false,
        census: census_effect,
        validate: validate_nothing,
    },
    GeneratorSpec {
        id: "filter",
        label: "Filter",
        lane: TrackKind::Filter,
        renders_with: &["canvas-pass"],
        has_text: false,
        has_shape: false,
        needs_asset: false,
        paints: false,
        param_time: ParamTime::Sequence,
        animatable: false,
        census: census_filter,
        validate: validate_nothing,
    },
    GeneratorSpec {
        id: "adjustment",
        label: "Adjustment",
        lane: TrackKind::Adjustment,
        renders_with: &["canvas-pass"],
        has_text: false,
        has_shape: false,
        needs_asset: false,
        paints: false,
        param_time: ParamTime::Sequence,
        animatable: false,
        census: census_adjustment,
        validate: validate_nothing,
    },
];

/// Which row in [`SHIPPED`] describes `generator`.
///
/// Exhaustive on purpose, and the whole fail-by-default mechanism for this
/// axis: a generator added to `generator.rs` and nowhere else does not compile.
const fn index_of(generator: &Generator) -> usize {
    match generator {
        Generator::Text { .. } => 0,
        Generator::SolidColor { .. } => 1,
        Generator::Shape { .. } => 2,
        Generator::Sticker { .. } => 3,
        Generator::Lottie { .. } => 4,
        Generator::Effect => 5,
        Generator::Filter => 6,
        Generator::Adjustment => 7,
    }
}

/// Every generated content kind, in declaration order.
///
/// A surface that offers "insert a generator" walks this instead of keeping its
/// own list, so a new generator appears there by existing.
pub const fn generator_specs() -> &'static [GeneratorSpec] {
    SHIPPED
}

/// The row describing `generator`.
pub const fn generator_spec(generator: &Generator) -> &'static GeneratorSpec {
    &SHIPPED[index_of(generator)]
}

/// The row with this wire id, for surfaces that address a generator by name.
pub fn generator_spec_by_id(id: &str) -> Option<&'static GeneratorSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

// ---------------------------------------------------------------------------
// Census: one value of every variant
// ---------------------------------------------------------------------------

fn census_text() -> Generator {
    Generator::text("")
}

fn census_solid() -> Generator {
    Generator::SolidColor {
        rgba: [255, 255, 255, 255],
    }
}

fn census_shape() -> Generator {
    Generator::shape(Shape::Rectangle, [255, 255, 255, 255])
}

fn census_sticker() -> Generator {
    // The legacy payload-less sticker: valid, renders nothing.
    Generator::sticker("")
}

fn census_lottie() -> Generator {
    Generator::lottie("census.json", 1, 1)
}

fn census_effect() -> Generator {
    Generator::Effect
}

fn census_filter() -> Generator {
    Generator::Filter
}

fn census_adjustment() -> Generator {
    Generator::Adjustment
}

// ---------------------------------------------------------------------------
// Validation routes
// ---------------------------------------------------------------------------

/// A generator whose content is data with nothing to check.
fn validate_nothing(_: &Generator) -> Result<(), ModelError> {
    Ok(())
}

fn validate_text(generator: &Generator) -> Result<(), ModelError> {
    let Generator::Text { style, .. } = generator else {
        return Ok(());
    };
    style.validate()
}

fn validate_sticker(generator: &Generator) -> Result<(), ModelError> {
    let Generator::Sticker { asset } = generator else {
        return Ok(());
    };
    // Empty = legacy payload-less sticker; renders nothing but loads.
    if !asset.is_empty() && crate::sticker::sticker_spec(asset).is_none() {
        return Err(ModelError::InvalidParam(format!(
            "unknown sticker '{asset}'"
        )));
    }
    Ok(())
}

fn validate_lottie(generator: &Generator) -> Result<(), ModelError> {
    let Generator::Lottie {
        path,
        width,
        height,
    } = generator
    else {
        return Ok(());
    };
    // Path validity (file exists, parses) is a render-time concern — projects
    // move machines. Structural soundness is not.
    if path.trim().is_empty() {
        return Err(ModelError::InvalidParam("empty lottie path".into()));
    }
    if *width == 0 || *height == 0 {
        return Err(ModelError::InvalidParam(
            "lottie intrinsic size must be non-zero".into(),
        ));
    }
    Ok(())
}

fn validate_shape(generator: &Generator) -> Result<(), ModelError> {
    let Generator::Shape {
        shape,
        rgba,
        width,
        height,
        corner_radius,
        stroke,
    } = generator
    else {
        return Ok(());
    };

    match shape {
        Shape::Polygon { sides } => validate_star_points(*sides, "polygon sides")?,
        Shape::Star {
            points,
            inner_ratio,
        } => {
            validate_star_points(*points, "star points")?;
            inner_ratio.validate_shape()?;
            inner_ratio.for_each_value(|v| validate_unit_fraction(*v, "star inner_ratio"))?;
        }
        Shape::Path(path) => path.validate()?,
        Shape::Rectangle | Shape::Ellipse | Shape::Line | Shape::Arrow | Shape::Heart => {}
    }

    rgba.validate_shape()?;
    for p in [width, height] {
        p.validate_shape()?;
        p.for_each_value(|v| validate_shape_dim(*v))?;
    }
    corner_radius.validate_shape()?;
    corner_radius.for_each_value(|v| validate_corner_radius(*v))?;
    if let Some(s) = stroke {
        validate_stroke(s)?;
    }
    Ok(())
}

fn validate_stroke(s: &ShapeStroke) -> Result<(), ModelError> {
    s.rgba.validate_shape()?;
    s.width.validate_shape()?;
    s.width.for_each_value(|v| validate_stroke_width(*v))
}

/// Range check for a shape width/height value.
pub(crate) fn validate_shape_dim(v: f32) -> Result<(), ModelError> {
    if !v.is_finite() || v <= 0.0 || v > MAX_SHAPE_DIM {
        return Err(ModelError::InvalidParam(format!(
            "shape size must be positive and at most {MAX_SHAPE_DIM} reference px"
        )));
    }
    Ok(())
}

/// Range check for a corner-radius value.
pub(crate) fn validate_corner_radius(v: f32) -> Result<(), ModelError> {
    if !v.is_finite() || !(0.0..=MAX_SHAPE_DIM).contains(&v) {
        return Err(ModelError::InvalidParam(format!(
            "corner radius must be in 0..={MAX_SHAPE_DIM} reference px"
        )));
    }
    Ok(())
}

/// Range check for a stroke-width value (0 = invisible but legal, so a width
/// animation can ease from nothing).
pub(crate) fn validate_stroke_width(v: f32) -> Result<(), ModelError> {
    if !v.is_finite() || !(0.0..=MAX_STROKE_WIDTH).contains(&v) {
        return Err(ModelError::InvalidParam(format!(
            "stroke width must be in 0..={MAX_STROKE_WIDTH} reference px"
        )));
    }
    Ok(())
}

/// Range check for a `0..=1` fraction value.
pub(crate) fn validate_unit_fraction(v: f32, what: &str) -> Result<(), ModelError> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return Err(ModelError::InvalidParam(format!("{what} must be in 0..=1")));
    }
    Ok(())
}

/// Range check for star spike / polygon side counts.
fn validate_star_points(n: u32, what: &str) -> Result<(), ModelError> {
    if !(3..=MAX_STAR_POINTS).contains(&n) {
        return Err(ModelError::InvalidParam(format!(
            "{what} must be in 3..={MAX_STAR_POINTS}"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "generator_catalog_tests.rs"]
mod tests;
