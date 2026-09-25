//! Pass one of a frame: decode or rasterize every scene layer into owned pixels
//! and a final placement.
//!
//! Moved out of `mod.rs` to stay under the file ceiling; nothing changed. It is
//! its own method rather than an inline block because its three outputs are
//! owned — the borrowed `CompositeLayer`s that pass two builds all point into
//! the `Realized` values returned here, so this pass has to finish before any
//! of those borrows exist.

use super::*;

impl Renderer {
    /// Realize every layer of `scene`, returning the owned layers, the blur
    /// placements parallel to them, and the milliseconds spent in media decode.
    ///
    /// `cancel` is checked before each layer. A layer is the unit because it is
    /// the unit of decode: abandoning a frame between two layers gives back the
    /// engine lock without paying for the seek-and-walk of the layers left,
    /// which is what a scrub needs when the playhead has already moved on.
    pub(super) fn realize_scene_layers(
        &mut self,
        project: &Project,
        scene: &Scene,
        policy: SeekPolicy,
        cancel: &Cancel,
        realized: &mut Vec<Realized>,
        scene_indices: &mut Vec<usize>,
        blur_for_realized: &mut Vec<Vec<LayerPlacement>>,
        occurrence: &mut HashMap<MediaId, u32>,
    ) -> Result<f64, RenderError> {
        self.preload_scene_luts(scene);
        // Decode time accumulated across media layers — on weak machines this
        // is where whole-frame seconds hide, so the stage log splits it out.
        let mut decode_ms = 0.0f64;
        realized.reserve(scene.layers.len());
        scene_indices.reserve(scene.layers.len());
        blur_for_realized.reserve(scene.layers.len());
        for (scene_idx, layer) in scene.layers.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            let mut blur = match layer.size {
                SizeSpec::Fixed(_) => {
                    layer_placements_from_blur_passes(&layer.blur_passes, layer.anchor_point)
                }
                SizeSpec::BitmapScaled(_) => Vec::new(),
            };
            let fx = layer_effects(layer);
            let color_grade = layer.color_grade;
            let mode = blend_mode(layer.blend_mode);
            let styles = layer_styles(layer.styles.as_ref());
            // Load (or recall) the layer's .cube table; unreadable files
            // resolve to None and grade nothing.
            let scene_lut = self.resolve_scene_lut(&layer.lut);
            // The layer carries the anchor position; the quad center falls out
            // of the final pixel size (bitmap sizes only exist after raster).
            let place = |size: [f32; 2]| LayerPlacement {
                center: layer.quad_center(size),
                size,
                rotation: layer.rotation,
                opacity: layer.opacity,
            };
            match &layer.source {
                LayerSource::CanvasPass => {
                    realized.push(Realized::CanvasPass {
                        grade: color_grade,
                        lut: scene_lut,
                    });
                }
                LayerSource::Transition {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                } => {
                    let out = self.realize_subscene_layer(project, scene, outgoing, policy)?;
                    let inc = self.realize_subscene_layer(project, scene, incoming, policy)?;
                    realized.push(Realized::Transition {
                        outgoing: out,
                        incoming: inc,
                        transition_id: transition_id.clone(),
                        progress: *progress,
                    });
                }
                LayerSource::Matte {
                    layers,
                    read,
                    invert,
                    softness,
                } => {
                    let content =
                        self.realize_subscene_layer(project, scene, &layers[0], policy)?;
                    let stencil =
                        self.realize_subscene_layer(project, scene, &layers[1], policy)?;
                    realized.push(Realized::Matte {
                        content,
                        stencil,
                        read: *read,
                        invert: *invert,
                        softness: *softness,
                    });
                }
                LayerSource::Solid(rgba) => {
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    realized.push(Realized::Solid {
                        rgba: *rgba,
                        placement: place(size),
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::Text {
                    content,
                    style,
                    animation,
                } => {
                    let Some(layer_realized) = text::realize_text_layer(
                        &mut self.text,
                        layer,
                        content,
                        style,
                        animation,
                        [scene.width as f32, scene.height as f32],
                        fx,
                        color_grade,
                        scene_lut,
                        mode,
                        styles,
                    ) else {
                        continue;
                    };
                    if matches!(layer.size, SizeSpec::BitmapScaled(_))
                        && !layer.blur_passes.is_empty()
                        && let Realized::Bitmap { image, .. } = &layer_realized
                    {
                        blur = layer_placements_from_bitmap_scaled_blur_passes(
                            &layer.blur_passes,
                            layer.anchor_point,
                            [image.width as f32, image.height as f32],
                        );
                    }
                    realized.push(layer_realized);
                }
                LayerSource::Media { media, source_time } => {
                    let slot = occurrence.entry(*media).or_insert(0);
                    let decode_started = Instant::now();
                    let frame = self.decode(project, *media, *slot, *source_time, policy)?;
                    decode_ms += decode_started.elapsed().as_secs_f64() * 1000.0;
                    *slot += 1;
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    realized.push(Realized::Frame {
                        frame,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::Still { media } => {
                    self.ensure_still(project, *media)?;
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    realized.push(Realized::Still {
                        media: *media,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::Lottie { path, local_time } => {
                    // A missing or unsupported file draws nothing (the media
                    // offline story — projects move machines), never an error.
                    let Some(frame_index) = self.ensure_lottie_frame(path, *local_time) else {
                        continue;
                    };
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    realized.push(Realized::Lottie {
                        path: path.clone(),
                        frame_index,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::Sticker { asset, local_time } => {
                    // The resolver only emits catalog ids, but stay graceful:
                    // an unknown id draws nothing rather than failing a frame.
                    let Some(spec) = aria_models::sticker_spec(asset) else {
                        continue;
                    };
                    self.ensure_sticker(spec)?;
                    let frame_index = self.stickers[spec.id.as_str()].frame_at(*local_time);
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    realized.push(Realized::Sticker {
                        asset: asset.clone(),
                        frame_index,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::Shape {
                    params,
                    fill,
                    stroke,
                    pad,
                } => {
                    // The resolver sized the quad as shape + pad per side;
                    // recover the shape's own half-extents for the shader.
                    let size = fixed_size(layer.size, [scene.width as f32, scene.height as f32]);
                    let half = [
                        (size[0] * 0.5 - pad).max(0.0),
                        (size[1] * 0.5 - pad).max(0.0),
                    ];
                    realized.push(Realized::Sdf {
                        shape: SdfLayer {
                            shape: params.with_half(half),
                            fill: *fill,
                            stroke: *stroke,
                        },
                        placement: place(size),
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
                LayerSource::PathShape {
                    path,
                    fill,
                    stroke,
                    raster_scale,
                } => {
                    let style = ShapeStyle {
                        fill: Some(*fill).filter(|c| c[3] > 0),
                        stroke: *stroke,
                    };
                    let image = self.paths.rasterize(path, &style, *raster_scale);
                    if image.width == 0 || image.height == 0 {
                        continue; // nothing inked (degenerate path or style)
                    }
                    let scale = match layer.size {
                        SizeSpec::BitmapScaled(s) => s,
                        SizeSpec::Fixed(_) => [1.0, 1.0],
                    };
                    let size = [
                        image.width as f32 * scale[0],
                        image.height as f32 * scale[1],
                    ];
                    if matches!(layer.size, SizeSpec::BitmapScaled(_))
                        && !layer.blur_passes.is_empty()
                    {
                        blur = layer_placements_from_bitmap_scaled_blur_passes(
                            &layer.blur_passes,
                            layer.anchor_point,
                            [image.width as f32, image.height as f32],
                        );
                    }
                    realized.push(Realized::Bitmap {
                        image,
                        placement: place(size),
                        uv: layer.uv,
                        fx,
                        color_grade,
                        lut: scene_lut,
                        blend_mode: mode,
                        styles,
                    });
                }
            }
            scene_indices.push(scene_idx);
            blur_for_realized.push(blur);
        }
        Ok(decode_ms)
    }
}
