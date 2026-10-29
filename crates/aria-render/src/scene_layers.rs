//! Every layer topology, as data.
//!
//! A layer topology is the change class that touches five crates at once —
//! [`LayerSource`], `Realized`, `LayerJob`, `CompositorLayer`, and a shader — and
//! it used to be an enum variant plus a hand-written predicate wherever
//! something cared. Those predicates were scattered as `matches!` calls: the
//! resolver asked twice whether a layer was a canvas pass before pairing it into
//! a matte, once more before a transition, once more before attaching layer
//! styles; motion blur asked whether it was a canvas pass *or* a transition; the
//! realize conformance suite asked which topologies its sub-scene dispatch
//! refuses. Nothing tied them together, so a topology could be added and quietly
//! inherit every default answer.
//!
//! A row here carries the topology's stable id and each of those questions as a
//! column. The realize conformance suite reads this table instead of parsing the
//! `LayerSource` declaration out of `scene.rs` as text, which is what it had to
//! do while the axis was not data.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`LayerSource`], so a new topology
//! cannot compile until it names a row. `a_row_per_layer_topology`
//! proves the arm points at that topology's own row rather than reusing a
//! neighbour's, by round-tripping every row's own [`LayerSourceSpec::census`]
//! sample.

use aria_core::RationalTime;
use aria_models::{MatteSource, MediaId};
use aria_shapes::{BezierPath, SdfParams, Stroke};
use aria_text::TextStyle;

use crate::scene::{Scene, SceneLayer, TextAnimation};

/// The pixel source for a [`SceneLayer`].
#[derive(Debug, Clone, PartialEq)]
pub enum LayerSource {
    /// A decoded video frame: decode `media` at `source_time` and place it.
    Media {
        media: MediaId,
        source_time: RationalTime,
    },
    /// A still image: decode `media`'s single frame once (the renderer
    /// caches it) and place it for the clip's whole extent.
    Still { media: MediaId },
    /// A bundled sticker (static or animated): the renderer decodes the
    /// catalog asset once into a frame sequence and picks the frame at
    /// `local_time`, looping.
    Sticker {
        /// Catalog id (see [`aria_models::sticker_catalog`]).
        asset: String,
        /// Seconds since the clip's timeline start.
        local_time: f64,
    },
    /// A file-backed Lottie animation: the renderer parses `path` once and
    /// rasterizes the capped-fps frame at `local_time` on demand (LRU-cached,
    /// looping). A missing/unparseable file draws nothing.
    Lottie {
        /// Absolute path to the `.json` on disk.
        path: String,
        /// Seconds since the clip's timeline start.
        local_time: f64,
    },
    /// A text run. When `animation` is `Some`, realize draws per-character
    /// instanced glyphs; otherwise the whole run is rasterized as one bitmap.
    Text {
        content: String,
        style: TextStyle,
        animation: Option<TextAnimation>,
    },
    /// A solid RGBA fill across the placed quad.
    Solid([u8; 4]),
    /// Apply this layer's effect chain and color grade to the current canvas.
    ///
    /// Lane-level effect/filter/adjustment generator bars use this geometry-free
    /// marker to process everything already drawn below their track.
    CanvasPass,
    /// A parametric vector shape, every animatable parameter already sampled
    /// at this instant (canvas pixels). Realized as a GPU SDF layer: the
    /// layer's `size` is the *padded quad* (shape + stroke overhang + AA);
    /// `params` + the fill/stroke style travel to the fragment shader as
    /// uniforms.
    Shape {
        /// Size-free shape parameters (the shape's pixel box is derived from
        /// the layer's padded `size` minus `pad`).
        params: SdfParams,
        /// Straight-alpha fill; alpha 0 means stroke-only.
        fill: [u8; 4],
        /// Optional centered outline (width in canvas pixels).
        stroke: Option<Stroke>,
        /// Padding per side between the shape box and the placed quad
        /// (stroke overhang + AA margin), in canvas pixels.
        pad: f32,
    },
    /// A pen-tool bezier path (shape-local pixels), rasterized on the CPU at
    /// `raster_scale` and composited as a bitmap like text.
    PathShape {
        path: BezierPath,
        fill: [u8; 4],
        stroke: Option<Stroke>,
        /// Path-local px → canvas px factor folded into the raster
        /// (reference scale; the clip's animated transform scale rides the
        /// quad via [`crate::SizeSpec::BitmapScaled`]).
        raster_scale: f32,
    },
    /// A track transition between two abutting clips, sampled at `progress`.
    Transition {
        outgoing: Box<SceneLayer>,
        incoming: Box<SceneLayer>,
        transition_id: String,
        /// `0.0` = fully outgoing, `1.0` = fully incoming.
        progress: f32,
    },
    /// A track matte: `layers[1]` is not drawn; it selects where `layers[0]`
    /// survives. The layer above the matted clip becomes the stencil at resolve
    /// time and leaves the top-level list, which is why one wrapper covers the
    /// canvas where two layers used to. The fixed pair uses one recursive
    /// allocation for the matte rather than one allocation per nested layer.
    Matte {
        /// `[content, stencil]`.
        layers: Box<[SceneLayer; 2]>,
        /// Which channel of the stencil is read.
        read: MatteSource,
        /// Keep where the stencil is transparent (or black) instead of opaque.
        invert: bool,
        /// Edge softness in `0.0..=1.0`; `0` uses the stencil's own edge.
        softness: f32,
    },
    /// A nested sequence: its frame at this instant, resolved into a scene of
    /// its own, drawn apart from the canvas, and placed as one picture. The
    /// layer's UV rect is over the nest scene's canvas.
    Nest {
        /// The sequence's frame on a canvas the size of the one the nest is
        /// placed on, over a transparent background: fitted inside it and
        /// centered, at the time the nest's clip shows.
        scene: Box<Scene>,
        /// Where the sequence's own frame sits on that canvas, `[x0, y0, x1,
        /// y1]` as fractions of its width and height.
        frame: [f32; 4],
    },
}

