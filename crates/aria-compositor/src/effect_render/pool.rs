use super::RT_FORMAT;
use super::pass_cache::PassCache;

/// Reused canvas-sized RGBA textures for effect ping-pong, transitions,
/// style silhouette/blur, and blend-mode canvas snapshots.
///
/// Slots are scaled dynamically to satisfy the concurrent texture demand
/// of the active scene (base draw, multi-pass effect ping-pong, styles,
/// transitions, and dst-sampling blend snapshots).
///
/// Slot scheduling guarantees:
/// - Layer base draws into slot 0.
/// - Multi-pass separable effects (e.g. Gaussian blur) draw into non-colliding
///   intermediate and output slots.
/// - Style passes (shadow/glow/outline) execute without overwriting content on S.
/// - Blend mode snapshots copy canvas state into an independent slot before blending.
pub(crate) struct OffscreenPool {
    pub(crate) width: u32,
    pub(crate) height: u32,
    textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
    /// Effect-pass bind groups keyed by the views above. Built with the pool
    /// and dropped with it, so an entry can never name a freed view.
    passes: PassCache,
}

impl OffscreenPool {
    pub const DEFAULT_SLOTS: usize = 2;

    pub fn ensure(device: &wgpu::Device, queue: wgpu::Queue, width: u32, height: u32) -> Self {
        let mut pool = Self {
            width,
            height,
            textures: Vec::with_capacity(Self::DEFAULT_SLOTS),
            views: Vec::with_capacity(Self::DEFAULT_SLOTS),
            passes: PassCache::new(queue),
        };
        pool.ensure_slots(device, Self::DEFAULT_SLOTS);
        pool
    }

    /// Ensure the pool holds at least `required` canvas-sized offscreen slots.
    pub fn ensure_slots(&mut self, device: &wgpu::Device, required: usize) {
        while self.textures.len() < required {
            let idx = self.textures.len();
            let label = format!("aria.offscreen.{idx}");
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&label),
                size: wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: RT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.textures.push(texture);
            self.views.push(view);
        }
    }

    /// Current number of offscreen slots in the pool.
    #[inline]
    pub fn slots(&self) -> usize {
        self.views.len()
    }

    pub fn view(&self, index: usize) -> &wgpu::TextureView {
        &self.views[index]
    }

    pub fn texture(&self, index: usize) -> &wgpu::Texture {
        &self.textures[index]
    }

    /// Pool slot holding `view`, if it is one of this pool's views.
    pub fn index_of(&self, view: &wgpu::TextureView) -> Option<usize> {
        self.views.iter().position(|v| std::ptr::eq(v, view))
    }

    /// The reuse table for passes reading this pool's slots.
    pub fn passes(&self) -> &PassCache {
        &self.passes
    }
}
