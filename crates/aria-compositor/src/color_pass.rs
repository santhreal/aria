//! Render passes over one color target.
//!
//! Every full-screen pass and every clear in this crate begins the same
//! descriptor: one color attachment, stored, no depth, no queries. Only the
//! target and whether its contents are cleared, kept, or left unloaded differ.

/// Clear the target to transparent before drawing.
pub(crate) const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

/// Draw over the target's contents.
pub(crate) const KEEP: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Load;

/// A full-screen pipeline that writes every pixel of its target without
/// blending, so a pass that draws it skips the load or clear before it.
pub(crate) struct ReplacePipeline(wgpu::RenderPipeline);

impl ReplacePipeline {
    /// Mark `pipeline` as one that replaces its whole target.
    ///
    /// # Safety
    ///
    /// `pipeline` has blending disabled and writes every color channel, its
    /// vertex shader covers the target with vertices `0..6`, and its fragment
    /// shader never discards.
    pub(crate) unsafe fn new(pipeline: wgpu::RenderPipeline) -> Self {
        Self(pipeline)
    }

    /// The pipeline itself, to draw over a loaded target.
    #[cfg(test)]
    pub(crate) fn raw(&self) -> &wgpu::RenderPipeline {
        &self.0
    }
}

/// Begin a pass that writes `view`, loading it with `load`.
pub(crate) fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Begin a pass that writes `view` without loading its contents.
///
/// A tiled GPU skips the load from memory and a software rasterizer skips
/// the clear fill, so a pass that replaces every pixel costs only its draw.
///
/// # Safety
///
/// The first draw of the pass writes every pixel of `view` with blending
/// disabled. The target's contents are undefined until that draw runs.
pub(crate) unsafe fn begin_overwrite<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    // SAFETY: the caller's first draw writes every pixel of `view` without
    // blending, so no undefined texel is read or stored.
    let load = wgpu::LoadOp::DontCare(unsafe { wgpu::LoadOpDontCare::enabled() });
    begin(encoder, view, load)
}

/// Clear `view` to `color`.
pub(crate) fn clear(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    color: wgpu::Color,
) {
    begin(encoder, view, wgpu::LoadOp::Clear(color));
}

/// Draw the full-screen quad of `pipeline` with `bind_group` into `view`,
/// loading it with `load`.
pub(crate) fn draw_quad(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    quad(begin(encoder, view, load), pipeline, bind_group);
}

/// Draw the full-screen quad of `pipeline` with `bind_group` into `view`,
/// replacing every pixel without loading or clearing `view` first.
pub(crate) fn replace_quad(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    pipeline: &ReplacePipeline,
    bind_group: &wgpu::BindGroup,
) {
    // SAFETY: the pass's one draw is a `ReplacePipeline` quad, which writes
    // every pixel of `view` without blending.
    let pass = unsafe { begin_overwrite(encoder, view) };
    quad(pass, &pipeline.0, bind_group);
}

fn quad(
    mut pass: wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..6, 0..1);
}
