use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::ids::{MediaId, MulticamGroupId, SequenceId};
use crate::param::Param;
use crate::time::TimeRange;

use super::generator_catalog::{GeneratorSpec, generator_spec};
use super::text::TextStyle;

mod params;

/// What a clip draws. Either a trimmed range of imported media, or synthetic
/// content rendered by the engine (text, shapes, solids, ...).
///
/// `Generated` dwarfs `Media` because [`Generator`] carries the full text
/// style / shape params inline. That's deliberate: projects hold at most a
/// few hundred clips and the resolver walks them every frame, so inline
/// storage beats boxing (one less pointer chase on the hot path).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipSource {
    /// A trimmed portion of a [`MediaSource`](crate::MediaSource).
    ///
    /// `source` is the in/out within the media at the media's native rate.
    Media { media: MediaId, source: TimeRange },
    /// Engine-generated content with no backing file.
    Generated(Generator),
    /// A multicam camera group placement.
    Multicam {
        group: MulticamGroupId,
        source: TimeRange,
    },
    /// A nested sequence placed as a clip on a timeline.
    ///
    /// `source` is the in/out within the sequence timeline at its native rate.
    Sequence {
        sequence: SequenceId,
        source: TimeRange,
    },
}

/// A synthetic clip with no source media. Parameters are intentionally minimal
/// for now; richer styling (fonts, transforms, gradients) can be added per
/// variant without touching the timeline model.
///
/// `Deserialize` is hand-written (below the enum) for sticker back-compat;
/// keep the mirror enum in that impl in sync when changing variants here.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Generator {
    /// A title / text layer.
    ///
    /// `style` carries the full visual treatment (font, size, color, stroke,
    /// background, shadow, …). It is `#[serde(default)]` so projects written
    /// before styling existed load with the default look.
    Text {
        content: String,
        #[serde(default)]
        style: TextStyle,
    },
    /// A solid fill (RGBA, 0-255).
    SolidColor { rgba: [u8; 4] },
    /// A centered vector shape with a fill color and optional outline.
    ///
    /// `width` and `height` are in *reference pixels* relative to a 1080px-tall
    /// canvas; the renderer scales them by `canvas_height / 1080` (same
    /// convention as [`TextStyle::size`]). Missing fields deserialize to the
    /// legacy centered-50%-of-canvas look; freshly dropped shapes use
    /// [`SHAPE_DROP_WIDTH`] × [`SHAPE_DROP_HEIGHT`].
    ///
    /// The geometry and style fields are [`Param`]s (M2 pattern): constants
    /// serialize as bare values, byte-identical to the pre-shape-animation
    /// format, and keyframed curves as `{"kf":[...]}`. Keyframe ticks are
    /// clip-relative, like every other clip param. Never-touched
    /// `corner_radius`/`stroke` are elided from saves entirely.
    Shape {
        shape: Shape,
        /// Fill color. Old projects without this field default to white.
        #[serde(default = "default_shape_rgba")]
        rgba: Param<[u8; 4]>,
        #[serde(default = "default_shape_width")]
        width: Param<f32>,
        #[serde(default = "default_shape_height")]
        height: Param<f32>,
        /// Corner rounding in reference pixels. Shapes with corners
        /// (rectangle, polygon, star) honor it; curved shapes ignore it.
        #[serde(default = "zero_param", skip_serializing_if = "is_zero_param")]
        corner_radius: Param<f32>,
        /// Outline centered on the shape edge (half in, half out).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stroke: Option<ShapeStroke>,
    },
    /// An image or animated sticker from the bundled catalog
    /// ([`crate::sticker::sticker_catalog`]). `asset` is a catalog id. The
    /// empty string — what payload-less pre-catalog projects deserialize to —
    /// is valid and renders nothing, exactly like the old unit variant.
    Sticker { asset: String },
    /// A Lottie vector animation, file-backed: `path` points at a `.json`
    /// on disk (downloaded from the asset catalog or user-imported),
    /// path-referenced like media rather than embedded like [`Sticker`].
    ///
    /// `width`/`height` are the composition's intrinsic size, captured when
    /// the generator is created so the resolver can place the layer without
    /// touching the filesystem (they are *reference pixels* on a 1080p
    /// canvas, the sticker convention). A missing or unreadable file renders
    /// nothing — the media offline story, never an error.
    ///
    /// [`Sticker`]: Generator::Sticker
    Lottie {
        path: String,
        width: u32,
        height: u32,
    },
    /// Motion / composited VFX layer (implementation TBD).
    Effect,
    /// Blur, mask, and similar pixel filters (implementation TBD).
    Filter,
    /// Color grade / pass-through layer affecting tracks beneath it.
    Adjustment,
}

