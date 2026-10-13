//! The renderer and the editing engine, plus the host policies that guard a
//! command before it is applied.

use std::sync::Arc;

use aria_commands::Command;
use aria_compositor::GpuContext;
use aria_core::{Rational, RgbaImage};
use aria_engine::{Engine, EngineConfig};
use aria_models::Project;
use aria_plugin::{
    CommandProposed, Plugin, PluginContext, PluginError, PluginManifest, ProjectHost,
    RenderInterceptorService, Service,
};
use aria_render::Renderer;
use parking_lot::Mutex;

use super::register_dyn;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.render",
            name: "Render",
            version: "0.1.0",
            requires: &[
                "aria.decoder",
                "aria.compositor",
                "aria.text",
                "aria.shapes",
            ],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // The process's one GPU device. Registered rather than rendered with:
        // this plugin owns the device, `aria.engine` owns the renderer, and both
        // used to bring up a device of their own — as did `aria.compositor` —
        // three drivers and three pipeline sets for one editor, two of which no
        // caller ever read.
        match GpuContext::new_headless_blocking() {
            Ok(gpu) => {
                ctx.register(gpu);
            }
            Err(err) => {
                tracing::warn!(error = %err, "no gpu device; the editor starts without a preview");
            }
        }
        Ok(())
    }
}

pub struct EnginePlugin;

/// Refuse an edit that would change a locked lane.
///
/// A lock used to mean almost nothing: only extract-audio and freeze-frame
/// checked it, so a locked lane could still be trimmed, moved, or emptied. The
/// policy lives here rather than in the model because it is a host policy over
/// whole commands, and it reads the ids from [`EditCommand::targets`], which is
/// exhaustive — a new command variant cannot slip past it.
fn locked_lane_veto(project: &Project, command: &Command) -> Option<String> {
    let Command::Edit(edit) = command else {
        return None;
    };
    if edit.ignores_track_lock() {
        return None;
    }
    let targets = edit.targets();
    let timeline = project.timeline();
    let mut lanes: Vec<_> = targets.tracks.clone();
    for clip in targets.clips {
        if let Some(track) = timeline.track_of(clip) {
            lanes.push(track);
        }
    }
    lanes.into_iter().find_map(|id| {
        let track = timeline.track(id)?;
        track
            .locked
            .then(|| format!("{} is locked", track.name.clone()))
    })
}

struct FrameGate;

impl Service for FrameGate {}

impl RenderInterceptorService for FrameGate {
    fn before_frame(&self, _time: aria_core::RationalTime, _project: &Project) -> bool {
        true
    }

    fn after_frame(&self, _frame: &mut RgbaImage) {}
}

impl Plugin for EnginePlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.engine",
            name: "Engine",
            version: "0.1.0",
            requires: &["aria.render"],
            // The device the render plugin brought up. Declared, so the refusal
            // ledger shows it if this is ever mounted without one.
            uses: &["GpuContext"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // One device for the process: the engine renders on the render plugin's,
        // rather than bringing up a second driver instance and a second pipeline
        // set of its own. Without one there is no preview, which is what the
        // warning below has always meant.
        let engine = match ctx.try_get::<GpuContext>() {
            Some(gpu) => Ok(Engine::with_renderer(
                EngineConfig::default(),
                Project::new("untitled", Rational::FPS_24),
                Renderer::with_gpu(gpu.share()),
            )),
            None => Engine::new(EngineConfig::default()),
        };
        match engine {
            Ok(mut engine) => {
                engine.new_session();
                let shared = Arc::new(Mutex::new(engine));
                ctx.register_arc(Arc::clone(&shared));
                ctx.register(ProjectHost::new(shared, ctx));
                // What the window shows besides the project, recorded per drawn
                // frame; a plugin reads it by declaring `EditorView`.
                ctx.register(aria_plugin::EditorView::new());
                // The lock policy is a waterfall handler, not a service: the host
                // asks every plugin in mount order and the first refusal wins. It
                // reads the project the event carries; the engine is locked while
                // the event runs.
                ctx.on_waterfall::<CommandProposed, _>(|proposed| {
                    if let Some(reason) = locked_lane_veto(&proposed.project, &proposed.command)
                    {
                        proposed.refuse(reason);
                    }
                });
            }
            Err(err) => {
                tracing::warn!(error = %err, "engine gpu session unavailable");
            }
        }
        register_dyn::<dyn RenderInterceptorService>(ctx, Arc::new(FrameGate));
        // The project extension is not claimed here on purpose: the host decides it,
        // so a dropped project still opens on a machine where this plugin failed to
        // mount.
        Ok(())
    }
}