impl LayerSource {
    /// This topology's capability row: whether it paints, whether it decodes
    /// media, whether it reprocesses the canvas, whether it wraps other layers,
    /// and what the sub-scene dispatch does with it.
    ///
    /// The rows are defined in this module, one per topology, and the
    /// exhaustive match stops a new topology from shipping
    /// without answers. It is also what the realize conformance suite reads: it
    /// used to parse this enum's declaration out of `scene.rs` as text, because
    /// the axis was not data.
    pub const fn spec(&self) -> &'static LayerSourceSpec {
        layer_source_spec(self)
    }

    /// The topology's stable name.
    pub const fn kind(&self) -> &'static str {
        self.spec().id
    }

    /// Whether this layer reprocesses the whole canvas instead of placing
    /// content of its own.
    pub const fn is_canvas_pass(&self) -> bool {
        self.spec().is_canvas_pass
    }

    /// Whether this layer can be lifted out of the stack as a single-clip
    /// sprite for a drag gesture.
    pub const fn is_sprite(&self) -> bool {
        self.spec().sprite_partitionable
    }

    /// Whether this layer is eligible for transform supersampling.
    pub const fn takes_motion_blur(&self) -> bool {
        self.spec().takes_motion_blur
    }

    /// Whether this layer carries a blend mode and the quad style blocks.
    pub const fn takes_layer_styles(&self) -> bool {
        self.spec().takes_layer_styles
    }
}

/// How the sub-scene dispatch in `render::realize::layers` treats a topology
/// nested inside a transition or a matte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nested {
    /// Realized there too, drawing the same pixels it draws at the top level.
    Paints,
    /// Refused there. The refusal names the topology, so a nested wrapper never
    /// fails with a message that states no corrective action.
    Refuses,
}

/// One layer topology.
///
/// Every column is a question the resolver, the motion-blur pass, or the realize
/// dispatch used to answer with its own `matches!`.
#[derive(Debug, Clone, Copy)]
pub struct LayerSourceSpec {
    /// The topology's stable name. What [`LayerSource::kind`] returns, what a
    /// generator row's `renders_with` names, and what a refusal message spells
    /// out with its hyphens turned into spaces.
    pub id: &'static str,
    /// Whether the layer draws its own pixels onto the canvas. A canvas pass
    /// has no geometry: it reprocesses what is already composited below it.
    pub paints: bool,
    /// Whether realizing the layer decodes project media.
    pub needs_media: bool,
    /// Whether the layer reprocesses the whole canvas instead of placing
    /// content. Only such a layer forces the resolver to give up sprite
    /// partitioning, refuse a matte or transition pairing, and skip layer
    /// styles.
    pub is_canvas_pass: bool,
    /// Whether the layer holds whole [`SceneLayer`]s that draw on the canvas in
    /// its place, so a canvas rescale has to recurse into them. A nest is not
    /// a wrapper: its layers draw on a canvas of their own, and the nest
    /// places that picture as one layer.
    pub wraps_layers: bool,
    /// Whether the layer is eligible for transform supersampling. A canvas pass
    /// has no transform, and a wrapper takes none of its own: the layers inside
    /// it are sampled instead.
    pub takes_motion_blur: bool,
    /// Whether the layer carries a blend mode and the quad styles (shadow,
    /// glow, outline, background). A canvas pass has no quad; a wrapper stays
    /// `Normal` and `None` so the styles ride the layers inside it.
    pub takes_layer_styles: bool,
    /// Which CPU rasterizer produces the layer's bitmap, when one does. Read by
    /// the generator sizing path to pick a rasterizer rather than to ask a
    /// yes/no question, so a topology that grows a third raster kind adds a
    /// [`CpuRaster`] variant and turns that dispatch red instead of silently
    /// measuring as nothing.
    pub cpu_raster: CpuRaster,
    /// Whether a gesture can lift a layer of this topology out of the stack as
    /// a single-clip sprite, leaving the rest of the canvas to render below and
    /// above it. A canvas pass reprocesses everything under it, and a
    /// transition is not one clip, so neither can be partitioned out. A matte
    /// wrapper can: it composites to one quad that stands in for its content.
    pub sprite_partitionable: bool,
    /// What the sub-scene dispatch does with the topology.
    pub nested: Nested,
    /// One representative value of this variant, for tests and sweeps that need
    /// a topology of every kind without hand-listing them.
    pub census: fn() -> LayerSource,
}

