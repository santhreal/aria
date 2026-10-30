//! Which CPU rasterizer a layer topology's bitmap comes from.
//!
//! Each `ShippedLayer` row declares a [`CpuRaster`] value in its `cpu_raster`
//! column. The sizing path in `render::api` matches exhaustively on the three
//! variants to select the rasterizer for a topology's payload.

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
