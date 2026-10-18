//! Aria plugin framework: the host every plugin registers into.
//!
//! A [`PluginContext`] is a typed service registry plus an event bus. Every
//! capability is a plugin that registers services and event handlers into the
//! context. The project is read and edited through one [`ProjectHost`], which
//! the editor and every plugin share: built-in plugins hold no edit path that
//! an external plugin lacks.

mod activation;
mod ai_tools;
mod analyzers;
mod audio_host;
mod clip_types;
mod command_icon;
mod commands;
mod context;
mod cost;
mod decoders;
mod drivers;
mod editor_events;
mod enablement;
mod error;
mod events;
mod expressions;
pub mod external;
mod foreign_decoder;
mod hooks;
mod interchange;
mod jobs;
pub mod keys;
mod listed;
mod live;
mod menus;
mod order;
pub mod package;
mod panels;
mod plugin;
mod plugin_log;
mod presets;
mod project_host;
mod queries;
mod registration;
mod render_host;
mod scopes;
mod service;
pub mod settings_schema;
mod themes;
mod ui_tree;
pub mod viewer_overlay;
mod workspaces;

pub use activation::{Activated, Activation, ActivationKind, WaitingPanel, WaitingPlugin};
pub use ai_tools::{
    AI_TOOL_PREFIX, AiTool, AiToolError, AiToolImage, AiToolOutput, AiToolSpec, AiToolTier,
    MAX_AI_TOOL_IMAGE_SIDE, MAX_AI_TOOL_NAME, MAX_AI_TOOL_SPEC, PluginAiTool,
};
pub use audio_host::{AUDIO_HOST_BLOCK, AudioHost, AudioSource};
pub use command_icon::{CommandIcon, ICON_MAX_BYTES, ICON_MAX_NODES};
pub use commands::{CommandError, CommandSpec, PluginCommand};
pub use context::{
    CapabilityDenial, MountFailure, MountedPluginInfo, PluginContext, PluginFault, WeakContext,
    short_service_name,
};
pub use cost::{CostMetric, PluginCost};
pub use editor_events::{
    ContextMenuRequested, EditorView, ExportFinished, ExportStarted, InOutChanged, JobFinished,
    JobOutcome, JobProgress, MediaImported, MediaOffline, MediaRelinked, MenuEntry, MenuTarget,
    PlaybackStarted, PlaybackStopped, PlayheadMoved, PluginActivated, PluginDeactivated,
    ProjectClosed, ProjectOpened, ProjectSaved, RenderFinished, RenderKind, RenderResult,
    SelectionChanged, ViewFrame, ViewState,
};
pub use enablement::{
    ContextKey, ContextKeys, ENABLEMENT_MAX_DEPTH, ENABLEMENT_MAX_LEN, Enablement,
};
pub use error::PluginError;
pub use events::{Event, EventBus, SerialEvent, WaterfallEvent, WaterfallResult};
pub use hooks::{CommandProposed, DropAction, Dropped, ResolveDropped};
pub use interchange::{
    Encoder, EncoderSpec, Exporter, ExporterSpec, Importer, ImporterSpec, InterchangeError,
    InterchangeReport,
};
pub use keys::{Chord, KeyDefault, KeyRoute};
pub use menus::{MenuCommand, MenuLocation, MenuPath};
pub use order::load_order;
pub use panels::{DOCKED_LOCATIONS, PanelError, PanelSpec, PanelState, PanelView};
pub use plugin::{Names, Plugin, PluginManifest};
pub use plugin_log::{
    LOG_LINE_MAX_BYTES, LOG_LINE_TRUNCATED, LOG_LINES_PER_PLUGIN, LogLevel, LogLine,
};
pub use presets::PluginPreset;
pub use project_host::{
    CommandApplied, EditGroup, Origin, ProjectHost, Receipt, TransactError, Transaction,
};
pub use queries::{
    ClipRef, InOut, clip, clips_at, clips_in_range, in_out, markers_in_range, selected_clips,
};
pub use registration::{Contribution, ContributionKind, Registration};
pub use render_host::{FrameRequest, FrameSize, FrameSource, RenderHost};
pub use scopes::{
    MAX_SCOPE_READOUT, MAX_SCOPE_SIDE, PluginScope, Scope, ScopeError, ScopeFrame, ScopeImage,
    ScopeSpec,
};
pub use service::{
    DecoderService, EffectService, EncoderService, PanelLocation, Service, SettingsService,
    StorageService, TransitionService, UiPanelService,
};
pub use settings_schema::{SettingKind, SettingSpec, SettingsSchema, SettingsSection};
pub use themes::{PluginTheme, THEME_ID_MAX_BYTES, THEME_MAX_BYTES};
pub use ui_tree::{
    EventMismatch, MAX_CHOICE_OPTIONS, MAX_IMAGE_SIDE, MAX_TEXT_BYTES, MAX_TREE_DEPTH,
    MAX_TREE_IMAGE_BYTES, MAX_TREE_NODES, MAX_TREE_TEXT_BYTES, MAX_WIDGET_ID_BYTES, PanelEvent,
    Tone, TreeError, UiNode, snap,
};
pub use viewer_overlay::{
    DragPhase, HandleLook, OverlayColor, OverlayError, OverlayEvent, OverlayScene, OverlayShape,
    OverlaySpec, OverlayState, SceneError, ViewerOverlay,
};
pub use workspaces::{
    PluginWorkspace, WORKSPACE_ID_MAX_BYTES, WORKSPACE_MAX_SIZE, WORKSPACE_NAME_MAX_BYTES,
    WorkspaceFocus, WorkspacePanes, WorkspaceSpec,
};

