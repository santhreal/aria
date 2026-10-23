//! Canvas-sized targets kept by id across frames.
//!
//! A [`CompositorLayer::Reference`](crate::CompositorLayer::Reference) draws a
//! clip into the target of its id, which a registered effect reads through a
//! layer parameter. A [`CompositorLayer::Nest`](crate::CompositorLayer::Nest)
//! draws the items of a nested sequence into the target of its id, which a
//! layer whose content is [`LayerContent::Nest`](crate::LayerContent::Nest)
//! samples. Each target is marked for a transparent clear at the start of a
//! frame and fused into its first draw pass (or cleared on demand if read
//! before any item draws into it).

use std::cell::Cell;

use rustc_hash::FxHashMap;

use super::RT_FORMAT;

struct TargetSlot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    needs_clear: Cell<bool>,
}

/// One target per id this frame names, at the canvas size.
pub(crate) struct CanvasTargets {
    label: &'static str,
    width: u32,
    height: u32,
    targets: FxHashMap<u64, TargetSlot>,
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
    /// targets of ids this frame does not name and marking every retained
    /// target to clear before it is drawn or read.
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
            let slot = self.targets.entry(id).or_insert_with(|| {
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
                TargetSlot {
                    texture,
                    view,
                    needs_clear: Cell::new(true),
                }
            });
            slot.needs_clear.set(true);
        }
    }

    /// Mark the target of `id` to clear to transparent on its next draw or read.
    pub(crate) fn mark_clear(&self, id: u64) {
        if let Some(slot) = self.targets.get(&id) {
            slot.needs_clear.set(true);
        }
    }

    /// Consume the pending transparent clear for `id` so the caller can fuse it
    /// into its render pass.
    pub(crate) fn take_clear(&self, id: u64) -> Option<wgpu::Color> {
        self.targets.get(&id).and_then(|slot| {
            slot.needs_clear
                .replace(false)
                .then_some(wgpu::Color::TRANSPARENT)
        })
    }

    /// Ensure the target of `id` has been cleared if no draw has consumed its
    /// pending clear yet.
    pub(crate) fn ensure_cleared(&self, encoder: &mut wgpu::CommandEncoder, id: u64) {
        if let Some(slot) = self.targets.get(&id)
            && slot.needs_clear.replace(false)
        {
            crate::color_pass::clear(encoder, &slot.view, wgpu::Color::TRANSPARENT);
        }
    }

    /// The target view of `id`, or `None` when this frame does not name it.
    pub(crate) fn view(&self, id: u64) -> Option<&wgpu::TextureView> {
        self.targets.get(&id).map(|slot| &slot.view)
    }

    /// The target texture and view of `id`, or `None` when this frame does
    /// not name it.
    pub(crate) fn target(&self, id: u64) -> Option<(&wgpu::Texture, &wgpu::TextureView)> {
        self.targets
            .get(&id)
            .map(|slot| (&slot.texture, &slot.view))
    }
}
