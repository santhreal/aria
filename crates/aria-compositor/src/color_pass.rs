//! Render passes over one color target.
//!
//! Every full-screen pass and every clear in this crate begins the same
//! descriptor: one color attachment, stored, no depth, no queries. Only the
//! target and whether its contents are cleared, kept, or left unloaded differ.

use std::cell::Cell;

thread_local! {
    static PASSES_BEGUN: Cell<u32> = const { Cell::new(0) };
}

/// Cumulative render passes begun on the current thread.
pub(crate) fn passes_begun() -> u32 {
    PASSES_BEGUN.with(Cell::get)
}

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
    #[allow(unsafe_code)]
    pub(crate) unsafe fn new(pipeline: wgpu::RenderPipeline) -> Self {
        Self(pipeline)
    }

    /// The pipeline itself, to draw over a loaded target.
    pub(crate) fn raw(&self) -> &wgpu::RenderPipeline {
        &self.0
    }
}

/// Full-screen quad pipeline: either blending over its target or replacing
/// every pixel without blending.
#[derive(Clone, Copy)]
pub(crate) enum QuadPipeline<'a> {
    Blended(&'a wgpu::RenderPipeline),
    Replace(&'a ReplacePipeline),
}

impl<'a> QuadPipeline<'a> {
    pub(crate) fn raw(self) -> &'a wgpu::RenderPipeline {
        match self {
            Self::Blended(pipeline) => pipeline,
            Self::Replace(pipeline) => pipeline.raw(),
        }
    }

    pub(crate) fn overwrites_target(self) -> bool {
        matches!(self, Self::Replace(_))
    }
}

/// Begin a pass that writes `view`, loading it with `load`.
pub(crate) fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    PASSES_BEGUN.with(|c| c.set(c.get().wrapping_add(1)));
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
#[allow(unsafe_code)]
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
    let mut pass = begin(encoder, view, load);
    draw_quad_in(&mut pass, pipeline, bind_group);
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
    #[allow(unsafe_code)]
    let mut pass = unsafe { begin_overwrite(encoder, view) };
    draw_quad_in(&mut pass, &pipeline.0, bind_group);
}

/// Record a full-screen quad draw of `pipeline` with `bind_group` inside an
/// open render `pass`.
pub(crate) fn draw_quad_in(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..6, 0..1);
}
