//! Aria plugin framework: the host every plugin registers into.
//!
//! There is no privileged core. The "core" is a [`PluginContext`] — a typed
//! service registry plus an event bus. Every capability is a plugin that
//! registers services and event handlers into the context.

mod context;
mod error;
mod events;
pub mod external;
mod hooks;
mod order;
mod plugin;
mod registration;
mod service;
pub mod settings_schema;

pub use context::{
    CapabilityDenial, MountFailure, MountedPluginInfo, PluginContext, short_service_name,
};
pub use error::PluginError;
pub use events::{Event, EventBus, SerialEvent, WaterfallEvent, WaterfallResult};
pub use hooks::{CommandProposed, DropAction, ResolveDroppedPath};
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

/// The command types a `CommandProposed` handler inspects. Re-exported so an
/// external plugin needs one dependency, not two.
pub use aria_commands::{Command, EditCommand, EditTargets};