/// Hand-written for one reason: sticker back-compat. `Sticker` used to be a
/// unit variant (the bare JSON string `"Sticker"`); it now carries `{asset}`.
/// A derived externally-tagged enum can't accept both shapes, so this looks
/// at the input first: a bare string is one of the unit variants (including
/// the legacy sticker), a map is the current tagged form, delegated to a
/// derived mirror enum. Projects persist as JSON (self-describing), so
/// `deserialize_any` is safe here.
impl<'de> Deserialize<'de> for Generator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// Field-for-field mirror of [`Generator`]'s tagged (map) forms,
        /// carrying the deserialize-side serde attributes. The round-trip
        /// test (`generator_roundtrips_through_custom_deserialize`) breaks
        /// if it drifts from the real enum.
        #[derive(Deserialize)]
        #[serde(rename = "Generator")]
        enum Tagged {
            Text {
                content: String,
                #[serde(default)]
                style: TextStyle,
            },
            SolidColor {
                rgba: [u8; 4],
            },
            Shape {
                shape: Shape,
                #[serde(default = "default_shape_rgba")]
                rgba: Param<[u8; 4]>,
                #[serde(default = "default_shape_width")]
                width: Param<f32>,
                #[serde(default = "default_shape_height")]
                height: Param<f32>,
                #[serde(default = "zero_param")]
                corner_radius: Param<f32>,
                #[serde(default)]
                stroke: Option<ShapeStroke>,
            },
            Sticker {
                #[serde(default)]
                asset: String,
            },
            Lottie {
                path: String,
                width: u32,
                height: u32,
            },
            Effect,
            Filter,
            Adjustment,
        }

        impl From<Tagged> for Generator {
            fn from(t: Tagged) -> Generator {
                match t {
                    Tagged::Text { content, style } => Generator::Text { content, style },
                    Tagged::SolidColor { rgba } => Generator::SolidColor { rgba },
                    Tagged::Shape {
                        shape,
                        rgba,
                        width,
                        height,
                        corner_radius,
                        stroke,
                    } => Generator::Shape {
                        shape,
                        rgba,
                        width,
                        height,
                        corner_radius,
                        stroke,
                    },
                    Tagged::Sticker { asset } => Generator::Sticker { asset },
                    Tagged::Lottie {
                        path,
                        width,
                        height,
                    } => Generator::Lottie {
                        path,
                        width,
                        height,
                    },
                    Tagged::Effect => Generator::Effect,
                    Tagged::Filter => Generator::Filter,
                    Tagged::Adjustment => Generator::Adjustment,
                }
            }
        }

        const VARIANTS: &[&str] = &[
            "Text",
            "SolidColor",
            "Shape",
            "Sticker",
            "Lottie",
            "Effect",
            "Filter",
            "Adjustment",
        ];

        struct GeneratorVisitor;

        impl<'de> serde::de::Visitor<'de> for GeneratorVisitor {
            type Value = Generator;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a Generator variant (bare string or single-key map)")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Generator, E> {
                match v {
                    // Legacy payload-less sticker (pre-catalog projects).
                    "Sticker" => Ok(Generator::Sticker {
                        asset: String::new(),
                    }),
                    "Effect" => Ok(Generator::Effect),
                    "Filter" => Ok(Generator::Filter),
                    "Adjustment" => Ok(Generator::Adjustment),
                    other => Err(E::unknown_variant(other, VARIANTS)),
                }
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Generator, A::Error> {
                Tagged::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(Generator::from)
            }
        }

        deserializer.deserialize_any(GeneratorVisitor)
    }
}

