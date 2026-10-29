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
///
/// The two sides of a transition or matte are held in side textures outside
/// the slot ring, so drawing one side through the ring leaves the other intact.
pub(crate) struct OffscreenPool {
    pub(crate) width: u32,
    pub(crate) height: u32,
    textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
    /// Side textures, allocated the first time a scene draws a pair.
    sides: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    /// Effect-pass bind groups keyed by the views above. Built with the pool
    /// and dropped with it, so an entry can never name a freed view.
    passes: PassCache,
}

impl OffscreenPool {
    pub(crate) const DEFAULT_SLOTS: usize = 2;

    pub(crate) fn ensure(
        device: &wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Self {
        let mut pool = Self {
            width,
            height,
            textures: Vec::with_capacity(Self::DEFAULT_SLOTS),
            views: Vec::with_capacity(Self::DEFAULT_SLOTS),
            sides: None,
            passes: PassCache::new(queue),
        };
        pool.ensure_slots(device, Self::DEFAULT_SLOTS);
        pool
    }

    /// Ensure the pool holds at least `required` canvas-sized offscreen slots.
    pub(crate) fn ensure_slots(&mut self, device: &wgpu::Device, required: usize) {
        while self.textures.len() < required {
            let label = format!("aria.offscreen.{}", self.textures.len());
            let (texture, view) = self.create_texture(device, &label);
            self.textures.push(texture);
            self.views.push(view);
        }
    }

    /// Ensure the pool holds the two side textures a transition or matte
    /// draws its sides into.
    pub(crate) fn ensure_sides(&mut self, device: &wgpu::Device) {
        if self.sides.is_none() {
            self.sides = Some([
                self.create_texture(device, "aria.offscreen.side.0"),
                self.create_texture(device, "aria.offscreen.side.1"),
            ]);
        }
    }

    fn create_texture(
        &self,
        device: &wgpu::Device,
        label: &str,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
        (texture, view)
    }

    /// Current number of offscreen slots in the pool.
    #[inline]
    pub(crate) fn slots(&self) -> usize {
        self.views.len()
    }

    pub(crate) fn view(&self, index: usize) -> &wgpu::TextureView {
        &self.views[index]
    }

    pub(crate) fn texture(&self, index: usize) -> &wgpu::Texture {
        &self.textures[index]
    }

    /// Side texture `index` (0 or 1) and its view.
    ///
    /// # Panics
    ///
    /// When [`ensure_sides`](Self::ensure_sides) has not run on this pool.
    pub(crate) fn side(&self, index: usize) -> (&wgpu::Texture, &wgpu::TextureView) {
        let (texture, view) = &self.sides.as_ref().expect("side textures ensured")[index];
        (texture, view)
    }

    /// Pool slot holding `view`, if it is one of this pool's views.
    pub(crate) fn index_of(&self, view: &wgpu::TextureView) -> Option<usize> {
        self.views.iter().position(|v| std::ptr::eq(v, view))
    }

    /// The reuse table for passes reading this pool's slots.
    pub(crate) fn passes(&self) -> &PassCache {
        &self.passes
    }
}
