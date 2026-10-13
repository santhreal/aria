//! GPU work: the compositor, and the effect and transition catalogs.

use std::sync::Arc;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, GpuContext, LayerPlacement,
    PassInstance, pass_id, renders_effect, renders_transition,
};
use aria_core::RgbaImage;
use aria_plugin::{
    DropAction, EffectService, Plugin, PluginContext, PluginError, PluginManifest,
    ResolveDroppedPath, Service, TransitionService,
};
use aria_render::RenderError;
use parking_lot::Mutex;

use super::{has_extension, register_dyn};

pub struct CompositorPlugin;

struct CompositorHost;

impl Service for CompositorHost {}

impl Plugin for CompositorPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.compositor",
            name: "Compositor",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // The effect and transition catalogs: what this plugin registers is a
        // description of what can be composited, not a device. `aria.render`
        // brings the device up, once, and the engine renders on it. A second
        // `GpuContext` and a second `Compositor` were built here and never read
        // — 288 ms of a 583 ms start, and a whole driver instance and pipeline
        // set held in video memory for the life of the process.
        ctx.register(CompositorHost);
        Ok(())
    }
}

/// Renders one image at a time on the process's GPU device for a plugin that
/// asks, with a compositor built on the first call and kept for the next.
struct OneShot {
    gpu: Option<Arc<GpuContext>>,
    compositor: Mutex<Option<Compositor>>,
}

impl OneShot {
    fn new(ctx: &PluginContext) -> Self {
        Self {
            gpu: ctx.try_get::<GpuContext>(),
            compositor: Mutex::new(None),
        }
    }

    /// A transparent canvas of one size, and the device to draw it on.
    fn canvas(&self, width: u32, height: u32) -> Result<(&GpuContext, CompositorConfig), RenderError> {
        let gpu = self
            .gpu
            .as_deref()
            .ok_or_else(|| RenderError::unsupported("no GPU device is available"))?;
        Ok((gpu, CompositorConfig::new(width, height).with_background([0, 0, 0, 0])))
    }

    /// Draw `layers` and read the canvas back as straight alpha.
    fn draw(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositorLayer<'_>],
    ) -> Result<RgbaImage, RenderError> {
        let mut held = self.compositor.lock();
        let comp = held.get_or_insert_with(|| Compositor::new(gpu));
        let mut image = comp.render_compositor_layers(gpu, config, layers)?;
        // Drawn over a transparent canvas, the readback is premultiplied.
        image.unpremultiply();
        Ok(image)
    }
}

pub struct EffectPlugin;

struct EffectCatalog(OneShot);

impl Service for EffectCatalog {}

impl EffectService for EffectCatalog {
    fn renders_effect(&self, id: &str) -> bool {
        renders_effect(id)
    }

    fn render_effect(
        &self,
        id: &str,
        params: &[f32],
        input: &RgbaImage,
    ) -> Result<RgbaImage, RenderError> {
        let pass = pass_id(id)
            .ok_or_else(|| RenderError::unsupported(format!("no pass renders effect `{id}`")))?;
        let (gpu, config) = self.0.canvas(input.width, input.height)?;
        let effects = [PassInstance::new(pass, params.to_vec())];
        let layer = CompositeLayer::rgba(input, LayerPlacement::full_canvas(&config))
            .with_effects(&effects);
        self.0.draw(gpu, &config, &[CompositorLayer::layer(&layer)])
    }
}

impl Plugin for EffectPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.effects",
            name: "Effects",
            version: "0.1.0",
            // `aria.render` brings up the device a plugin's render call draws on.
            requires: &["aria.compositor", "aria.render"],
            uses: &["GpuContext"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn EffectService>(ctx, Arc::new(EffectCatalog(OneShot::new(ctx))));
        // A `.cube` file is a grade, not media.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            has_extension(&event.path, &["cube"]).then_some(DropAction::ApplyLut)
        });
        Ok(())
    }
}

pub struct TransitionPlugin;

struct TransitionCatalog(OneShot);

impl Service for TransitionCatalog {}

impl TransitionService for TransitionCatalog {
    fn renders_transition(&self, id: &str) -> bool {
        renders_transition(id)
    }

    fn render_transition(
        &self,
        id: &str,
        outgoing: &RgbaImage,
        incoming: &RgbaImage,
        progress: f32,
    ) -> Result<RgbaImage, RenderError> {
        if !renders_transition(id) {
            return Err(RenderError::unsupported(format!(
                "no pass renders transition `{id}`"
            )));
        }
        if (outgoing.width, outgoing.height) != (incoming.width, incoming.height) {
            return Err(RenderError::unsupported(format!(
                "transition `{id}` blends images of one size; got {}x{} and {}x{}",
                outgoing.width, outgoing.height, incoming.width, incoming.height
            )));
        }
        let (gpu, config) = self.0.canvas(outgoing.width, outgoing.height)?;
        let placement = LayerPlacement::full_canvas(&config);
        let out_layer = CompositeLayer::rgba(outgoing, placement);
        let in_layer = CompositeLayer::rgba(incoming, placement);
        let items = [CompositorLayer::Transition {
            outgoing: &out_layer,
            incoming: &in_layer,
            transition_id: id,
            progress: progress.clamp(0.0, 1.0),
        }];
        self.0.draw(gpu, &config, &items)
    }
}

impl Plugin for TransitionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.transitions",
            name: "Transitions",
            version: "0.1.0",
            requires: &["aria.compositor", "aria.render"],
            uses: &["GpuContext"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn TransitionService>(ctx, Arc::new(TransitionCatalog(OneShot::new(ctx))));
        Ok(())
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
