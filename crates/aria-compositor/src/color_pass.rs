//! Render passes over one color target.
//!
//! Every full-screen pass and every clear in this crate begins the same
//! descriptor: one color attachment, stored, no depth, no queries. Only the
//! target and whether its contents are cleared or kept differ.

/// Clear the target to transparent before drawing.
pub(crate) const CLEAR: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

/// Draw over the target's contents.
pub(crate) const KEEP: wgpu::LoadOp<wgpu::Color> = wgpu::LoadOp::Load;

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
    let mut pass = begin(encoder, view, load);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..6, 0..1);
}