/// Re-export core types plugins commonly need.
pub use aria_core::{
    ColorSpace, DecodeError, EncodeError, EncoderChoice, EncoderConfig, FrameData, PixelFormat,
    RationalTime, RgbaImage, SourceInfo, TimeRange, VideoDecoder, VideoEncoder, VideoFrame,
};

/// What a decoder plugin implements and returns.
pub use aria_core::{AudioReader, Plane, Rational};
pub use aria_decoder::{ExternalDecoder, MediaProbe};
pub use decoders::{DecoderSpec, PluginDecoder};
pub use drivers::{DriverLanguageSpec, PluginDriverLanguage};
pub use expressions::{EXPRESSIONS_LANGUAGE, Expressions, ExpressionsPlugin};

/// What a driver language plugin implements and reads.
pub use aria_models::{ClipParam, DriverCode, ParamValue};
pub use aria_render::{
    DriverClipInfo, DriverFault, DriverInput, DriverLanguage, DriverProgram, DriverScope,
    InputClip, InputProperty,
};
pub use foreign_decoder::{
    CallError, DecodedPlanes, DecoderCalls, ForeignDecoder, MAX_FOREIGN_FRAME_SIDE, StreamInfo,
};
pub use jobs::JobStep;

/// What a plugin's background job reads and returns.
pub use aria_jobs::{JobContext, JobId, JobManager, JobSnapshot, JobState};

/// The command and project types a plugin inspects and proposes. Re-exported so
/// an external plugin needs one dependency, not several.
pub use aria_commands::{Command, EditCommand, EditTargets};
pub use aria_engine::{ApplyOutcome, EngineError};
pub use aria_models::{ClipId, Marker, MarkerId, Project, SequenceId, TrackId};
pub use aria_models::{ClipTypeIcon, ClipTypeSpec, MarkerColor};
pub use aria_models::{
    PRESET_FILE_EXTENSION, PRESET_MAX_BYTES, PRESET_VERSION, Preset, PresetParts, PresetProperty,
};
pub use clip_types::{ClipRender, ClipTypeDefinition, ClipTypeRender};

/// What the render host takes and returns: the device a frame texture is
/// created on, the `wgpu` it belongs to, and the render failure.
pub use aria_compositor::{CompositorError, GpuContext, wgpu};
pub use aria_render::RenderError;

/// What a plugin registers to add an effect or a transition: a model spec for
/// the inspector and the timeline, and the WGSL passes or CPU callback the
/// compositor draws it with.
pub use aria_compositor::{
    CpuEffect, CpuEffectDefinition, CpuFrameIn, CpuFrameOut, EffectDefinition, EffectFault,
    EffectFaultKind, EffectParams, EffectRender, F16_ONE, MAX_EFFECT_FLOATS, MAX_EFFECT_PASSES,
    MAX_TEXTURE_SIDE, RegisterError, RegisteredPass, TextureError, TransitionDefinition, U8_TO_F16,
    f16_to_f32, f16_to_u8, f32_to_f16, registered_effect,
};
pub use aria_models::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectBounds, EffectChoiceOption,
    EffectCurvePoint, EffectGradientStop, EffectGroupSpec, EffectParamDisplay, EffectParamKind,
    EffectParamSpec, EffectSpec, EffectVisibilityCondition, GraphPass, MAX_GRAPH_PASSES,
    MAX_PASS_INPUTS, MAX_PASS_SCALE, MAX_PASS_SIDE, MAX_WORKGROUP_INVOCATIONS, OutputSize,
    PassInput, PassStage, TransitionSpec, evaluate_effect_curve_lut, evaluate_effect_gradient_lut,
    sample_effect_curve, sample_effect_gradient,
};

/// What a plugin registers to add an audio effect: a model spec for the
/// inspector and the mixer, and the processor factory that runs it.
pub use aria_models::{AudioEffectSpec, MAX_AUDIO_LATENCY};
pub use aria_render::{
    AudioBlock, AudioProcessor, AudioProcessorFactory, EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE,
    FaultKind, ParamSignal, ProcessorFault,
};

/// What a plugin registers to add an audio analyzer: a spec for the menus and
/// palette, the block-fed analyzer runner, and the shipped detectors.
pub use analyzers::{
    Analyzer, AnalyzerError, AnalyzerLoudness, AnalyzerMarker, AnalyzerRange, AnalyzerResult,
    AnalyzerRun, AnalyzerScope, AnalyzerSpec, BeatAnalyzer, LoudnessAnalyzer, RegisteredAnalyzer,
    SHIPPED_BEAT_ANALYZER_ID, SHIPPED_LOUDNESS_ANALYZER_ID, SHIPPED_SILENCE_ANALYZER_ID,
    SilenceAnalyzer, apply_analyzer_result, frame_to_tick, register_shipped_analyzers,
};
