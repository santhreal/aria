//! GPU work: the compositor, the effect and transition catalogs, and the
//! `.cube` grade importer.

use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use aria_commands::EditOutcome;
use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, CubeLut, GpuContext,
    LayerPlacement, PassInstance, pass_id, renders_effect, renders_transition,
};
use aria_core::RgbaImage;
use aria_models::{Generator, Lut, TimeRange, TrackKind};
use aria_plugin::{
    ApplyOutcome, Command, DropAction, EditCommand, EffectService, Importer, ImporterSpec,
    InterchangeReport, Plugin, PluginContext, PluginError, PluginManifest, ResolveDropped, Service,
    TransactError, Transaction, TransitionService,
};
use aria_render::RenderError;
use parking_lot::Mutex;

use super::{has_extension, register_dyn};

#[derive(Debug)]
pub struct CompositorPlugin;

struct CompositorHost;

impl Service for CompositorHost {}

impl Plugin for CompositorPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.compositor", "Compositor", "0.1.0", &[], &[]);
        &MANIFEST
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
    fn canvas(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(&GpuContext, CompositorConfig), RenderError> {
        let gpu = self
            .gpu
            .as_deref()
            .ok_or_else(|| RenderError::unsupported("no GPU device is available"))?;
        Ok((
            gpu,
            CompositorConfig::new(width, height).with_background([0, 0, 0, 0]),
        ))
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

#[derive(Debug)]
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
        // `aria.render` brings up the device a plugin's render call draws on.
        static MANIFEST: PluginManifest = PluginManifest::new(
            "aria.effects",
            "Effects",
            "0.1.0",
            &["aria.compositor", "aria.render"],
            &["GpuContext"],
        );
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn EffectService>(ctx, Arc::new(EffectCatalog(OneShot::new(ctx))));
        // A `.cube` file is a grade, not media: dropped with a clip selected it
        // grades that clip, and read by the importer it grades the sequence.
        ctx.on_serial::<ResolveDropped, DropAction, _>(|event| {
            event
                .item
                .as_path()
                .is_some_and(|path| has_extension(path, &["cube"]))
                .then_some(DropAction::ApplyLut)
        });
        ctx.register_importer(
            ImporterSpec {
                id: "aria.effects.cube".to_owned(),
                name: "Cube LUT".to_owned(),
                extensions: vec!["cube".to_owned()],
            },
            CubeImporter,
        )?;
        Ok(())
    }
}

/// The largest `.cube` file read. A 65³ table, the largest size parsed, is
/// under 20 MiB.
const MAX_CUBE_BYTES: u64 = 64 << 20;

/// Reads a `.cube` grade as a LUT filter bar across the sequence, on a new
/// filter track named after the file, as one undo step. An empty sequence
/// gets a five-second bar.
struct CubeImporter;

impl Importer for CubeImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let aborted =
            |reason: String| TransactError::Aborted(format!("{} {reason}", file.display()));
        let path = std::path::absolute(file)
            .map_err(|err| aborted(format!("could not be read: {err}")))?;
        let lut_path = path
            .to_str()
            .ok_or_else(|| aborted("could not be read: the path is not UTF-8".to_owned()))?
            .to_owned();
        let text = read_cube(&path).map_err(|err| aborted(format!("could not be read: {err}")))?;
        CubeLut::parse(&text).map_err(|err| aborted(format!("is not a Cube LUT: {err}")))?;

        let project = tx.project();
        let timeline = project.timeline();
        let rate = timeline.frame_rate;
        let length = match timeline.duration().value {
            0 => (5 * i64::from(rate.num) / i64::from(rate.den)).max(1),
            ticks => ticks,
        };
        let name = file.file_stem().map_or_else(
            || "LUT".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let track = match tx.apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Filter,
            name,
            index: None,
            pinned: false,
        }))? {
            ApplyOutcome::Edited(EditOutcome::CreatedTrack(track)) => track,
            other => return Err(aborted(format!("could not add its track: {other:?}"))),
        };
        let clip = match tx.apply(Command::Edit(EditCommand::AddGenerated {
            track,
            generator: Generator::Filter,
            timeline: TimeRange::at_rate(0, length, rate),
        }))? {
            ApplyOutcome::Edited(EditOutcome::Created(clip)) => clip,
            other => return Err(aborted(format!("could not add its filter: {other:?}"))),
        };
        tx.apply(Command::Edit(EditCommand::SetClipLut {
            clip,
            lut: Some(Lut::new(lut_path)),
        }))?;
        Ok(InterchangeReport::default())
    }
}

/// The text of the `.cube` file at `path`, refused past [`MAX_CUBE_BYTES`].
fn read_cube(path: &Path) -> io::Result<String> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("LUT files over {} MiB are not read", MAX_CUBE_BYTES >> 20),
        )
    };
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_CUBE_BYTES {
        return Err(too_large());
    }
    // A pipe or device states no length; the read stops one byte past the limit.
    let mut text = String::new();
    file.take(MAX_CUBE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_CUBE_BYTES {
        return Err(too_large());
    }
    Ok(text)
}

#[derive(Debug)]
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
        static MANIFEST: PluginManifest = PluginManifest::new(
            "aria.transitions",
            "Transitions",
            "0.1.0",
            &["aria.compositor", "aria.render"],
            &["GpuContext"],
        );
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn TransitionService>(ctx, Arc::new(TransitionCatalog(OneShot::new(ctx))));
        Ok(())
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
