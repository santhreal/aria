//! Safe Rust guest layer over [`aria_plugin_abi`].
//!
//! A native plugin implements [`Plugin`] and exports its entry table with
//! [`export!`]. Every callback from the host is wrapped in `catch_unwind` at
//! the C ABI boundary so a panic becomes an `ARIA_STATUS_PANIC` return rather
//! than unwinding across `extern "C"`.

#![deny(missing_docs)]

mod ai_tools;
mod clip_type_schema;
mod contributions;
mod decoders;
mod effect_graph;
mod host;
mod overlays;
mod panels;
mod runtime;
mod scopes;

use std::path::Path;

pub use ai_tools::{AiToolImage, AiToolOutput, AiToolSpec, AiToolTier};
pub use aria_plugin_abi as abi;
pub use clip_type_schema::ClipTypeSchema;
pub use contributions::{
    Analyzer, AnalyzerLoudness, AnalyzerMarker, AnalyzerRange, AnalyzerResult, AnalyzerRun,
    AnalyzerScope, AnalyzerSpec, ClipTypeSpec, CpuEffect, CpuFrameIn, CpuFrameOut, DriverLanguage,
    DriverLanguageSpec, DriverVars, EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES,
    EffectChoiceOption, EffectCurvePoint, EffectGradientStop, EffectGroupSpec, EffectParamDisplay,
    EffectParamKind, EffectParamSpec, EffectSpec, EffectVisibilityCondition, MarkerColor,
    ParamValue, PresetSpec, TransitionSpec, WorkspaceFocus, WorkspacePanes, WorkspaceSpec, f16,
};
pub use decoders::{
    AudioStream, DecodeFailure, Decoder, DecoderProbe, DecoderSpec, FrameColor, FrameFormat,
    FramePlane, VideoInfo, VideoStream,
};
pub use effect_graph::{EffectBounds, GraphPass, OutputSize, PassInput, PassStage};
pub use host::Host;
pub use overlays::{
    DragPhase, HandleLook, OverlayColor, OverlayEvent, OverlayScene, OverlayShape, OverlaySpec,
};
pub use panels::{Panel, PanelEvent, PanelLocation, PanelSpec, PanelView, Tone, UiNode};
pub use runtime::{PluginRuntime, panic_message};
pub use scopes::{ScopeFrame, ScopeImage, ScopeSpec};
pub use serde_json;

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

/// Specification of a timeline importer registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterSpec {
    /// Not empty, and unique among importers and exporters.
    pub id: String,
    /// The format as a menu lists it, such as `CMX 3600 EDL`.
    pub name: String,
    /// The file extensions it reads, without the dot: at least one, each ASCII
    /// letters and digits. Matched without regard to case.
    pub extensions: Vec<String>,
}

/// Specification of a timeline exporter registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExporterSpec {
    /// Not empty, and unique among importers and exporters.
    pub id: String,
    /// The format as a menu lists it.
    pub name: String,
    /// The extension of the file it writes, without the dot: ASCII letters and
    /// digits, stored lowercase.
    pub extension: String,
}

/// Specification of a media format encoder registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncoderSpec {
    /// Not empty, and unique among encoders.
    pub id: String,
    /// The format as the Export dialog lists it, such as `Apple ProRes 422`.
    pub name: String,
    /// The extension of the file it writes, without the dot: ASCII letters and
    /// digits, stored lowercase. Selecting the encoder sets the destination's
    /// extension.
    pub extension: String,
    /// The encoder takes audio through [`EncoderSession::push_audio`]. When
    /// `false`, the Export dialog hides the Audio row and the export is video
    /// only.
    #[serde(default)]
    pub audio: bool,
}

/// Configuration passed when opening an [`EncoderSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frame rate numerator: `fps_num / fps_den` frames per second.
    pub fps_num: i64,
    /// Frame rate denominator.
    pub fps_den: i32,
    /// Requested video bitrate in kilobits per second, or 0 when the export
    /// requests none.
    pub video_bitrate_kbps: u32,
    /// Requested audio bitrate in kilobits per second. The host passes 0.
    pub audio_bitrate_kbps: u32,
}

