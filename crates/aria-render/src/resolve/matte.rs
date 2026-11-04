//! Track mattes: reading a clip's matte settings at the tick on screen, and
//! folding each matted layer's neighbour above into a single matte wrapper.

use super::*;

/// A clip's matte settings, sampled at the tick on screen.
pub(super) struct MatteRequest {
    read: MatteSource,
    invert: bool,
    softness: f32,
}

/// The matte a resolved layer asks for, if its clip carries one, after the
/// clip's drivers.
pub(super) fn matte_request<'p>(
    project: &'p Project,
    layer: &SceneLayer,
    t: RationalTime,
    drivers: &mut DriverFrame<'p>,
) -> Option<MatteRequest> {
    let clip = drivers.apply(project.clip(layer.clip?)?, t.value as f64);
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
/// stack still under construction, not a reason to blank the frame.
pub(super) fn fold_mattes_into(
    layers: &mut Vec<SceneLayer>,
    requests: &mut Vec<Option<MatteRequest>>,
    cw: f32,
    ch: f32,
    pool: &mut ResolvePool,
) {
    if !requests.iter().any(Option::is_some) {
        return;
    }
    let mut out = std::mem::take(&mut pool.layers_scratch);
    out.clear();
    let mut layers_iter = layers.drain(..);
    let mut requests_iter = requests.drain(..);
    while let Some(layer) = layers_iter.next() {
        let Some(request) = requests_iter.next().flatten() else {
            out.push(layer);
            continue;
        };
        let Some(stencil) = layers_iter.next() else {
            out.push(layer);
            continue;
        };
        let _stencil_request = requests_iter.next();
        // A canvas-wide pass has no picture of its own to read or to cut, so
        // pairing one with a matte would ask the compositor for a nested pass.
        if layer.source.is_canvas_pass() || stencil.source.is_canvas_pass() {
            out.push(layer);
            out.push(stencil);
            continue;
        }
        let blend_mode = layer.blend_mode;
        let clip = layer.clip;
        let pair = pool.box_layer_pair(layer, stencil);
        out.push(SceneLayer {
            clip,
            source: LayerSource::Matte {
                layers: pair,
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
            masks: Vec::new(),
            chroma_key: None,
            color_grade: None,
            lut: None,
            blend_mode,
            styles: None,
            blur_passes: Vec::new(),
        });
    }
    drop(layers_iter);
    drop(requests_iter);
    std::mem::swap(layers, &mut out);
    pool.layers_scratch = out;
}
