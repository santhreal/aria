//! GPU work: the compositor, the effect and transition catalogs, and the
//! text and shape generators.

use std::sync::Arc;

use aria_compositor::{GpuContext, PassDescriptor, effect_descriptors, transition_descriptors};
use aria_core::{GpuSurface, RgbaImage};
use aria_plugin::{
    DropAction, EffectService, GeneratorService, Plugin, PluginContext, PluginError,
    PluginManifest, ResolveDroppedPath, Service, TransitionService,
};
use aria_render::RenderError;
use aria_shapes::{SdfParams, ShapeStyle, raster};
use aria_text::{TextRenderer, TextStyle};
use parking_lot::Mutex;

use super::{GENERATOR_KINDS, has_extension, register_dyn};

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
        // The effect and transition catalogs, the text and shape generators: what
        // this plugin registers is a description of what can be composited, not a
        // device. `aria.render` brings the device up, once, and the engine renders
        // on it. A second `GpuContext` and a second `Compositor` were built here
        // and never read — 288 ms of a 583 ms start, and a whole driver instance
        // and pipeline set held in video memory for the life of the process.
        ctx.register(CompositorHost);
        Ok(())
    }
}

pub struct EffectPlugin;

struct EffectCatalog;

impl Service for EffectCatalog {}

impl EffectService for EffectCatalog {
    fn effect_descriptors(&self) -> &[PassDescriptor] {
        effect_descriptors()
    }

    fn render_effect(
        &self,
        id: &str,
        _params: &[f32],
        _input: &GpuSurface,
        _gpu: &GpuContext,
    ) -> Result<GpuSurface, RenderError> {
        Err(RenderError::unsupported(format!(
            "effect '{id}' has no public compositor dispatch yet"
        )))
    }
}

impl Plugin for EffectPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.effects",
            name: "Effects",
            version: "0.1.0",
            requires: &["aria.compositor"],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn EffectService>(ctx, Arc::new(EffectCatalog));
        // A `.cube` file is a grade, not media.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            has_extension(&event.path, &["cube"]).then_some(DropAction::ApplyLut)
        });
        Ok(())
    }
}

pub struct TransitionPlugin;

struct TransitionCatalog;

impl Service for TransitionCatalog {}

impl TransitionService for TransitionCatalog {
    fn transition_descriptors(&self) -> &[PassDescriptor] {
        transition_descriptors()
    }

    fn render_transition(
        &self,
        id: &str,
        _params: &[f32],
        _a: &GpuSurface,
        _b: &GpuSurface,
        _t: f32,
        _gpu: &GpuContext,
    ) -> Result<GpuSurface, RenderError> {
        Err(RenderError::unsupported(format!(
            "transition '{id}' has no public compositor dispatch yet"
        )))
    }
}

impl Plugin for TransitionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.transitions",
            name: "Transitions",
            version: "0.1.0",
            requires: &["aria.compositor"],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn TransitionService>(ctx, Arc::new(TransitionCatalog));
        Ok(())
    }
}

struct CombinedGenerators {
    text: Mutex<TextRenderer>,
}

impl CombinedGenerators {
    fn new() -> Self {
        Self {
            text: Mutex::new(TextRenderer::new()),
        }
    }
}

impl Service for CombinedGenerators {}

impl GeneratorService for CombinedGenerators {
    fn generator_kinds(&self) -> &[&str] {
        GENERATOR_KINDS
    }

    fn render(&self, kind: &str, params: &[(String, String)]) -> Result<RgbaImage, RenderError> {
        match kind {
            "text" => {
                let content = params
                    .iter()
                    .find(|(k, _)| k == "content")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("Aria");
                let size = params
                    .iter()
                    .find(|(k, _)| k == "size")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(48.0);
                let style = TextStyle::new(size);
                Ok(self.text.lock().rasterize(content, &style))
            }
            "shape" => {
                let half = 64.0;
                let shape = SdfParams::RoundedRect { radius: 8.0 }.with_half([half, half]);
                let style = ShapeStyle {
                    fill: Some([255, 255, 255, 255]),
                    stroke: None,
                };
                Ok(raster(&shape, &style))
            }
            "solid" => Ok(RgbaImage::transparent(8, 8)),
            other => Err(RenderError::unsupported(format!(
                "unknown generator '{other}'"
            ))),
        }
    }
}

fn ensure_generators(ctx: &PluginContext) {
    if ctx.has::<dyn GeneratorService>() {
        return;
    }
    register_dyn::<dyn GeneratorService>(ctx, Arc::new(CombinedGenerators::new()));
}

pub struct TextPlugin;

impl Plugin for TextPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.text",
            name: "Text",
            version: "0.1.0",
            requires: &["aria.compositor"],
            // Registers the combined generator catalog only if nothing did yet.
            uses: &["GeneratorService"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ensure_generators(ctx);
        Ok(())
    }
}

pub struct ShapePlugin;

impl Plugin for ShapePlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.shapes",
            name: "Shapes",
            version: "0.1.0",
            requires: &["aria.compositor"],
            uses: &["GeneratorService"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ensure_generators(ctx);
        Ok(())
    }
}