/// One RGBA video frame handed to [`EncoderSession::push_frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderFrame<'a> {
    /// Straight-alpha RGBA8 pixels: `height` rows of `stride` bytes, top row
    /// first.
    pub rgba: &'a [u8],
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Bytes per row, at least `width * 4`.
    pub stride: u32,
    /// Presentation time in ticks of `pts_rate`: the frame starts
    /// `pts_value / pts_rate` seconds into the file.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// One interleaved `f32` audio block handed to [`EncoderSession::push_audio`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderAudio<'a> {
    /// Interleaved samples, `channels` per sample frame.
    pub samples: &'a [f32],
    /// Sample frames per second.
    pub sample_rate: u32,
    /// Channels per sample frame.
    pub channels: u32,
    /// Presentation time of the first sample frame in ticks of `pts_rate`.
    pub pts_value: i64,
    /// Ticks per second of `pts_value`.
    pub pts_rate: i32,
}

/// Active media encoding session created by [`Encoder::open`] or [`Plugin::encoder_open`].
pub trait EncoderSession: Send {
    /// Encode one RGBA video frame.
    fn push_frame(&mut self, frame: &EncoderFrame<'_>) -> Result<(), String>;

    /// Encode one interleaved `f32` audio block.
    fn push_audio(&mut self, _audio: &EncoderAudio<'_>) -> Result<(), String> {
        Ok(())
    }

    /// Finalize the output container and flush all pending bytes to disk.
    fn finish(self: Box<Self>) -> Result<(), String>;
}

/// Media encoder factory registered with [`Host::register_encoder`].
pub trait Encoder: Send + Sync + 'static {
    /// Open an [`EncoderSession`] writing to `path` with `config`.
    fn open(
        &self,
        host: &Host,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn EncoderSession>, String>;
}

impl<F> Encoder for F
where
    F: Fn(&Host, &Path, &EncoderConfig) -> Result<Box<dyn EncoderSession>, String>
        + Send
        + Sync
        + 'static,
{
    fn open(
        &self,
        host: &Host,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn EncoderSession>, String> {
        self(host, path, config)
    }
}

/// Result returned by [`Plugin::import`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResult {
    /// Serde JSON of `Vec<aria_commands::Command>`, applied by the host as one transaction.
    pub commands_json: String,
    /// Items in the source file that the importer skipped.
    #[serde(default)]
    pub skipped: Vec<String>,
}

impl ImportResult {
    /// Build an import result from serialized commands JSON and skipped lines.
    pub fn new(commands_json: impl Into<String>, skipped: Vec<String>) -> Self {
        Self {
            commands_json: commands_json.into(),
            skipped,
        }
    }
}

/// Report returned by [`Plugin::export`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReport {
    /// Items in the project that the exported format could not represent.
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// Scalar parameter specification for an audio effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioParamSpec {
    /// Parameter key, unique within the effect.
    pub name: String,
    /// The parameter's row label.
    pub label: String,
    /// Value of a new insert, inside `min..=max`.
    pub default: f32,
    /// Lowest value.
    pub min: f32,
    /// Highest value, not below `min`.
    pub max: f32,
}

impl AudioParamSpec {
    /// Define a scalar parameter with default and inclusive `[min, max]` range.
    pub fn scalar(
        name: impl Into<String>,
        label: impl Into<String>,
        default: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            default,
            min,
            max,
        }
    }
}

