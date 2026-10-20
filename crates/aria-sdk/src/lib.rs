//! Safe Rust guest layer over [`aria_plugin_abi`].
//!
//! A native plugin implements [`Plugin`] and exports its entry table with
//! [`export!`]. Every callback from the host is wrapped in `catch_unwind` at
//! the C ABI boundary so a panic becomes an `ARIA_STATUS_PANIC` return rather
//! than unwinding across `extern "C"`.

#![deny(missing_docs)]

mod ai_tools;
mod analyzers;
mod audio;
mod clip_types;
mod cpu_effects;
mod decoders;
mod drivers;
mod effect_graph;
mod effects;
mod encoders;
mod export;
mod host;
mod interchange;
mod overlays;
mod panels;
mod presets;
mod project;
mod runtime;
mod scopes;
mod workspaces;

use std::path::Path;

pub use ai_tools::{AiToolImage, AiToolOutput, AiToolSpec, AiToolTier};
pub use analyzers::{
    Analyzer, AnalyzerLoudness, AnalyzerMarker, AnalyzerRange, AnalyzerResult, AnalyzerRun,
    AnalyzerScope, AnalyzerSpec,
};
pub use aria_plugin_abi as abi;
pub use audio::{AudioBlock, AudioEffectSpec, AudioParam, AudioParamSpec, AudioProcessor};
pub use clip_types::{ClipTypeSchema, ClipTypeSpec, MarkerColor};
pub use cpu_effects::{CpuEffect, CpuFrameIn, CpuFrameOut, f16};
pub use decoders::{
    AudioStream, DecodeFailure, Decoder, DecoderProbe, DecoderSpec, FrameColor, FrameFormat,
    FramePlane, VideoInfo, VideoStream,
};
pub use drivers::{DriverLanguage, DriverLanguageSpec, DriverVars, ParamValue};
pub use effect_graph::{EffectBounds, GraphPass, OutputSize, PassInput, PassStage};
pub use effects::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectChoiceOption, EffectCurvePoint,
    EffectGradientStop, EffectGroupSpec, EffectParamDisplay, EffectParamKind, EffectParamSpec,
    EffectSpec, EffectVisibilityCondition, TransitionSpec,
};
pub use encoders::{
    Encoder, EncoderAudio, EncoderConfig, EncoderFrame, EncoderSession, EncoderSpec,
};
pub use host::Host;
pub use interchange::{ExportReport, ExporterSpec, ImportResult, ImporterSpec};
pub use overlays::{
    DragPhase, HandleLook, OverlayColor, OverlayEvent, OverlayScene, OverlayShape, OverlaySpec,
};
pub use panels::{Panel, PanelEvent, PanelLocation, PanelSpec, PanelView, Tone, UiNode};
pub use presets::PresetSpec;
pub use project::{ClipRef, InOut, Marker, MarkerKind, Rational, RationalTime, TimeRange};
pub use runtime::{PluginRuntime, panic_message};
pub use scopes::{ScopeFrame, ScopeImage, ScopeSpec};
pub use serde_json;
pub use workspaces::{WorkspaceFocus, WorkspacePanes, WorkspaceSpec};

use aria_plugin_abi::{
    ARIA_LOG_DEBUG, ARIA_LOG_ERROR, ARIA_LOG_INFO, ARIA_LOG_TRACE, ARIA_LOG_WARN,
};
use serde::{Deserialize, Serialize};

/// Severity of a plugin log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Step-by-step detail.
    Trace,
    /// Diagnostic detail.
    Debug,
    /// Normal operation.
    Info,
    /// A problem the plugin recovered from.
    Warn,
    /// A failure.
    Error,
}

impl LogLevel {
    pub(crate) fn to_raw(self) -> u32 {
        match self {
            Self::Trace => ARIA_LOG_TRACE,
            Self::Debug => ARIA_LOG_DEBUG,
            Self::Info => ARIA_LOG_INFO,
            Self::Warn => ARIA_LOG_WARN,
            Self::Error => ARIA_LOG_ERROR,
        }
    }
}

/// Action claimed for a `resolve-dropped` serial event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DropAction {
    /// Open it as an Aria project.
    OpenProject,
    /// Register it in the media pool.
    ImportMedia,
    /// Load it as a 3D LUT.
    ApplyLut,
    /// Install it as an assistant skill.
    InstallSkill,
    /// Read it into the open project through the importer registered for its
    /// extension.
    ImportTimeline,
    /// The plugin handled the dropped item itself.
    Handled,
}

/// Response returned from [`Plugin::event`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EventResponse {
    /// No modification or claim (used for all notification events).
    #[default]
    Pass,
    /// Refuse a `command-proposed` event with a user-facing reason.
    Refuse(String),
    /// Rewrite the command in a `command-proposed` event with replacement command JSON.
    RewriteCommand(String),
    /// Claim a `resolve-dropped` event with `DropAction`.
    ClaimDrop(DropAction),
    /// Raw JSON response payload for custom or future waterfall/serial events.
    Json(String),
}

impl EventResponse {
    pub(crate) fn into_json(self) -> Option<String> {
        match self {
            Self::Pass => None,
            Self::Refuse(reason) => Some(serde_json::json!({ "veto": reason }).to_string()),
            Self::RewriteCommand(command_json) => Some(command_json),
            Self::ClaimDrop(action) => Some(serde_json::to_string(&action).unwrap_or_default()),
            Self::Json(raw) => Some(raw),
        }
    }
}

