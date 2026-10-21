//! Layers that cover their whole target with opaque pixels.
//!
//! A video frame or solid fill drawn at full opacity writes alpha 1, and
//! straight-alpha src-over of an alpha-1 source yields the source color
//! exactly. When its placed quad also contains every corner of clip space,
//! the layer writes every pixel of its target, so it draws through a pipeline
//! with blending disabled and a pass that draws it first leaves the target
//! unloaded.

/// Whether the quad placed by `linear` and `trans`, as
/// [`placement_affine_rot`](super::upload::placement_affine_rot) returns
/// them, covers its whole target at full opacity.
///
/// A corner of clip space lies inside the quad when the inverse placement
/// maps it into the unit quad `[-0.5, 0.5]²`. Every pixel center lies at
/// least half a pixel inside those corners, so containing the corners
/// contains every pixel the rasterizer samples.
pub(super) fn covers_target(linear: [f32; 4], trans: [f32; 4]) -> bool {
    if trans[2] != 1.0 {
        return false;
    }
    // Columns (m00, m10) and (m01, m11) map a unit-quad corner into clip
    // space, as the layer vertex shaders apply them.
    let [m00, m10, m01, m11] = linear;
    let det = m00 * m11 - m01 * m10;
    if !det.is_normal() {
        return false;
    }
    [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]]
        .into_iter()
        .all(|[x, y]: [f32; 2]| {
            let (dx, dy) = (x - trans[0], y - trans[1]);
            let u = (m11 * dx - m01 * dy) / det;
            let v = (m00 * dy - m10 * dx) / det;
            u.abs() <= 0.5 && v.abs() <= 0.5
        })
}

#[cfg(test)]
#[path = "cover_tests.rs"]
mod tests;
