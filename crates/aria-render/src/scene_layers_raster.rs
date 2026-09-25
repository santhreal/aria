//! Which CPU rasterizer a layer topology's bitmap comes from.
//!
//! Split out of the table for the file-size ceiling, and kept as its own type
//! rather than a `bool` because the sizing path in `render::api` needs more than
//! "yes, a bitmap": it needs to know *which* rasterizer to ask. That question
//! used to be answered by a `match` over [`LayerSource`] with a `_ => return
//! None` arm, so a twelfth topology that rasterized on the CPU inherited
//! "unsizeable" in silence.
//!
//! Now it is a column. A row names its rasterizer, the sizing path matches on
//! the column (three variants, exhaustive), and pulling the payload out of the
//! one variant that carries it is the only place a single-variant pattern
//! appears — a new topology has no answer to give there, and does not need one.

/// Which CPU rasterizer produces a topology's bitmap.
///
/// [`Self::None`] is not "unknown": it is the positive statement that the
/// topology's pixels come from a decoder, a shader, or the layers nested inside
/// it, so there is no CPU raster to measure or reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuRaster {
    /// No CPU raster: a decoded frame, a GPU signed-distance field, a solid
    /// fill, a canvas pass, or a wrapper around other layers.
    None,
    /// The text rasterizer, from the run's content and style.
    Text,
    /// The path rasterizer, from the bezier path and its fill/stroke style.
    Path,
}

impl CpuRaster {
    /// Whether realize rasterizes this topology on the CPU at all.
    pub const fn is_cpu(self) -> bool {
        !matches!(self, Self::None)
    }
}
