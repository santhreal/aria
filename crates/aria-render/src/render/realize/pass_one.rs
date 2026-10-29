//! Pass one of a frame: decode or rasterize every scene layer into owned pixels
//! and a final placement.
//!
//! It is its own method rather than an inline block because its outputs are
//! owned: the borrowed `CompositeLayer`s that pass two builds all point into
//! the `Realized` values returned here, so this pass has to finish before any
//! of those borrows exist.
//!
//! The frame's scene is level 0. Each nest a level shows adds its scene to
//! [`Nests`], and nest `k` is realized as level `k + 1` once the levels before
//! it are done, so the nests a nest shows come after it.

use super::*;

impl Renderer {
    /// Realize every layer of `scene` and of every nest it shows into
    /// `scratch`: the realized layers, the draw index of each in its level's
    /// scene, and the range of each level. Returns what the media decodes
    /// took and drew.
    ///
    /// `cancel` is checked before each layer. A layer is the unit because it is
    /// the unit of decode: abandoning a frame between two layers gives back the
    /// engine lock without paying for the seek-and-walk of the layers left,
    /// which is what a scrub needs when the playhead has already moved on.
    pub(super) fn realize_scene_layers<'s>(
        &mut self,
        project: &Project,
        scene: &'s Scene,
        policy: SeekPolicy,
        cancel: &Cancel,
        nests: &mut Nests<'s>,
        scratch: &mut RealizeScratch,
    ) -> Result<DecodeTally, RenderError> {
        self.preload_scene_luts(scene);
        // Decode time accumulated across media layers: on weak machines this
        // is where whole-frame seconds hide, so the stage log splits it out.
        let mut decoded = DecodeTally::default();
        let mut level = Some(scene);
        let mut next_nest = 0;
        while let Some(level_scene) = level {
            let start = scratch.realized.len();
            self.realize_level(
                project,
                level_scene,
                policy,
                cancel,
                nests,
                scratch,
                &mut decoded,
            )?;
            scratch.levels.push(start..scratch.realized.len());
            level = nests.scenes.get(next_nest).copied();
            next_nest += 1;
        }
        Ok(decoded)
    }

    /// Realize the layers of one level's `scene` onto the end of `scratch`.
    #[allow(clippy::too_many_arguments)]
    fn realize_level<'s>(
        &mut self,
        project: &Project,
        scene: &'s Scene,
        policy: SeekPolicy,
        cancel: &Cancel,
        nests: &mut Nests<'s>,
        scratch: &mut RealizeScratch,
        decoded: &mut DecodeTally,
    ) -> Result<(), RenderError> {
        let drawn = scene.references.len() + scene.layers.len();
        scratch.realized.reserve(drawn);
        scratch.scene_indices.reserve(drawn);
        let canvas = LayerPlacement {
            center: [scene.width as f32 * 0.5, scene.height as f32 * 0.5],
            size: [scene.width as f32, scene.height as f32],
            rotation: 0.0,
            opacity: 1.0,
        };
        for (scene_idx, layer) in scene.draw_order().enumerate() {
            if cancel.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            let item = match &layer.source {
                LayerSource::CanvasPass => {
                    // A canvas pass has no quad: its mask covers the canvas.
                    let mut fx = layer_effects(layer);
                    Realized::CanvasPass {
                        grade: layer.color_grade,
                        lut: self.resolve_scene_lut(&layer.lut),
                        stencil: self.layer_stencil(layer, &mut fx, false, canvas),
                    }
                }
                LayerSource::Transition {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                } => Realized::Transition {
                    outgoing: self
                        .realize_side(project, scene, outgoing, policy, nests, decoded)?,
                    incoming: self
                        .realize_side(project, scene, incoming, policy, nests, decoded)?,
                    transition_id: transition_id.clone(),
                    progress: *progress,
                },
                LayerSource::Matte {
                    layers,
                    read,
                    invert,
                    softness,
                } => Realized::Matte {
                    content: self
                        .realize_side(project, scene, &layers[0], policy, nests, decoded)?,
                    stencil: self
                        .realize_side(project, scene, &layers[1], policy, nests, decoded)?,
                    read: *read,
                    invert: *invert,
                    softness: *softness,
                },
                _ => {
                    match self.realize_leaf(project, scene, layer, policy, false, nests, decoded)? {
                        Some(leaf) => Realized::Layer(leaf),
                        None => continue,
                    }
                }
            };
            scratch.realized.push(item);
            scratch.scene_indices.push(scene_idx);
        }
        Ok(())
    }
}