/// The geometry of a [`Generator::Shape`]: parametric figures the compositor
/// evaluates as GPU signed-distance fields, or a pen-tool bezier
/// [`ShapePath`] rasterized on the CPU. The legacy `Rectangle`/`Ellipse`
/// unit variants keep their serialized names, so old projects load
/// unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Rectangle,
    Ellipse,
    /// Regular polygon (`sides >= 3`; a triangle is `sides: 3`), fit to the
    /// generator's `width`×`height` box.
    Polygon {
        sides: u32,
    },
    /// Star with `points` spikes (`>= 3`); `inner_ratio` is the inner-vertex
    /// radius as a fraction of the outer (`0..=1` — small is spiky, large is
    /// blunt), animatable.
    Star {
        points: u32,
        #[serde(default = "default_star_inner")]
        inner_ratio: Param<f32>,
    },
    /// A horizontal capsule (round-capped line) spanning the box; the box
    /// height is the line thickness. Rotate via the clip transform.
    Line,
    /// A right-pointing arrow filling the box.
    Arrow,
    /// An upright heart fit to the box.
    Heart,
    /// A custom pen-tool outline (cubic beziers).
    Path(ShapePath),
}

/// A pen-tool outline: anchors with absolute cubic handles, in shape-local
/// reference pixels around the shape's center (the pen tool normalizes a
/// committed path so its bounds center the origin — that point lands on the
/// clip's placed center). Point geometry is edited structurally via
/// `SetGenerator` (not keyframed); the fill/stroke/transform animate like
/// any other shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapePath {
    pub points: Vec<ShapePathPoint>,
    /// Closed paths fill; open paths draw stroke only.
    pub closed: bool,
}

/// One pen-tool anchor. A handle equal to its anchor makes that side of the
/// point a sharp corner (the "click without drag" pen gesture).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapePathPoint {
    pub anchor: [f32; 2],
    /// Control of the segment arriving at this anchor.
    pub handle_in: [f32; 2],
    /// Control of the segment leaving this anchor.
    pub handle_out: [f32; 2],
}

impl ShapePathPoint {
    /// A corner point: both handles collapsed onto the anchor.
    pub fn corner(anchor: [f32; 2]) -> Self {
        Self {
            anchor,
            handle_in: anchor,
            handle_out: anchor,
        }
    }
}

/// Outline drawn on a shape's edge, centered on it. Color and width are
/// animatable [`Param`]s (constants serialize bare).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeStroke {
    pub rgba: Param<[u8; 4]>,
    pub width: Param<f32>,
}

/// The stroke a shape is given when one is added, and what a reset returns
/// its width and colour to: opaque black, 4 reference pixels wide.
impl Default for ShapeStroke {
    fn default() -> Self {
        Self::new([0, 0, 0, 255], 4.0)
    }
}

impl ShapeStroke {
    /// A constant stroke.
    pub fn new(rgba: [u8; 4], width: f32) -> Self {
        Self {
            rgba: Param::Constant(rgba),
            width: Param::Constant(width),
        }
    }
}

/// Default fill color for a shape without one (opaque white).
pub(super) fn default_shape_rgba() -> Param<[u8; 4]> {
    Param::Constant([255, 255, 255, 255])
}

/// Default width for shapes missing the field — reproduces the legacy
/// centered 50%-of-canvas geometry on a 1920×1080 project.
fn default_shape_width() -> Param<f32> {
    Param::Constant(960.0)
}

/// Default height for shapes missing the field — same legacy geometry.
fn default_shape_height() -> Param<f32> {
    Param::Constant(540.0)
}

pub(super) fn default_star_inner() -> Param<f32> {
    Param::Constant(0.5)
}

pub(crate) use crate::param::{default_zero_param as zero_param, is_zero_param};

/// Size of a freshly dropped shape (reference pixels @ 1080 canvas height).
pub const SHAPE_DROP_WIDTH: f32 = 200.0;
pub const SHAPE_DROP_HEIGHT: f32 = 200.0;

/// Largest shape extent / path coordinate magnitude, in reference pixels.
/// Sanity bound only — SDF shapes are resolution-independent, but a runaway
/// value would still blow up path rasters and content-box math.
pub const MAX_SHAPE_DIM: f32 = 8192.0;