/// Trait implemented by an Aria native plugin.
pub trait Plugin: Send + Sync + 'static {
    /// Called when the plugin is activated. Registrations are allowed only here.
    fn mount(&self, host: &Host) -> Result<(), String>;

    /// Called when the plugin is unloaded or disabled.
    fn unmount(&self, _host: &Host) -> Result<(), String> {
        Ok(())
    }

    /// Execute a command registered by this plugin.
    fn command_run(&self, _host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        Err(format!("unknown command: {id}"))
    }

    /// Handle an event whose kind this plugin subscribed to during `mount`.
    fn event(&self, _host: &Host, _kind: &str, _event_json: &str) -> Result<EventResponse, String> {
        Ok(EventResponse::Pass)
    }

    /// Run a registered timeline importer on `path`.
    fn import(
        &self,
        _host: &Host,
        importer_id: &str,
        _path: &Path,
    ) -> Result<ImportResult, String> {
        Err(format!("unknown importer: {importer_id}"))
    }

    /// Run a registered timeline exporter for `project_json` writing to `path`.
    fn export(
        &self,
        _host: &Host,
        exporter_id: &str,
        _project_json: &str,
        _path: &Path,
    ) -> Result<ExportReport, String> {
        Err(format!("unknown exporter: {exporter_id}"))
    }

    /// Create an audio processor instance for `effect_id` at `sample_rate`.
    fn audio_create(
        &self,
        _host: &Host,
        effect_id: &str,
        _sample_rate: u32,
    ) -> Result<Box<dyn AudioProcessor>, String> {
        Err(format!("unknown audio effect: {effect_id}"))
    }

    /// Build the declarative widget tree for `panel_id` when no [`Panel`] object
    /// was registered with [`Host::register_panel`].
    fn panel_view(&self, _host: &Host, panel_id: &str) -> Result<UiNode, String> {
        Err(format!("unknown panel: {panel_id}"))
    }

    /// Handle a user interaction event on `panel_id` when no [`Panel`] object
    /// was registered with [`Host::register_panel`].
    fn panel_event(&self, _host: &Host, panel_id: &str, _event: &PanelEvent) -> Result<(), String> {
        Err(format!("unknown panel: {panel_id}"))
    }

    /// Open an encoder session for `encoder_id` when no [`Encoder`] object was
    /// registered with [`Host::register_encoder`].
    fn encoder_open(
        &self,
        _host: &Host,
        encoder_id: &str,
        _path: &Path,
        _config: &EncoderConfig,
    ) -> Result<Box<dyn EncoderSession>, String> {
        Err(format!("unknown encoder: {encoder_id}"))
    }

    /// Build the viewer overlay scene for `id`.
    fn overlay_scene(&self, _host: &Host, _id: &str) -> OverlayScene {
        OverlayScene::default()
    }

    /// Handle a user interaction event on viewer overlay `id`.
    fn overlay_event(&self, _host: &Host, id: &str, _event: &OverlayEvent) -> Result<(), String> {
        Err(format!("no overlay {id}"))
    }

    /// Run one step of job `job` started with [`Host::start_job`], on the job's thread.
    ///
    /// The host calls this until a step returns `done` or an error, or the job is cancelled.
    fn job_step(&self, _host: &Host, job: u64) -> Result<JobStep, String> {
        Err(format!("no job {job}"))
    }

    /// Release the state of job `job`; `cancelled` is true when it stopped before `done`.
    fn job_end(&self, _host: &Host, _job: u64, _cancelled: bool) {}

    /// Run the AI tool `name` registered with [`Host::register_ai_tool`], on
    /// the assistant's turn thread. `arguments` is an object matching the
    /// tool's schema. An `Err` reaches the model as written.
    fn ai_tool_call(
        &self,
        _host: &Host,
        name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<AiToolOutput, String> {
        Err(format!("no AI tool {name}"))
    }

    /// Measure `frame` with the scope `id` registered with
    /// [`Host::register_scope`], drawing into `image`, on the preview worker's
    /// thread while the scope strip is open. Returns the readout line shown
    /// under the image, empty for none: at most 160 bytes and no line break.
    fn scope_measure(
        &self,
        _host: &Host,
        id: &str,
        _frame: &ScopeFrame<'_>,
        _image: &mut ScopeImage<'_>,
    ) -> Result<String, String> {
        Err(format!("no scope {id}"))
    }

    /// Process `input` into `output` for CPU effect or generator `id` when no
    /// [`CpuEffect`] object was bound with [`Host::register_cpu_effect`] or
    /// [`Host::register_generator`].
    fn cpu_effect_process(
        &self,
        _host: &Host,
        id: &str,
        _input: &CpuFrameIn<'_>,
        _output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        Err(format!("no cpu effect {id}"))
    }

    /// Evaluate driver expression `expr` in language `id` for `vars` when no
    /// [`DriverLanguage`] object was bound with [`Host::register_driver_language`].
    fn driver_evaluate(
        &self,
        _host: &Host,
        id: &str,
        _expr: &str,
        _vars: &DriverVars,
    ) -> Result<ParamValue, String> {
        Err(format!("no driver language {id}"))
    }

    /// Open an audio analyzer session for `id` when no [`Analyzer`] object was
    /// bound with [`Host::register_analyzer`].
    fn analyzer_open(
        &self,
        _host: &Host,
        id: &str,
        _sample_rate: u32,
        _channels: u16,
    ) -> Result<Box<dyn AnalyzerRun>, String> {
        Err(format!("no analyzer {id}"))
    }
}

/// One step of a job, returned from [`Plugin::job_step`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobStep {
    /// Fraction done, `0.0..=1.0`, or `None` while it is unknown.
    pub progress: Option<f32>,
    /// The line the jobs board shows.
    pub detail: String,
    /// The job is finished, and `detail` is its last line.
    pub done: bool,
    /// The next step cannot be undone: Cancel is refused from here on.
    pub commit: bool,
}