/// The shipped layer topologies, in declaration order.
pub(crate) const SHIPPED: &[LayerSourceSpec] = &[
    LayerSourceSpec {
        id: "media",
        paints: true,
        needs_media: true,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_media,
    },
    LayerSourceSpec {
        id: "still",
        paints: true,
        needs_media: true,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_still,
    },
    LayerSourceSpec {
        id: "sticker",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_sticker,
    },
    LayerSourceSpec {
        id: "lottie",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_lottie,
    },
    LayerSourceSpec {
        id: "text",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::Text,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_text,
    },
    LayerSourceSpec {
        id: "solid",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_solid,
    },
    LayerSourceSpec {
        id: "canvas-pass",
        paints: false,
        needs_media: false,
        is_canvas_pass: true,
        wraps_layers: false,
        takes_motion_blur: false,
        takes_layer_styles: false,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: false,
        nested: Nested::Refuses,
        census: census_canvas_pass,
    },
    LayerSourceSpec {
        id: "shape",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_shape,
    },
    LayerSourceSpec {
        id: "path-shape",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::Path,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_path_shape,
    },
    LayerSourceSpec {
        id: "transition",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: true,
        takes_motion_blur: false,
        takes_layer_styles: false,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: false,
        nested: Nested::Refuses,
        census: census_transition,
    },
    LayerSourceSpec {
        id: "matte",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: true,
        takes_motion_blur: false,
        takes_layer_styles: false,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Refuses,
        census: census_matte,
    },
    LayerSourceSpec {
        id: "nest",
        paints: true,
        needs_media: false,
        is_canvas_pass: false,
        wraps_layers: false,
        takes_motion_blur: true,
        takes_layer_styles: true,
        cpu_raster: CpuRaster::None,
        sprite_partitionable: true,
        nested: Nested::Paints,
        census: census_nest,
    },
];

/// Which row in [`SHIPPED`] describes `source`.
///
/// Exhaustive on purpose, and the whole fail-by-default mechanism for this axis:
/// a topology added to `scene_layers.rs` and nowhere else does not compile.
const fn index_of(source: &LayerSource) -> usize {
    match source {
        LayerSource::Media { .. } => 0,
        LayerSource::Still { .. } => 1,
        LayerSource::Sticker { .. } => 2,
        LayerSource::Lottie { .. } => 3,
        LayerSource::Text { .. } => 4,
        LayerSource::Solid(_) => 5,
        LayerSource::CanvasPass => 6,
        LayerSource::Shape { .. } => 7,
        LayerSource::PathShape { .. } => 8,
        LayerSource::Transition { .. } => 9,
        LayerSource::Matte { .. } => 10,
        LayerSource::Nest { .. } => 11,
    }
}

/// Every layer topology, in declaration order.
///
/// The realize conformance suite sweeps this instead of parsing `scene.rs` for
/// an enum declaration, so a topology added without a pixel proof turns that
/// suite red.
/// No production reader enumerates the table; the sweeps that do are suites.
#[cfg(test)]
pub(crate) const fn layer_source_specs() -> &'static [LayerSourceSpec] {
    SHIPPED
}

/// The row describing `source`.
pub(crate) const fn layer_source_spec(source: &LayerSource) -> &'static LayerSourceSpec {
    &SHIPPED[index_of(source)]
}

/// The row with this topology id. A lookup by id is what a suite sweeping the
/// table needs; production already holds the `LayerSource` and asks
/// [`layer_source_spec`].
#[cfg(test)]
pub(crate) fn layer_source_by_id(id: &str) -> Option<&'static LayerSourceSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

#[path = "scene_layers_raster.rs"]
mod raster;

pub(crate) use raster::CpuRaster;

#[path = "scene_layers_census.rs"]
mod census;

use census::{
    census_canvas_pass, census_lottie, census_matte, census_media, census_nest, census_path_shape,
    census_shape, census_solid, census_sticker, census_still, census_text, census_transition,
};

#[cfg(test)]
#[path = "scene_layers_tests.rs"]
mod tests;
