//! Canvas-sized targets kept by id across frames.
//!
//! A [`CompositorLayer::Reference`](crate::CompositorLayer::Reference) draws a
//! clip into the target of its id, which a registered effect reads through a
//! layer parameter. A [`CompositorLayer::Nest`](crate::CompositorLayer::Nest)
//! draws the items of a nested sequence into the target of its id, which a
//! layer whose content is [`LayerContent::Nest`](crate::LayerContent::Nest)
//! samples. Every target is cleared at the start of a frame, so an id no item
//! of the frame draws reads transparent.

use rustc_hash::FxHashMap;

use super::RT_FORMAT;

/// One target per id this frame names, at the canvas size.
pub(crate) struct CanvasTargets {
    label: &'static str,
    width: u32,
    height: u32,
    targets: FxHashMap<u64, (wgpu::Texture, wgpu::TextureView)>,
    /// Ids this frame names, reused across frames.
    wanted: Vec<u64>,
}

impl CanvasTargets {
    /// No targets yet; each one this set creates carries `label`.
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            label,
            width: 0,
            height: 0,
            targets: FxHashMap::default(),
            wanted: Vec::new(),
        }
    }

    /// Keep a target for every id in `ids` at `width`×`height`, dropping the
    /// targets of ids this frame does not name.
    pub(crate) fn ensure(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        ids: impl IntoIterator<Item = u64>,
    ) {
        if self.width != width || self.height != height {
            self.targets.clear();
            self.width = width;
            self.height = height;
        }
        self.wanted.clear();
        self.wanted.extend(ids);
        let wanted = &self.wanted;
        self.targets.retain(|id, _| wanted.contains(id));
        let label = self.label;
        for &id in &self.wanted {
            self.targets.entry(id).or_insert_with(|| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: RT_FORMAT,
                    // A layer drawn under a blend mode other than Normal reads
                    // what is under it through a copy of its destination.
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (texture, view)
            });
        }
    }

    /// Clear every target to transparent.
    pub(crate) fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        for (_, view) in self.targets.values() {
            clear_target(encoder, self.label, view);
        }
    }

    /// Clear the target of `id` to transparent, when this frame names it.
    pub(crate) fn clear_one(&self, encoder: &mut wgpu::CommandEncoder, id: u64) {
        if let Some((_, view)) = self.targets.get(&id) {
            clear_target(encoder, self.label, view);
        }
    }

    /// The target view of `id`, or `None` when this frame does not name it.
    pub(crate) fn view(&self, id: u64) -> Option<&wgpu::TextureView> {
        self.targets.get(&id).map(|(_, view)| view)
    }

    /// The target texture and view of `id`, or `None` when this frame does
    /// not name it.
    pub(crate) fn target(&self, id: u64) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
        self.targets.get(&id).map(|(texture, view)| (texture, view))
    }
}

fn clear_target(
    encoder: &mut wgpu::CommandEncoder,
    _label: &'static str,
    view: &wgpu::TextureView,
) {
    let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}
