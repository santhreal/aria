//! Clips a registered effect reads through a layer parameter.
//!
//! An effect pass that binds `PassInput::Layer(param)` reads the frame of the
//! clip that parameter references, placed on the canvas as it would composite
//! at the same instant. Each referenced clip resolves once per frame into
//! [`Scene::references`], in an order where every clip comes after the clips
//! its own effects reference, so the renderer draws a reference before the
//! effect that reads it.

use super::*;

/// The longest chain of references that resolves. The model refuses cycles
/// when a parameter is set; this bounds a project edited outside the model.
const MAX_REFERENCE_DEPTH: usize = 8;

/// The clips on `timeline` that `layers` read by layer reference, resolved at
/// `t`, each after the clips it reads itself. A clip that is missing, on
/// another timeline, outside its span at `t`, disabled, silent, or a canvas
/// pass resolves to nothing, and the effect reading it reads a transparent
/// frame. A layer inside a nest reads clips of the nest's own timeline, which
/// the nest's own scene resolves.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_references<'p>(
    project: &'p Project,
    timeline: &'p Timeline,
    layers: &[SceneLayer],
    t: RationalTime,
    canvas: [f32; 2],
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
    drivers: &mut DriverFrame<'p>,
) -> Result<Vec<SceneLayer>, aria_models::ModelError> {
    let mut wanted = Vec::new();
    for layer in layers {
        referenced(layer, &mut wanted);
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut walk = Walk {
        project,
        timeline,
        t,
        canvas,
        overrides,
        nesting,
        drivers,
        visiting: Vec::new(),
        out: Vec::new(),
    };
    for id in wanted {
        walk.visit(id)?;
    }
    Ok(walk.out)
}

/// Append the clips `layer` reads by layer reference: the layer parameters of
/// its registered effects that a pass binds, in this layer and every layer it
/// wraps.
fn referenced(layer: &SceneLayer, out: &mut Vec<ClipId>) {
    for pass in &layer.effects {
        let aria_compositor::PassId::Registered(registered) = &pass.id else {
            continue;
        };
        let Some(instance) = &pass.instance else {
            continue;
        };
        for (param, &clip) in &instance.layer_params {
            let clip = ClipId::from_raw(clip);
            if registered.reads_layer(param) && !out.contains(&clip) {
                out.push(clip);
            }
        }
    }
    match &layer.source {
        LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            referenced(outgoing, out);
            referenced(incoming, out);
        }
        LayerSource::Matte { layers, .. } => {
            for inner in layers.iter() {
                referenced(inner, out);
            }
        }
        // A nest's layers read clips of the nest's own timeline, which its
        // own scene resolves.
        _ => {}
    }
}

/// A post-order walk over the reference graph.
struct Walk<'a, 'p, 'o> {
    project: &'p Project,
    timeline: &'p Timeline,
    t: RationalTime,
    canvas: [f32; 2],
    overrides: ResolveOverrides<'o>,
    nesting: &'a Nesting<'a>,
    drivers: &'a mut DriverFrame<'p>,
    /// The clips whose references are being resolved, outermost first.
    visiting: Vec<ClipId>,
    out: Vec<SceneLayer>,
}

impl Walk<'_, '_, '_> {
    fn visit(&mut self, id: ClipId) -> Result<(), aria_models::ModelError> {
        if self.visiting.contains(&id)
            || self.visiting.len() >= MAX_REFERENCE_DEPTH
            || self.out.iter().any(|layer| layer.clip == Some(id))
        {
            return Ok(());
        }
        let project = self.project;
        let Some(clip) = self.timeline.clip(id) else {
            return Ok(());
        };
        let span = &clip.timeline;
        if self.t.value < span.start.value || self.t.value >= span.end_tick() {
            return Ok(());
        }
        let clip = self.drivers.apply(clip, self.t.value as f64);
        let [cw, ch] = self.canvas;
        let Some(layer) =
            resolve_clip(project, &clip, self.t, cw, ch, self.overrides, self.nesting)?
        else {
            return Ok(());
        };
        if layer.source.is_canvas_pass() {
            return Ok(());
        }
        let mut reads = Vec::new();
        referenced(&layer, &mut reads);
        self.visiting.push(id);
        for read in reads {
            self.visit(read)?;
        }
        self.visiting.pop();
        self.out.push(layer);
        Ok(())
    }
}
