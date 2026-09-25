//! Plane textures backing one composited layer.
//!
//! A video frame needs two (NV12) or three (I420) plane textures, a bitmap or
//! glyph atlas one. The count is bounded and known from the pixel format, so
//! the set is held inline: the compositor builds one of these per layer per
//! frame, and a `Vec` here was a heap allocation and free sixty times a second
//! for three pointer-sized handles.
//!
//! Which variant a frame produced is also *what the shader needs to know*:
//! [`PlaneSet::yuv_views`] hands NV12's interleaved CbCr texture to both chroma
//! slots, so callers never track a separate biplanar flag alongside the
//! textures.

/// One plane: the texture and the view sampled from it. Both are kept — the
/// bind group retains them internally, and holding them beside it makes the
/// lifetime of the upload unmistakable.
pub(crate) type PlaneTexture = (wgpu::Texture, wgpu::TextureView);

/// The plane textures of one layer.
pub(crate) enum PlaneSet {
    /// A single packed texture: an RGBA bitmap or a glyph atlas.
    ///
    /// Nothing reads it back: the bind group was built from the view before
    /// this set existed, so the payload is retention only — it keeps the
    /// texture alive for as long as the frame that samples it. `expect`
    /// rather than `allow`, so the day a caller does read it the attribute
    /// goes stale and says so.
    Single(
        #[expect(
            dead_code,
            reason = "retained so the sampled texture outlives the frame"
        )]
        PlaneTexture,
    ),
    /// Luma plus interleaved chroma (NV12).
    Biplanar { y: PlaneTexture, uv: PlaneTexture },
    /// Luma plus separate Cb and Cr planes (I420).
    Triplanar {
        y: PlaneTexture,
        u: PlaneTexture,
        v: PlaneTexture,
    },
}

impl PlaneSet {
    /// The `(Y, U, V)` views the YUV pipelines bind.
    ///
    /// NV12 has no third plane, so its CbCr texture is bound in both chroma
    /// slots; the shader reads only the one its plane mode selects. `None` for
    /// a packed single texture, which no YUV pipeline can sample.
    pub(crate) fn yuv_views(
        &self,
    ) -> Option<(&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView)> {
        match self {
            PlaneSet::Single(_) => None,
            PlaneSet::Biplanar { y, uv } => Some((&y.1, &uv.1, &uv.1)),
            PlaneSet::Triplanar { y, u, v } => Some((&y.1, &u.1, &v.1)),
        }
    }

    /// Whether chroma is interleaved (NV12) — the YUV shader's plane mode.
    pub(crate) fn is_biplanar(&self) -> bool {
        matches!(self, PlaneSet::Biplanar { .. })
    }
}
