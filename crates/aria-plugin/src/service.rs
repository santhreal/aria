//! Service trait definitions — the interfaces plugins implement and register.

use std::path::Path;

use aria_compositor::{GpuContext, PassDescriptor};
use aria_core::{
    DecodeError, EncodeError, EncoderConfig, GpuSurface, RationalTime, RgbaImage, VideoDecoder,
    VideoEncoder,
};
use aria_models::Project;
use aria_render::RenderError;

use crate::error::PluginError;

/// Marker trait for all plugin services.
pub trait Service: Send + Sync {}

/// Platform-native video decoding.
pub trait DecoderService: Service {
    fn can_decode(&self, path: &Path) -> bool;
    fn open(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError>;
}

/// Platform-native video encoding.
pub trait EncoderService: Service {
    fn can_encode(&self, config: &EncoderConfig) -> bool;
    fn create(&self, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncodeError>;
}

/// GPU effect catalog.
pub trait EffectService: Service {
    fn effect_descriptors(&self) -> &[PassDescriptor];
    fn render_effect(
        &self,
        id: &str,
        params: &[f32],
        input: &GpuSurface,
        gpu: &GpuContext,
    ) -> Result<GpuSurface, RenderError>;
}

/// GPU transition catalog.
pub trait TransitionService: Service {
    fn transition_descriptors(&self) -> &[PassDescriptor];
    fn render_transition(
        &self,
        id: &str,
        params: &[f32],
        a: &GpuSurface,
        b: &GpuSurface,
        t: f32,
        gpu: &GpuContext,
    ) -> Result<GpuSurface, RenderError>;
}

/// Content generators: text, shapes, stickers, solids.
pub trait GeneratorService: Service {
    fn generator_kinds(&self) -> &[&str];
    fn render(&self, kind: &str, params: &[(String, String)]) -> Result<RgbaImage, RenderError>;
}

// Command interception is an event, not a service: the registry holds one value
// per type, so a service could only ever have one interceptor. See
// `hooks::CommandProposed`.

/// Intercept frames during rendering.
pub trait RenderInterceptorService: Service {
    fn before_frame(&self, time: RationalTime, project: &Project) -> bool;
    fn after_frame(&self, frame: &mut RgbaImage);
}

/// Project and cache directories.
pub trait StorageService: Service {
    fn cache_dir(&self) -> &Path;
    fn project_dir(&self) -> &Path;
}

/// User settings (`~/.aria/config.toml`).
pub trait SettingsService: Service {
    fn get(&self, key: &str) -> Option<serde_json::Value>;
    fn set(&self, key: &str, value: serde_json::Value);
    fn save(&self) -> Result<(), PluginError>;
}

/// Where a contributed panel lives in the shell.
///
/// `Viewer` and `Home` hold a whole body rather than a docked pane, so more than
/// one panel is registered there and which of them draws depends on state: the
/// assistant replaces the preview, and the Home route replaces the editor. The
/// host decides that with the entry's own predicate, so the shell names none of
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelLocation {
    Inspector,
    Sidebar,
    Overlay,
    Timeline,
    /// The area above the timeline: the preview, or the assistant.
    Viewer,
    /// The whole window on the Home route.
    Home,
}

/// A UI panel contributed by a plugin.
pub trait UiPanelService: Service {
    /// Static because the registry holds the metadata for the life of the mount;
    /// a panel's title is a name, not a value that changes per frame.
    fn panel_id(&self) -> &'static str;
    fn panel_title(&self) -> &'static str;
    fn panel_location(&self) -> PanelLocation;
}