/// Specification of an audio effect registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEffectSpec {
    /// Namespaced id with at least one `.`, such as `com.example.drive`. A
    /// shipped id or an id already registered is refused.
    pub id: String,
    /// The name the Effect menu and the mixer inserts list. Not empty.
    pub label: String,
    /// The Effect submenu and Effects browser group the effect is listed
    /// under. Empty lists it under Other Audio.
    #[serde(default)]
    pub category: String,
    /// Scalar parameters in slot order: the order the processor receives their
    /// values.
    #[serde(default)]
    pub params: Vec<AudioParamSpec>,
    /// Sample frames at 48 kHz by which the output trails the input, at most
    /// 48000. The mixer delays every parallel path by the same amount.
    #[serde(default)]
    pub latency: u32,
    /// The effect reads a key signal: a mixer strip shows a Key from row, and
    /// each audio block includes the key lane.
    #[serde(default)]
    pub sidechain: bool,
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

/// Rational frame rate or timebase (`num / den` ticks per second).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    /// Numerator.
    pub num: i64,
    /// Denominator.
    pub den: i64,
}

impl Rational {
    /// 24 per second.
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    /// 25 per second.
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    /// 30 per second.
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    /// 60 per second.
    pub const FPS_60: Self = Self { num: 60, den: 1 };

    /// The rate `num / den` per second.
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }
}

/// Exact timestamp `value / rate` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RationalTime {
    /// Tick count.
    pub value: i64,
    /// Ticks per second.
    pub rate: Rational,
}

impl RationalTime {
    /// The time `value / rate` seconds.
    pub const fn new(value: i64, rate: Rational) -> Self {
        Self { value, rate }
    }
}

/// Half-open time interval `[start, start + duration)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    /// The first tick.
    pub start: RationalTime,
    /// The length.
    pub duration: RationalTime,
}

impl TimeRange {
    /// The range from `start` lasting `duration`.
    pub const fn new(start: RationalTime, duration: RationalTime) -> Self {
        Self { start, duration }
    }
}

/// Summary of a timeline clip returned by [`Host`] project queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRef {
    /// The clip id.
    pub id: u64,
    /// Id of the track holding the clip.
    pub track: u64,
    /// The clip's first tick on the timeline.
    pub start: RationalTime,
    /// The tick after the clip's last frame.
    pub end: RationalTime,
    /// `media`, `sequence`, `multicam`, the generator id of a generated clip,
    /// or the clip type id of a typed clip.
    pub kind: String,
    /// The clip's title. A typed clip with no name of its own reads the type's
    /// name.
    pub name: String,
}

/// Sequence In and Out points returned by [`Host::in_out`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InOut {
    /// The In point, or `None` when unset.
    pub mark_in: Option<RationalTime>,
    /// The Out point, or `None` when unset.
    pub mark_out: Option<RationalTime>,
}

/// Marker category (`Comment` by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// A note.
    #[default]
    Comment,
    /// A chapter point.
    Chapter,
    /// A segment, such as a range an analyzer writes.
    Segmentation,
}

impl MarkerKind {
    /// Whether this is [`Self::Comment`], the kind of a marker whose JSON omits
    /// `kind`.
    pub fn is_default(&self) -> bool {
        *self == Self::Comment
    }
}

fn zero_marker_duration() -> RationalTime {
    RationalTime::new(0, Rational::FPS_24)
}

fn is_zero_marker_duration(duration: &RationalTime) -> bool {
    duration.value == 0
}

/// A ruler marker returned by [`Host::markers_in_range`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// The marker id.
    pub id: u64,
    /// Position on the sequence timeline.
    pub tick: RationalTime,
    /// Span of a range marker, or 0 ticks for a point marker.
    #[serde(
        default = "zero_marker_duration",
        skip_serializing_if = "is_zero_marker_duration"
    )]
    pub duration: RationalTime,
    /// The marker name.
    pub name: String,
    /// The marker colour.
    pub color: MarkerColor,
    /// Comment text, empty when none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// The marker category.
    #[serde(default, skip_serializing_if = "MarkerKind::is_default")]
    pub kind: MarkerKind,
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

/// One parameter signal across an [`AudioBlock`].
#[derive(Debug, Clone, Copy)]
pub struct AudioParam<'a> {
    pub(crate) constant: f32,
    pub(crate) per_frame: Option<&'a [f32]>,
}

