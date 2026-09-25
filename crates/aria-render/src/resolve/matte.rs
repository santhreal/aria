//! Track mattes: reading a clip's matte settings at the tick on screen, and
//! folding each matted layer's neighbour above into a single matte wrapper.
//!
//! Moved out of `resolve.rs` to stay under the file ceiling; nothing changed.

use super::*;

/// A clip's matte settings, sampled at the tick on screen.
pub(super) struct MatteRequest {
    read: MatteSource,
    invert: bool,
    softness: f32,
}

/// The matte a resolved layer asks for, if its clip carries one.
pub(super) fn matte_request(
    project: &Project,
    layer: &SceneLayer,
    t: RationalTime,
) -> Option<MatteRequest> {
    let clip = project.clip(layer.clip?)?;
    let matte = clip.matte.as_ref()?;
    let local = t.value - clip.timeline.start.value;
    Some(MatteRequest {
        read: matte.source,
        invert: matte.invert,
        softness: matte.softness.sample(local),
    })
}

/// Consume each matted layer's neighbour above into a matte wrapper.
///
/// Bottom-up, so the layer directly above is the stencil and leaves the
/// composite: two layers in, one wrapper out. A stencil's own matte request is
/// dropped rather than chained — it is being read, not drawn. A matted layer
/// with nothing above it keeps drawing unmatted, since a missing stencil is a
/// stack the operator is still building, not a reason to blank the frame.
pub(super) fn fold_mattes(
    layers: Vec<SceneLayer>,
    requests: Vec<Option<MatteRequest>>,
    cw: f32,
    ch: f32,
) -> Vec<SceneLayer> {
    let mut out = Vec::with_capacity(layers.len());
    let mut layers = layers.into_iter();
    let mut requests = requests.into_iter();
    while let Some(layer) = layers.next() {
        let Some(request) = requests.next().flatten() else {
            out.push(layer);
            continue;
        };
        let Some(stencil) = layers.next() else {
            out.push(layer);
            continue;
        };
        let _stencil_request = requests.next();
        // A canvas-wide pass has no picture of its own to read or to cut, so
        // pairing one with a matte would ask the compositor for a nested pass.
        if layer.source.is_canvas_pass() || stencil.source.is_canvas_pass() {
            out.push(layer);
            out.push(stencil);
            continue;
        }
        let blend_mode = layer.blend_mode;
        let clip = layer.clip;
        out.push(SceneLayer {
            clip,
            source: LayerSource::Matte {
                layers: Box::new([layer, stencil]),
                read: request.read,
                invert: request.invert,
                softness: request.softness.clamp(0.0, 1.0),
            },
            center: [cw * 0.5, ch * 0.5],
            anchor_point: [0.5, 0.5],
            size: SizeSpec::Fixed([cw, ch]),
            rotation: 0.0,
            opacity: 1.0,
            uv: [0.0, 0.0, 1.0, 1.0],
            effects: Vec::new(),
            mask: None,
            chroma_key: None,
            color_grade: None,
            lut: None,
            blend_mode,
            styles: None,
            blur_passes: Vec::new(),
        });
    }
    out
}
