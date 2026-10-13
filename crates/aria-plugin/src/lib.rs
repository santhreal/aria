//! Aria plugin framework: the host every plugin registers into.
//!
//! A [`PluginContext`] is a typed service registry plus an event bus. Every
//! capability is a plugin that registers services and event handlers into the
//! context. The project is read and edited through one [`ProjectHost`], which
//! the editor and every plugin share: built-in plugins hold no edit path that
//! an external plugin lacks.

mod context;
mod editor_events;
mod error;
mod events;
pub mod external;
mod hooks;
mod order;
mod plugin;
mod project_host;
mod registration;
mod service;
pub mod settings_schema;

pub use context::{
    CapabilityDenial, MountFailure, MountedPluginInfo, PluginContext, short_service_name,
};
pub use editor_events::{
    ContextMenuRequested, EditorView, ExportFinished, ExportStarted, InOutChanged, JobFinished,
    JobOutcome, JobProgress, MediaImported, MediaOffline, MediaRelinked, MenuEntry, MenuTarget,
    PlaybackStarted, PlaybackStopped, PlayheadMoved, PluginActivated, PluginDeactivated,
    ProjectClosed, ProjectOpened, ProjectSaved, RenderFinished, RenderKind, RenderResult,
    SelectionChanged, ViewFrame, ViewState,
};
pub use error::PluginError;
pub use events::{Event, EventBus, SerialEvent, WaterfallEvent, WaterfallResult};
pub use hooks::{CommandProposed, DropAction, ResolveDroppedPath};
pub use project_host::{
    CommandApplied, EditGroup, Origin, ProjectHost, Receipt, TransactError, Transaction,
};
pub use order::load_order;
pub use plugin::{Plugin, PluginManifest};
pub use registration::Registration;
pub use service::{
    DecoderService, EffectService, EncoderService, GeneratorService, PanelLocation,
    RenderInterceptorService, Service, SettingsService, StorageService, TransitionService,
    UiPanelService,
};
pub use settings_schema::{SettingSpec, SettingsSchema, SettingsSection};

/// Re-export core types plugins commonly need.
pub use aria_core::{
    ColorSpace, DecodeError, EncodeError, EncoderConfig, PixelFormat, RationalTime, RgbaImage,
    SourceInfo, VideoDecoder, VideoEncoder,
};

/// The command and project types a plugin inspects and proposes. Re-exported so
/// an external plugin needs one dependency, not several.
pub use aria_commands::{Command, EditCommand, EditTargets};
pub use aria_engine::{ApplyOutcome, EngineError};
pub use aria_models::Project;

/// What a plugin registers to add an effect or a transition: a model spec for
/// the inspector and the timeline, and the WGSL the compositor draws it with.
pub use aria_compositor::{
    EffectDefinition, MAX_EFFECT_FLOATS, MAX_EFFECT_PASSES, RegisterError, TransitionDefinition,
};
pub use aria_models::{EffectParamKind, EffectParamSpec, EffectSpec, TransitionSpec};