impl<'a> AudioParam<'a> {
    /// The constant value for the block, or the first frame's value when automated.
    pub fn constant(&self) -> f32 {
        self.constant
    }

    /// Per-frame values when the parameter is automated across the block.
    pub fn per_frame(&self) -> Option<&'a [f32]> {
        self.per_frame
    }

    /// Parameter value at `frame`.
    pub fn at(&self, frame: usize) -> f32 {
        match self.per_frame {
            Some(values) => values
                .get(frame)
                .or_else(|| values.last())
                .copied()
                .unwrap_or(self.constant),
            None => self.constant,
        }
    }
}

/// One block of planar stereo audio samples handed to [`AudioProcessor::process`].
#[derive(Debug)]
pub struct AudioBlock<'a> {
    pub(crate) left: &'a mut [f32],
    pub(crate) right: &'a mut [f32],
    pub(crate) position: i64,
    pub(crate) sample_rate: u32,
    pub(crate) sidechain: Option<(&'a [f32], &'a [f32])>,
}

impl<'a> AudioBlock<'a> {
    /// Number of sample frames in the block.
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    /// Timeline sample position of the block's first frame.
    pub fn position(&self) -> i64 {
        self.position
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Mutable planar stereo channels `(left, right)`.
    pub fn channels(&mut self) -> (&mut [f32], &mut [f32]) {
        (&mut *self.left, &mut *self.right)
    }

    /// Optional planar stereo sidechain key signal.
    pub fn sidechain(&self) -> Option<(&'a [f32], &'a [f32])> {
        self.sidechain
    }
}

/// Instance of a plugin audio effect processing stereo planar blocks.
pub trait AudioProcessor: Send {
    /// Process `block` in place using `params`.
    fn process(&mut self, params: &[AudioParam<'_>], block: &mut AudioBlock<'_>);

    /// Reset internal state (delay lines, filter state, envelopes) after a seek.
    fn reset(&mut self) {}
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

/// Export a [`Plugin`] implementation from a native `cdylib` crate.
///
/// ```ignore
/// aria_sdk::export!(NotesPlugin);
/// ```
#[macro_export]
macro_rules! export {
    ($plugin_ty:ty) => {
        $crate::export!(
            $plugin_ty,
            <$plugin_ty as ::core::default::Default>::default()
        );
    };
    ($plugin_ty:ty, $init:expr) => {
        const _: () = {
            use ::core::ffi::c_void;
            use ::std::sync::LazyLock;
            use $crate::abi::{
                ARIA_API_MAJOR, AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaCpuEffectCall,
                AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame,
                AriaHostV1, AriaInstanceResult, AriaJobStep, AriaPluginV1, AriaResult,
                AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo, free_rust_bytes,
                free_rust_string,
            };

            static RUNTIME: LazyLock<$crate::PluginRuntime<$plugin_ty>> =
                LazyLock::new(|| $crate::PluginRuntime::new($init));
            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_mount(host_ctx: *mut c_void) -> AriaResult {
                RUNTIME.handle_mount(host_ctx)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_unmount(host_ctx: *mut c_void) -> AriaResult {
                RUNTIME.handle_unmount(host_ctx)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_command_run(
                host_ctx: *mut c_void,
                id: AriaStr,
                args_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_command_run(host_ctx, id, args_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_event(
                host_ctx: *mut c_void,
                kind: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_event(host_ctx, kind, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_import(
                host_ctx: *mut c_void,
                importer_id: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_import(host_ctx, importer_id, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_export(
                host_ctx: *mut c_void,
                exporter_id: AriaStr,
                project_json: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_export(host_ctx, exporter_id, project_json, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_create(
                host_ctx: *mut c_void,
                effect_id: AriaStr,
                sample_rate: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_audio_create(host_ctx, effect_id, sample_rate)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_process(
                host_ctx: *mut c_void,
                instance: u64,
                params: *const AriaAudioParam,
                params_len: usize,
                block: *mut AriaAudioBlock,
            ) -> AriaResult {
                // SAFETY: `params` and `block` are valid for the duration of the FFI call.
                unsafe {
                    RUNTIME.handle_audio_process(host_ctx, instance, params, params_len, block)
                }
            }
            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_reset(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_audio_reset(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_audio_drop(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_panel_view(
                host_ctx: *mut c_void,
                panel_id: AriaStr,
            ) -> AriaBytesResult {
                RUNTIME.handle_panel_view(host_ctx, panel_id)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_panel_event(
                host_ctx: *mut c_void,
                panel_id: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_panel_event(host_ctx, panel_id, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_open(
                host_ctx: *mut c_void,
                encoder_id: AriaStr,
                path: AriaStr,
                config: *const AriaEncoderConfig,
            ) -> AriaInstanceResult {
                // SAFETY: `config` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_open(host_ctx, encoder_id, path, config) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_push_frame(
                host_ctx: *mut c_void,
                instance: u64,
                frame: *const AriaEncoderFrame,
            ) -> AriaResult {
                // SAFETY: `frame` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_push_frame(host_ctx, instance, frame) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_push_audio(
                host_ctx: *mut c_void,
                instance: u64,
                audio: *const AriaEncoderAudio,
            ) -> AriaResult {
                // SAFETY: `audio` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_push_audio(host_ctx, instance, audio) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_finish(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_encoder_finish(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_encoder_drop(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_overlay_scene(
                host_ctx: *mut c_void,
                overlay_id: AriaStr,
            ) -> AriaBytesResult {
                RUNTIME.handle_overlay_scene(host_ctx, overlay_id)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_overlay_event(
                host_ctx: *mut c_void,
                overlay_id: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_overlay_event(host_ctx, overlay_id, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_job_step(
                host_ctx: *mut c_void,
                job: u64,
                step: *mut AriaJobStep,
            ) -> AriaResult {
                // SAFETY: the host passes a writable `step` for the duration of the call.
                unsafe { RUNTIME.handle_job_step(host_ctx, job, step) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_job_end(
                host_ctx: *mut c_void,
                job: u64,
                cancelled: u32,
            ) -> AriaResult {
                RUNTIME.handle_job_end(host_ctx, job, cancelled)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_probe(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_decoder_probe(host_ctx, decoder_id, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_open_video(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
                info: *mut AriaVideoInfo,
            ) -> AriaInstanceResult {
                // SAFETY: the host passes a writable `info` for the duration of the call.
                unsafe { RUNTIME.handle_decoder_open_video(host_ctx, decoder_id, path, info) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_seek(
                host_ctx: *mut c_void,
                instance: u64,
                pts: i64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_seek(host_ctx, instance, pts)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_next_frame(
                host_ctx: *mut c_void,
                instance: u64,
                frame: *mut AriaVideoFrameOut,
            ) -> AriaResult {
                // SAFETY: the host passes a writable `frame` and planes for the duration of the call.
                unsafe { RUNTIME.handle_decoder_next_frame(host_ctx, instance, frame) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_open_audio(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
                sample_rate: u32,
                channels: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_decoder_open_audio(host_ctx, decoder_id, path, sample_rate, channels)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_read_audio(
                host_ctx: *mut c_void,
                instance: u64,
                out: *mut f32,
                out_len: usize,
                written: *mut usize,
            ) -> AriaResult {
                // SAFETY: the host passes `out_len` writable samples and a writable count.
                unsafe {
                    RUNTIME.handle_decoder_read_audio(host_ctx, instance, out, out_len, written)
                }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_seek_audio(
                host_ctx: *mut c_void,
                instance: u64,
                frame: i64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_seek_audio(host_ctx, instance, frame)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_close(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_close(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_ai_tool_call(
                host_ctx: *mut c_void,
                name: AriaStr,
                arguments_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_ai_tool_call(host_ctx, name, arguments_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_scope_measure(
                host_ctx: *mut c_void,
                scope_id: AriaStr,
                call: *mut AriaScopeCall,
            ) -> AriaResult {
                // SAFETY: the host passes a call whose frame and image buffers are valid for the call.
                unsafe { RUNTIME.handle_scope_measure(host_ctx, scope_id, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_cpu_effect_process(
                host_ctx: *mut c_void,
                effect_id: AriaStr,
                call: *mut AriaCpuEffectCall,
            ) -> AriaResult {
                // SAFETY: the host passes a call whose input, output, and params buffers are valid for the call.
                unsafe { RUNTIME.handle_cpu_effect_process(host_ctx, effect_id, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_driver_evaluate(
                host_ctx: *mut c_void,
                language_id: AriaStr,
                expr: AriaStr,
                call: *mut AriaDriverEvalCall,
            ) -> AriaResult {
                // SAFETY: the host passes a valid writable `AriaDriverEvalCall` for the duration of the call.
                unsafe { RUNTIME.handle_driver_evaluate(host_ctx, language_id, expr, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_open(
                host_ctx: *mut c_void,
                analyzer_id: AriaStr,
                sample_rate: u32,
                channels: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_analyzer_open(host_ctx, analyzer_id, sample_rate, channels)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_push(
                host_ctx: *mut c_void,
                instance: u64,
                samples: *const f32,
                samples_len: usize,
            ) -> AriaResult {
                // SAFETY: the host passes a valid `samples` buffer covering `samples_len` elements.
                unsafe { RUNTIME.handle_analyzer_push(host_ctx, instance, samples, samples_len) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_finish(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_analyzer_finish(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_analyzer_drop(host_ctx, instance)
            }

            static TABLE: AriaPluginV1 = AriaPluginV1 {
                struct_size: ::core::mem::size_of::<AriaPluginV1>(),
                api_major: ARIA_API_MAJOR,
                reserved: 0,
                free_string: free_rust_string,
                free_bytes: free_rust_bytes,
                mount: plugin_mount,
                unmount: plugin_unmount,
                command_run: plugin_command_run,
                event: plugin_event,
                import_file: plugin_import,
                export_file: plugin_export,
                audio_create: plugin_audio_create,
                audio_process: plugin_audio_process,
                audio_reset: plugin_audio_reset,
                audio_drop: plugin_audio_drop,
                panel_view: plugin_panel_view,
                panel_event: plugin_panel_event,
                encoder_open: plugin_encoder_open,
                encoder_push_frame: plugin_encoder_push_frame,
                encoder_push_audio: plugin_encoder_push_audio,
                encoder_finish: plugin_encoder_finish,
                encoder_drop: plugin_encoder_drop,
                overlay_scene: plugin_overlay_scene,
                overlay_event: plugin_overlay_event,
                job_step: plugin_job_step,
                job_end: plugin_job_end,
                decoder_probe: plugin_decoder_probe,
                decoder_open_video: plugin_decoder_open_video,
                decoder_seek: plugin_decoder_seek,
                decoder_next_frame: plugin_decoder_next_frame,
                decoder_open_audio: plugin_decoder_open_audio,
                decoder_read_audio: plugin_decoder_read_audio,
                decoder_seek_audio: plugin_decoder_seek_audio,
                decoder_close: plugin_decoder_close,
                ai_tool_call: plugin_ai_tool_call,
                scope_measure: plugin_scope_measure,
                cpu_effect_process: plugin_cpu_effect_process,
                driver_evaluate: plugin_driver_evaluate,
                analyzer_open: plugin_analyzer_open,
                analyzer_push: plugin_analyzer_push,
                analyzer_finish: plugin_analyzer_finish,
                analyzer_drop: plugin_analyzer_drop,
            };

            #[allow(unsafe_code)]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn aria_plugin_entry(
                host: *const AriaHostV1,
                api_major: u32,
            ) -> *const AriaPluginV1 {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    // SAFETY: `host` is checked for null and size inside `bind_host`.
                    unsafe {
                        RUNTIME.bind_host(host, api_major);
                    }
                }));
                &TABLE
            }
        };
    };
}
