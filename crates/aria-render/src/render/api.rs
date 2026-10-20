use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorError, CompositorLayer, FrameSink,
    GpuContext, ImageSink, LayerPlacement, RgbaImage, wgpu,
};
use aria_core::{Cancel, RationalTime};
use aria_decoder::OutputMode;
use aria_models::{ClipId, MediaId, Project};
use aria_shapes::PathRaster;
use aria_text::TextRenderer;

use crate::error::RenderError;
use crate::motion_blur::attach_motion_blur_passes;
use crate::render_files::RenderFiles;
use crate::resolve::{ResolveOverrides, resolve, resolve_gesture_partitions};
use crate::scene::{LayerSource, Scene, SceneLayer, SizeSpec};

use super::media_cache::default_decode_mode;
use super::realize::Output;
use super::{FrameStats, GestureFrames, PendingFrame, Renderer, SeekPolicy};

pub(super) fn media_scene(
    project: &Project,
    media: MediaId,
    source_time: RationalTime,
) -> Result<Scene, RenderError> {
    let source = project
        .media(media)
        .ok_or(RenderError::MissingMedia(media))?;
    let valid_dimension = |dimension| {
        (aria_models::CanvasSettings::MIN_DIM..=aria_models::CanvasSettings::MAX_DIM)
            .contains(&dimension)
    };
    let (width, height) = if valid_dimension(source.width) && valid_dimension(source.height) {
        ((source.width & !1).max(2), (source.height & !1).max(2))
    } else {
        (1920, 1080)
    };
    let fit = if source.width == 0 || source.height == 0 {
        1.0
    } else {
        (width as f32 / source.width as f32).min(height as f32 / source.height as f32)
    };
    let mut scene = Scene::empty(width, height, [0, 0, 0, 255]);
    scene.tick = RationalTime::new(0, source.frame_rate);
    scene.layers.push(SceneLayer {
        clip: None,
        source: if source.is_image {
            LayerSource::Still { media }
        } else {
            LayerSource::Media { media, source_time }
        },
        center: [width as f32 * 0.5, height as f32 * 0.5],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([source.width as f32 * fit, source.height as f32 * fit]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    });
    Ok(scene)
}

impl Renderer {
    /// Bring up a headless GPU and build the renderer.
    ///
    /// Use this where the renderer is the only GPU consumer — export, a CLI, a
    /// test. A process that already has a device shares it with
    /// [`Self::with_gpu`] instead of paying for a second one.
    pub fn new_headless() -> Result<Self, RenderError> {
        Ok(Self::with_gpu(GpuContext::new_headless_blocking()?))
    }

    /// Build the renderer on a device somebody else already created.
    ///
    /// The pipelines are this renderer's own — they are built against the shared
    /// device — but the driver instance, the adapter and the video memory behind
    /// them are not duplicated. `GpuContext::share` is what hands one over.
    pub fn with_gpu(gpu: GpuContext) -> Self {
        let compositor = Compositor::new(&gpu);
        Self {
            gpu,
            compositor,
            text: TextRenderer::new(),
            paths: PathRaster::new(),
            decoders: HashMap::new(),
            stills: HashMap::new(),
            stickers: HashMap::new(),
            lottie: HashMap::new(),
            scene_stamp: 0,
            shared_check_stamp: u64::MAX,
            scene_shares_frames: false,
            last_tick: None,
            look_uses: Vec::new(),
            look_ticks: Vec::new(),
            luts: HashMap::new(),
            decode_mode: default_decode_mode(),
            proxies: HashMap::new(),
            use_proxies: true,
            apply_motion_blur: false,
            realize_scratch: super::realize::RealizeScratch::default(),
            resolve_scene: Scene::empty(0, 0, [0, 0, 0, 255]),
            resolve_pool: crate::resolve::ResolvePool::default(),
            look_ahead_scene: Scene::empty(0, 0, [0, 0, 0, 255]),
            look_ahead_pool: crate::resolve::ResolvePool::default(),
            text_memo: HashMap::new(),
            cancel: Cancel::never(),
            last_stats: FrameStats::default(),
            render_files: None,
            render_file_bytes: Vec::new(),
            render_file_image: RgbaImage::new(0, 0, Vec::new()),
        }
    }

    /// Shared handle to this renderer's [`GpuContext`].
    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// Replace this renderer's [`GpuContext`] and rebuild device-bound GPU
    /// pipelines and texture caches on the new device.
    pub fn set_gpu(&mut self, gpu: GpuContext) {
        self.compositor.reset_gpu(&gpu);
        self.gpu = gpu;
    }

    /// Cumulative number of GPU-to-CPU frame readbacks submitted by this
    /// renderer's compositor.
    pub const fn readback_count(&self) -> u64 {
        self.compositor.readback_count()
    }

    /// Enable or disable transform motion-blur supersampling on subsequent
    /// frames. Export turns this on; interactive preview leaves it off.
    pub fn set_apply_motion_blur(&mut self, on: bool) {
        self.apply_motion_blur = on;
    }

    /// Attach or detach the content-addressed [`RenderFiles`] store consulted by
    /// preview frame entry points.
    pub fn set_render_files(&mut self, files: Option<Arc<RenderFiles>>) {
        self.render_files = files;
    }

    /// The attached [`RenderFiles`] store, if any.
    pub fn render_files(&self) -> Option<&Arc<RenderFiles>> {
        self.render_files.as_ref()
    }

    /// Let the next frame be abandoned when `cancel` fires.
    ///
    /// The token is spent by the frame that reads it: the realize pass takes it
    /// out on the way in, so it governs exactly one frame and cannot cancel an
    /// unrelated one afterwards. Set it immediately before the render call,
    /// under whatever lock serializes this renderer — a scrub does both while
    /// holding the engine lock, so no other caller can consume the token.
    ///
    /// A frame with no token set runs to completion, which is what an export, a
    /// still and a test want.
    pub fn cancel_next_frame(&mut self, cancel: Cancel) {
        self.cancel = cancel;
    }

    /// Stage timings of the most recent successful render — how the last
    /// frame's cost split between decode, raster, and composite. Zeroed
    /// until the first render completes.
    pub fn last_frame_stats(&self) -> FrameStats {
        self.last_stats
    }

    /// Decode `media` from the file at `path` (a preview proxy) instead of
    /// the project's own path, from the next frame on. Drops the media's
    /// open decoders so no cursor keeps reading the original.
    pub fn set_proxy(&mut self, media: MediaId, path: PathBuf) {
        self.proxies.insert(media, path);
        self.drop_decoders_for(media);
    }

    /// Remove `media`'s proxy substitution (e.g. the media was relinked to a
    /// new file), returning decode to the project's path.
    pub fn clear_proxy(&mut self, media: MediaId) {
        if self.proxies.remove(&media).is_some() {
            self.drop_decoders_for(media);
        }
    }

    /// Remove every proxy substitution while preserving original-media
    /// decoders. Use [`reset_media_sources`](Self::reset_media_sources) when
    /// the project/media-id namespace itself changes.
    pub fn clear_proxies(&mut self) {
        let stale: Vec<MediaId> = self.proxies.keys().copied().collect();
        self.proxies.clear();
        for media in stale {
            self.drop_decoders_for(media);
        }
    }

    /// Drop every cache keyed by the current project's media-id namespace.
    ///
    /// Call this before reusing one renderer with a different project or a
    /// relinked media catalog. Media ids are persisted per project, so the
    /// same numeric id can name a different file after a session switch;
    /// retaining its decoder or still bitmap would render the old asset.
    /// Path-keyed and bundled-asset caches remain warm.
    pub fn reset_media_sources(&mut self) {
        self.decoders.clear();
        self.stills.clear();
        self.proxies.clear();
    }

    /// Invalidate one media id after its source path or probed metadata
    /// changes. The next frame reopens/redecodes it from the project.
    pub fn invalidate_media_source(&mut self, media: MediaId) {
        self.proxies.remove(&media);
        self.drop_decoders_for(media);
        self.stills.remove(&media);
    }

    /// The proxy path registered for `media`, if any (regardless of
    /// [`set_use_proxies`](Self::set_use_proxies)).
    pub fn proxy_for(&self, media: MediaId) -> Option<&Path> {
        self.proxies.get(&media).map(PathBuf::as_path)
    }

    /// Turn proxy substitution on/off for subsequent decodes. Off renders
    /// full quality from the originals (the engine's in-place export);
    /// proxied media's open decoders drop on every change of state so a
    /// stale cursor can never serve the wrong file.
    pub fn set_use_proxies(&mut self, on: bool) {
        if self.use_proxies == on {
            return;
        }
        self.use_proxies = on;
        let proxied: Vec<MediaId> = self.proxies.keys().copied().collect();
        for media in proxied {
            self.drop_decoders_for(media);
        }
    }

    /// Drop every open decoder for `media` (see `decoders`: one entry per
    /// clip showing it).
    fn drop_decoders_for(&mut self, media: MediaId) {
        self.decoders.retain(|key, _| key.media != media);
    }

    /// Add a font face (TTF/OTF bytes) for deterministic text rendering. Without
    /// this the renderer uses the host's installed fonts.
    pub fn load_font(&mut self, data: Vec<u8>) {
        self.text.load_font(data);
        self.text_memo.clear();
    }

    /// Resolve `project` at `t` and composite the result into an [`RgbaImage`].
    pub fn render_frame(
        &mut self,
        project: &Project,
        t: RationalTime,
    ) -> Result<RgbaImage, RenderError> {
        self.render_frame_with(project, t, ResolveOverrides::default())
    }

    /// The scene `project` draws at `t`, with motion-blur samples attached when
    /// [`set_apply_motion_blur`](Self::set_apply_motion_blur) is on. Narrow or
    /// size it, then draw it with [`render_scene`](Self::render_scene) or
    /// [`render_scene_to_texture`](Self::render_scene_to_texture).
    pub fn resolve_frame(&self, project: &Project, t: RationalTime) -> Result<Scene, RenderError> {
        let mut scene = resolve(project, t)?;
        self.prepare_motion_blur(project, &mut scene);
        Ok(scene)
    }

    /// [`render_frame`](Self::render_frame) with live-preview
    /// [`ResolveOverrides`] applied — the gesture/inspector preview path.
    pub fn render_frame_with(
        &mut self,
        project: &Project,
        t: RationalTime,
        overrides: ResolveOverrides<'_>,
    ) -> Result<RgbaImage, RenderError> {
        let mut sink = ImageSink::default();
        self.render_frame_into_with(project, t, overrides, SeekPolicy::Exact, &mut sink)?;
        Ok(sink
            .into_image()
            .expect("render_frame_into_with fills the sink on success"))
    }

    /// [`render_frame`](Self::render_frame) scaled to fit within
    /// `max_width`×`max_height` (aspect preserved, never upscaled) — the
    /// interactive-preview path, where compositing and reading back a full
    /// 4K canvas per scrub tick would waste most of its pixels.
    pub fn render_frame_fit(
        &mut self,
        project: &Project,
        t: RationalTime,
        max_width: u32,
        max_height: u32,
    ) -> Result<RgbaImage, RenderError> {
        self.render_frame_fit_with(
            project,
            t,
            max_width,
            max_height,
            ResolveOverrides::default(),
        )
    }

    /// [`render_frame_fit`](Self::render_frame_fit) with live-preview
    /// [`ResolveOverrides`] applied.
    pub fn render_frame_fit_with(
        &mut self,
        project: &Project,
        t: RationalTime,
        max_width: u32,
        max_height: u32,
        overrides: ResolveOverrides<'_>,
    ) -> Result<RgbaImage, RenderError> {
        let mut sink = ImageSink::default();
        self.render_frame_fit_into_with(
            project,
            t,
            max_width,
            max_height,
            overrides,
            SeekPolicy::Exact,
            &mut sink,
        )?;
        Ok(sink
            .into_image()
            .expect("render_frame_fit_into_with fills the sink on success"))
    }

    /// Decode and composite one pool source directly, without constructing a
    /// timeline project. The real project and `media` id feed the renderer's
    /// existing decoder/still caches.
    pub fn render_media_frame_fit(
        &mut self,
        project: &Project,
        media: MediaId,
        source_time: RationalTime,
        max_width: u32,
        max_height: u32,
    ) -> Result<RgbaImage, RenderError> {
        let mut scene = media_scene(project, media, source_time)?;
        scene.fit_within(max_width, max_height);
        self.render_scene(project, &scene)
    }

    /// Partitioned preview frames for a zero-drift transform gesture: layers
    /// below the clip (opaque background), the clip alone at identity
    /// transform (transparent background), and layers above (transparent).
    /// Each pass is fit to `max_width`×`max_height`. Returns `None` when the
    /// clip can't be partitioned (transitions, etc.).
    pub fn render_gesture_frames(
        &mut self,
        project: &Project,
        t: RationalTime,
        clip_id: ClipId,
        max_width: u32,
        max_height: u32,
    ) -> Result<Option<GestureFrames>, RenderError> {
        let Some(partitions) = resolve_gesture_partitions(project, t, clip_id)? else {
            return Ok(None);
        };

        let mut below = partitions.below;
        below.fit_within(max_width, max_height);
        let below = self.render_scene(project, &below)?;

        let mut sprite_scene = partitions.sprite;
        sprite_scene.fit_within(max_width, max_height);
        let mut sprite = self.render_scene(project, &sprite_scene)?;
        straighten_alpha(&mut sprite);

        let above = if partitions.above.layers.is_empty() {
            None
        } else {
            let mut above_scene = partitions.above;
            above_scene.fit_within(max_width, max_height);
            let mut image = self.render_scene(project, &above_scene)?;
            straighten_alpha(&mut image);
            Some(image)
        };

        Ok(Some(GestureFrames {
            below,
            sprite,
            above,
        }))
    }

    /// [`render_frame_with`](Self::render_frame_with) writing the composited
    /// rows directly into `sink`-provided storage (see
    /// [`render_scene_into`](Self::render_scene_into)), decoding under
    /// `policy` — the interactive-preview entry point, where a scrub drag
    /// passes [`SeekPolicy::NearestSync`].
    pub fn render_frame_into_with(
        &mut self,
        project: &Project,
        t: RationalTime,
        overrides: ResolveOverrides<'_>,
        policy: SeekPolicy,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RenderError> {
        let mut scene = self.take_resolved_scene(project, t, overrides)?;
        if overrides.is_empty()
            && let Some(decode_ms) = self.try_load_render_file(project, &scene)
        {
            let (w, h) = (scene.width, scene.height);
            self.return_resolved_scene(scene);
            return self.composite_render_file_into(w, h, decode_ms, sink);
        }
        self.prepare_motion_blur(project, &mut scene);
        let tick = scene.tick;
        let res = self.render_scene_into_policy(project, &scene, sink, policy);
        self.return_resolved_scene(scene);
        res?;
        self.look_ahead(project, tick, policy);
        Ok(())
    }

    /// [`render_frame_fit_with`](Self::render_frame_fit_with) writing the
    /// composited rows directly into `sink`-provided storage — the
    /// interactive-preview path, which hands the pixels straight to the UI's
    /// frame buffer instead of round-tripping through an [`RgbaImage`].
    /// Decodes under `policy` (see [`render_frame_into_with`](Self::render_frame_into_with)).
    #[allow(clippy::too_many_arguments)] // the preview call: bound + overrides + policy are all load-bearing
    pub fn render_frame_fit_into_with(
        &mut self,
        project: &Project,
        t: RationalTime,
        max_width: u32,
        max_height: u32,
        overrides: ResolveOverrides<'_>,
        policy: SeekPolicy,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RenderError> {
        let mut scene = self.take_resolved_scene(project, t, overrides)?;
        if overrides.is_empty()
            && let Some(decode_ms) = self.try_load_render_file(project, &scene)
        {
            scene.fit_within(max_width, max_height);
            let (w, h) = (scene.width, scene.height);
            self.return_resolved_scene(scene);
            return self.composite_render_file_into(w, h, decode_ms, sink);
        }
        self.prepare_motion_blur(project, &mut scene);
        scene.fit_within(max_width, max_height);
        let tick = scene.tick;
        let res = self.render_scene_into_policy(project, &scene, sink, policy);
        self.return_resolved_scene(scene);
        res?;
        self.look_ahead(project, tick, policy);
        Ok(())
    }

    /// [`render_frame_with`](Self::render_frame_with) composited into a
    /// caller-supplied GPU texture (`target(width, height)`) with no GPU-to-CPU
    /// readback.
    pub fn render_frame_to_texture_with<F>(
        &mut self,
        project: &Project,
        t: RationalTime,
        overrides: ResolveOverrides<'_>,
        policy: SeekPolicy,
        target: F,
    ) -> Result<wgpu::Texture, RenderError>
    where
        F: FnOnce(u32, u32) -> wgpu::Texture,
    {
        let mut scene = self.take_resolved_scene(project, t, overrides)?;
        if overrides.is_empty()
            && let Some(decode_ms) = self.try_load_render_file(project, &scene)
        {
            let (w, h) = (scene.width, scene.height);
            self.return_resolved_scene(scene);
            let texture = target(w, h);
            self.composite_render_file_to_texture(w, h, decode_ms, &texture)?;
            return Ok(texture);
        }
        self.prepare_motion_blur(project, &mut scene);
        let tick = scene.tick;
        let texture = target(scene.width, scene.height);
        let pending = self.submit_scene_policy(project, &scene, policy, Output::Texture(&texture));
        self.return_resolved_scene(scene);
        let pending = pending?;
        self.finish_pending_on_gpu(pending);
        self.look_ahead(project, tick, policy);
        Ok(texture)
    }

    /// [`render_frame_fit_with`](Self::render_frame_fit_with) composited into a
    /// caller-supplied GPU texture (`target(width, height)`) with no GPU-to-CPU
    /// readback. `target` receives the fitted `(width, height)` after
    /// [`Scene::fit_within`].
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame_fit_to_texture_with<F>(
        &mut self,
        project: &Project,
        t: RationalTime,
        max_width: u32,
        max_height: u32,
        overrides: ResolveOverrides<'_>,
        policy: SeekPolicy,
        target: F,
    ) -> Result<wgpu::Texture, RenderError>
    where
        F: FnOnce(u32, u32) -> wgpu::Texture,
    {
        let mut scene = self.take_resolved_scene(project, t, overrides)?;
        if overrides.is_empty()
            && let Some(decode_ms) = self.try_load_render_file(project, &scene)
        {
            scene.fit_within(max_width, max_height);
            let (w, h) = (scene.width, scene.height);
            self.return_resolved_scene(scene);
            let texture = target(w, h);
            self.composite_render_file_to_texture(w, h, decode_ms, &texture)?;
            return Ok(texture);
        }
        self.prepare_motion_blur(project, &mut scene);
        scene.fit_within(max_width, max_height);
        let tick = scene.tick;
        let texture = target(scene.width, scene.height);
        let pending = self.submit_scene_policy(project, &scene, policy, Output::Texture(&texture));
        self.return_resolved_scene(scene);
        let pending = pending?;
        self.finish_pending_on_gpu(pending);
        self.look_ahead(project, tick, policy);
        Ok(texture)
    }

    /// [`render_frame`](Self::render_frame) at an exact output size: content
    /// uniformly scaled (up or down) and centered, aspect mismatches
    /// letterboxed over the canvas background — the export path for a
    /// requested resolution.
    pub fn render_frame_sized(
        &mut self,
        project: &Project,
        t: RationalTime,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, RenderError> {
        let mut scene = self.resolve_frame(project, t)?;
        scene.fit_into(width, height);
        let image = self.render_scene(project, &scene)?;
        self.look_ahead(project, scene.tick, SeekPolicy::Exact);
        Ok(image)
    }

    pub(crate) fn submit_export_frame(
        &mut self,
        project: &Project,
        t: RationalTime,
        output_size: Option<(u32, u32)>,
    ) -> Result<PendingFrame, RenderError> {
        let mut scene = self.resolve_frame(project, t)?;
        if let Some((width, height)) = output_size {
            scene.fit_into(width, height);
        }
        let pending =
            self.submit_scene_policy(project, &scene, SeekPolicy::Exact, Output::Readback)?;
        // Export overlaps the next frames' decode with this frame's GPU work.
        self.look_ahead(project, scene.tick, SeekPolicy::Exact);
        Ok(pending)
    }

    /// When [`Self::apply_motion_blur`] is on, attach sub-frame transform
    /// samples before fit/scale so letterbox adjusts them with the scene.
    fn prepare_motion_blur(&self, project: &Project, scene: &mut Scene) {
        if self.apply_motion_blur {
            attach_motion_blur_passes(project, scene);
        }
    }

    fn take_resolved_scene(
        &mut self,
        project: &Project,
        t: RationalTime,
        overrides: ResolveOverrides<'_>,
    ) -> Result<Scene, RenderError> {
        let mut scene =
            std::mem::replace(&mut self.resolve_scene, Scene::empty(0, 0, [0, 0, 0, 255]));
        if let Err(err) = crate::resolve::resolve_with_into(
            project,
            t,
            overrides,
            &mut scene,
            &mut self.resolve_pool,
        ) {
            self.resolve_scene = scene;
            return Err(err.into());
        }
        Ok(scene)
    }

    fn return_resolved_scene(&mut self, mut scene: Scene) {
        self.resolve_pool.recycle_scene(&mut scene);
        self.resolve_scene = scene;
    }

    fn try_load_render_file(&mut self, project: &Project, unscaled_scene: &Scene) -> Option<f64> {
        let files = self.render_files.as_ref()?;
        let key = files.key(project, unscaled_scene)?;
        let started = Instant::now();
        files
            .load_into(
                &key,
                (unscaled_scene.width, unscaled_scene.height),
                &mut self.render_file_bytes,
                &mut self.render_file_image,
            )
            .then(|| started.elapsed().as_secs_f64() * 1000.0)
    }

    fn composite_render_file_into(
        &mut self,
        width: u32,
        height: u32,
        decode_ms: f64,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RenderError> {
        if let Some(gpu) = sink.adopt_gpu() {
            self.set_gpu(gpu);
        }
        let config = CompositorConfig::new(width, height).with_background([0, 0, 0, 0]);
        let layer = CompositeLayer::rgba(
            &self.render_file_image,
            LayerPlacement::full_canvas(&config),
        );
        let layers = [CompositorLayer::layer(&layer)];
        let submitted_at = Instant::now();
        let res = if let Some(texture) = sink.texture(width, height) {
            self.compositor
                .submit_compositor_layers_to_texture(&self.gpu, &config, &layers, texture)?;
            let pending = PendingFrame {
                readback: None,
                scratch: None,
                decode_ms,
                raster_ms: 0.0,
                submitted_at,
                effect_passes: 0,
                effect_pass_bind_groups: 0,
                layers: 1,
                width,
                height,
            };
            self.finish_pending_on_gpu(pending);
            Ok(())
        } else {
            let readback = self
                .compositor
                .submit_compositor_layers(&self.gpu, &config, &layers)?;
            let pending = PendingFrame {
                readback: Some(readback),
                scratch: None,
                decode_ms,
                raster_ms: 0.0,
                submitted_at,
                effect_passes: 0,
                effect_pass_bind_groups: 0,
                layers: 1,
                width,
                height,
            };
            self.finish_pending_into(pending, sink)
        };
        sink.record_readbacks(self.readback_count());
        res
    }

    fn composite_render_file_to_texture(
        &mut self,
        width: u32,
        height: u32,
        decode_ms: f64,
        texture: &wgpu::Texture,
    ) -> Result<(), RenderError> {
        let config = CompositorConfig::new(width, height).with_background([0, 0, 0, 0]);
        let layer = CompositeLayer::rgba(
            &self.render_file_image,
            LayerPlacement::full_canvas(&config),
        );
        let layers = [CompositorLayer::layer(&layer)];
        let submitted_at = Instant::now();
        self.compositor
            .submit_compositor_layers_to_texture(&self.gpu, &config, &layers, texture)?;
        let pending = PendingFrame {
            readback: None,
            scratch: None,
            decode_ms,
            raster_ms: 0.0,
            submitted_at,
            effect_passes: 0,
            effect_pass_bind_groups: 0,
            layers: 1,
            width,
            height,
        };
        self.finish_pending_on_gpu(pending);
        Ok(())
    }

    /// Composite an already-resolved [`Scene`]. `project` supplies media file
    /// paths for the decoder cache.
    ///
    /// When decoding zero-copy ([`OutputMode::Gpu`] on Apple/Windows) produces
    /// a surface the compositor can't import, this falls back to CPU decode
    /// once and retries, so unusual formats (10-bit/HDR) still render.
    pub fn render_scene(
        &mut self,
        project: &Project,
        scene: &Scene,
    ) -> Result<RgbaImage, RenderError> {
        let mut sink = ImageSink::default();
        self.render_scene_into(project, scene, &mut sink)?;
        Ok(sink
            .into_image()
            .expect("render_scene_into fills the sink on success"))
    }

    /// [`render_scene`](Self::render_scene) writing the composited rows
    /// directly into `sink`-provided storage. The sink is consulted only
    /// after the GPU work succeeded, so the CPU-decode fallback retry can
    /// reuse it — at most one attempt ever writes.
    pub fn render_scene_into(
        &mut self,
        project: &Project,
        scene: &Scene,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RenderError> {
        self.render_scene_into_policy(project, scene, sink, SeekPolicy::Exact)
    }

    /// [`render_scene`](Self::render_scene) into `texture`, premultiplied, with
    /// no copy to the CPU. `texture` is on this renderer's device and passes
    /// [`aria_compositor::check_frame_target`] for the scene's size; any other
    /// texture is refused before anything is drawn.
    pub fn render_scene_to_texture(
        &mut self,
        project: &Project,
        scene: &Scene,
        texture: &wgpu::Texture,
    ) -> Result<(), RenderError> {
        let pending =
            self.submit_scene_policy(project, scene, SeekPolicy::Exact, Output::Texture(texture))?;
        self.finish_pending_on_gpu(pending);
        Ok(())
    }

    fn render_scene_into_policy(
        &mut self,
        project: &Project,
        scene: &Scene,
        sink: &mut dyn FrameSink,
        policy: SeekPolicy,
    ) -> Result<(), RenderError> {
        if let Some(gpu) = sink.adopt_gpu() {
            self.set_gpu(gpu);
        }
        let res = if let Some(texture) = sink.texture(scene.width, scene.height) {
            let pending =
                self.submit_scene_policy(project, scene, policy, Output::Texture(texture))?;
            self.finish_pending_on_gpu(pending);
            Ok(())
        } else {
            let pending = self.submit_scene_policy(project, scene, policy, Output::Readback)?;
            self.finish_pending_into(pending, sink)
        };
        sink.record_readbacks(self.readback_count());
        res
    }

    fn submit_scene_policy(
        &mut self,
        project: &Project,
        scene: &Scene,
        policy: SeekPolicy,
        output: Output<'_>,
    ) -> Result<PendingFrame, RenderError> {
        match self.submit_scene_once(project, scene, policy, output) {
            Err(RenderError::Compositor(CompositorError::UnsupportedFormat(_)))
                if self.decode_mode == OutputMode::Gpu =>
            {
                self.decode_mode = OutputMode::Cpu;
                self.decoders.clear();
                self.submit_scene_once(project, scene, policy, output)
            }
            other => other,
        }
    }

    /// Tight size (canvas px, at transform scale 1.0) of the content
    /// `generator` draws on a `canvas_w`×`canvas_h` canvas — what a preview
    /// selection box should hug. Animated params sample at clip-local `tick`.
    /// `None` for generators that draw nothing (empty text, degenerate
    /// shapes, kinds the compositor doesn't draw yet).
    ///
    /// `&mut self`: text and pen-path sizes come from the memoized
    /// rasterizers, so a miss here warms the composite path too. Animated
    /// text measures as its shaped extent, the quad its glyphs are placed on.
    pub fn generator_content_size(
        &mut self,
        generator: &aria_models::Generator,
        canvas_w: u32,
        canvas_h: u32,
        tick: i64,
    ) -> Option<(u32, u32)> {
        let layer = crate::resolve::resolve_generator(
            generator,
            [0.0, 0.0],
            [0.5, 0.5],
            0.0,
            1.0,
            [0.0, 0.0, 1.0, 1.0],
            None,
            None,
            canvas_w as f32,
            canvas_h as f32,
            aria_models::Scale2::ONE,
            tick,
            // Sizing doesn't depend on the animation clock.
            0.0,
            Vec::new(),
        )?;
        match layer.size {
            SizeSpec::Fixed(size) => Some((size[0].round() as u32, size[1].round() as u32)),
            // Bitmap layers size to their raster, read from the memo the
            // composite draws from rather than copied out of it.
            SizeSpec::BitmapScaled(_) => self.raster_size(&layer.source),
        }
    }
}

/// Convert premultiplied RGBA readbacks into straight alpha for Slint.
fn straighten_alpha(image: &mut RgbaImage) {
    for chunk in image.pixels.as_chunks_mut::<4>().0 {
        let a = chunk[3];
        if a == 0 || a == 255 {
            continue;
        }
        let af = f32::from(a) / 255.0;
        for c in &mut chunk[..3] {
            *c = ((*c as f32 / af).round() as u32).min(255) as u8;
        }
    }
}