/// Most spikes a star (or sides a polygon) may have. Mirrors the evaluator
/// bound in `aria-shapes` (`MAX_STAR_POINTS`), which sizes the fixed
/// vertex buffers on the CPU and in WGSL; a render-side test pins the two
/// constants together.
pub const MAX_STAR_POINTS: u32 = 20;

/// Widest stroke, in reference pixels.
pub const MAX_STROKE_WIDTH: f32 = 512.0;

impl Generator {
    /// A text generator with the default style. Convenience for the common
    /// case of creating a freshly-dropped title.
    pub fn text(content: impl Into<String>) -> Self {
        Generator::Text {
            content: content.into(),
            style: TextStyle::default(),
        }
    }

    /// A sticker generator referencing a bundled catalog asset.
    pub fn sticker(asset: impl Into<String>) -> Self {
        Generator::Sticker {
            asset: asset.into(),
        }
    }

    /// A file-backed Lottie generator. `width`/`height` are the
    /// composition's intrinsic size (probe the file before calling).
    pub fn lottie(path: impl Into<String>, width: u32, height: u32) -> Self {
        Generator::Lottie {
            path: path.into(),
            width,
            height,
        }
    }

    /// A shape generator with the default drop size and fill color.
    pub fn shape(shape: Shape, rgba: [u8; 4]) -> Self {
        Generator::Shape {
            shape,
            rgba: Param::Constant(rgba),
            width: Param::Constant(SHAPE_DROP_WIDTH),
            height: Param::Constant(SHAPE_DROP_HEIGHT),
            corner_radius: zero_param(),
            stroke: None,
        }
    }

    /// Resolve catalog presets into concrete fields — currently the text
    /// style's `effect_preset`: validate the id and bake the catalog's
    /// stroke / shadow / background onto the style. No-op for other
    /// generators and preset-less styles. Called by
    /// [`crate::Project::add_generated`], and by
    /// [`crate::Project::set_generator`] when the preset differs from the one
    /// the clip holds, so every platform gets identical baked fields from one
    /// source of truth and a treatment edited after the preset is kept.
    pub fn resolve_presets(&mut self) -> Result<(), ModelError> {
        let Generator::Text { style, .. } = self else {
            return Ok(());
        };
        let Some(preset) = &style.effect_preset else {
            return Ok(());
        };
        let spec = crate::look::text_effect_spec(preset)
            .ok_or_else(|| ModelError::InvalidParam(format!("unknown text effect '{preset}'")))?;
        style.stroke = spec.stroke.clone();
        style.shadow = spec.shadow.clone();
        style.background = spec.background.clone();
        Ok(())
    }

    /// This generator's capability row: the lane it belongs on, the layer
    /// topologies it resolves to, the inspector blocks it opens, and the routes
    /// to its structural check and its keyframe rebase.
    ///
    /// The rows are defined in `generator_catalog`, one per variant, and that
    /// file's exhaustive match is what stops a new generator from shipping
    /// without answers.
    pub const fn spec(&self) -> &'static GeneratorSpec {
        generator_spec(self)
    }

    /// `Ok` iff the generator's content is structurally sound and every
    /// stored value (constants and keyframes) is in range. Enforced by
    /// [`crate::Project::add_generated`] / [`crate::Project::set_generator`]
    /// so a project never holds generated content the renderer would have to
    /// clamp.
    ///
    /// The check is the `validate` column of this generator's row, so a variant
    /// cannot ship without declaring one — even if what it declares is that
    /// there is nothing to check.
    pub fn validate(&self) -> Result<(), ModelError> {
        (self.spec().validate)(self)
    }
}

impl ShapePath {
    /// `Ok` iff the path is drawable (>= 2 points) with finite, bounded
    /// coordinates.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.points.len() < 2 {
            return Err(ModelError::InvalidParam(
                "shape path needs at least 2 points".into(),
            ));
        }
        for p in &self.points {
            for v in [p.anchor, p.handle_in, p.handle_out] {
                if !v.iter().all(|c| c.is_finite() && c.abs() <= MAX_SHAPE_DIM) {
                    return Err(ModelError::InvalidParam(format!(
                        "shape path coordinate out of range (|v| <= {MAX_SHAPE_DIM})"
                    )));
                }
            }
        }
        Ok(())
    }
}
