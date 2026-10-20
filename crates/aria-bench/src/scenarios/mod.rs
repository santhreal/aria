//! Benchmark scenario implementations for `docs/spec/performance.md`.

pub(crate) mod audio_load;
pub(crate) mod av_sync;
pub(crate) mod edit_apply;
pub(crate) mod export;
pub(crate) mod memory;
pub(crate) mod param_latency;
pub(crate) mod play_start;
pub(crate) mod playback;
pub(crate) mod plugin_cost;
pub(crate) mod project_io;
pub(crate) mod scrub;
pub(crate) mod startup;
pub(crate) mod timeline_frame;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_compositor::{FrameSink, GpuContext, create_frame_target, wgpu};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::shell::Shell;
use gpui::AppContext as _;

use crate::{EnvironmentInfo, ScenarioId, ScenarioReport, mount_builtin_plugins};

/// Execution context shared across benchmark scenarios.
#[derive(Debug, Clone)]
pub struct BenchContext {
    pub workspace_root: PathBuf,
    pub fixtures_dir: PathBuf,
    pub results_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub env: EnvironmentInfo,
}

impl BenchContext {
    pub fn new(
        env: EnvironmentInfo,
        fixtures_dir: PathBuf,
        results_dir: PathBuf,
        scratch_dir: PathBuf,
    ) -> Self {
        Self {
            workspace_root: crate::default_workspace_root(),
            fixtures_dir,
            results_dir,
            scratch_dir,
            env,
        }
    }

    pub fn ensure_scratch_subdir(&self, name: &str) -> Result<PathBuf, String> {
        let dir = self.scratch_dir.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "failed to create scratch directory {}: {err}",
                dir.display()
            )
        })?;
        Ok(dir)
    }
}

/// A headless editor window: the dark theme, the built-in plugins mounted with
/// `data_dir` as the data directory, and a [`Shell`] over a new [`AppState`].
pub(crate) fn open_editor(
    data_dir: &Path,
) -> (
    gpui::TestApp,
    gpui::Entity<AppState>,
    gpui::TestAppWindow<Shell>,
) {
    let mut app = gpui::TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(aria_ui::icons::Assets),
    );
    let data_dir = data_dir.to_path_buf();
    let state = app.update(|cx| {
        aria_ui::theme::Theme::install(
            aria_ui::theme::Appearance::Dark,
            Default::default(),
            None,
            cx,
        );
        let plugin_ctx = PluginContext::new();
        mount_builtin_plugins(&plugin_ctx, &data_dir).expect("built-in plugins mount");
        cx.new(|_| AppState::new(plugin_ctx, data_dir).expect("editor state"))
    });
    let owned = gpui::Entity::clone(&state);
    let window = app.open_window(|_, cx| Shell::new(owned, cx));
    (app, state, window)
}

/// Open the project at `path` in `state` with In and Out over frames 0 to 24.
pub(crate) fn open_marked_project(
    app: &mut gpui::TestApp,
    state: &gpui::Entity<AppState>,
    path: PathBuf,
) {
    app.update(|cx| {
        state.update(cx, |s, cx| {
            s.open_project(path).expect("load project-10k");
            s.mark_in = Some(0);
            s.mark_out = Some(24);
            cx.notify();
        });
    });
}

/// Reusable GPU texture sink for display-path renders with no CPU readback.
pub(crate) struct GpuTextureSink {
    gpu: GpuContext,
    target: Option<(u32, u32, wgpu::Texture)>,
    cpu_scratch: Vec<u8>,
    cpu_frames: u64,
}

impl std::fmt::Debug for GpuTextureSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dims = self.target.as_ref().map(|(w, h, _)| (*w, *h));
        f.debug_struct("GpuTextureSink")
            .field("dims", &dims)
            .field("cpu_frames", &self.cpu_frames)
            .finish()
    }
}

impl GpuTextureSink {
    pub(crate) fn new(gpu: GpuContext) -> Self {
        Self {
            gpu,
            target: None,
            cpu_scratch: Vec::new(),
            cpu_frames: 0,
        }
    }

    pub(crate) fn preallocate(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        let tex = create_frame_target(&self.gpu.device, w, h);
        self.target = Some((w, h, tex));
    }
}

impl FrameSink for GpuTextureSink {
    fn texture(&mut self, width: u32, height: u32) -> Option<&wgpu::Texture> {
        let w = width.max(1);
        let h = height.max(1);
        let needs_alloc = match &self.target {
            Some((cw, ch, _)) => *cw != w || *ch != h,
            None => true,
        };
        if needs_alloc {
            let tex = create_frame_target(&self.gpu.device, w, h);
            self.target = Some((w, h, tex));
        }
        self.target.as_ref().map(|(_, _, tex)| tex)
    }

    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        self.cpu_frames += 1;
        let len = (width as usize) * (height as usize) * 4;
        self.cpu_scratch.resize(len, 0);
        &mut self.cpu_scratch
    }
}

/// Run a single benchmark scenario and return its structured report.
pub fn run_scenario(ctx: &BenchContext, id: ScenarioId) -> Result<ScenarioReport, String> {
    match id {
        ScenarioId::Startup => startup::run(ctx),
        ScenarioId::ProjectIo => project_io::run(ctx),
        ScenarioId::PlayStart => play_start::run(ctx),
        ScenarioId::Playback => playback::run(ctx),
        ScenarioId::AvSync => av_sync::run(ctx),
        ScenarioId::Scrub => scrub::run(ctx),
        ScenarioId::ParamLatency => param_latency::run(ctx),
        ScenarioId::TimelineFrame => timeline_frame::run(ctx),
        ScenarioId::EditApply => edit_apply::run(ctx),
        ScenarioId::Export => export::run(ctx),
        ScenarioId::AudioLoad => audio_load::run(ctx),
        ScenarioId::Memory => memory::run(ctx),
        ScenarioId::PluginCost => plugin_cost::run(ctx),
    }
}
