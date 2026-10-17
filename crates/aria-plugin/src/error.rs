//! Plugin errors.

use std::borrow::Cow;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin {0} failed to mount: {1}")]
    MountFailed(Cow<'static, str>, String),
    #[error("missing dependency: {0} requires {1}")]
    MissingDependency(Cow<'static, str>, String),
    #[error("service conflict: {0}")]
    ServiceConflict(String),
    #[error("plugin {0} failed to unmount: {1}")]
    UnmountFailed(Cow<'static, str>, String),
    /// The plugins in the cycle, in the order they were declared, so the message
    /// names the loop rather than a single arbitrary member of it.
    #[error("dependency cycle: {0}")]
    DependencyCycle(String),
    /// An effect or transition definition the host did not register: its WGSL
    /// did not check, or its catalog spec was refused.
    #[error("registration refused: {0}")]
    RegisterRefused(#[from] aria_compositor::RegisterError),
    /// An audio effect the host did not register: the catalog refused its
    /// spec, or a processor is already registered under its id.
    #[error("audio effect refused: {0}")]
    AudioEffectRefused(String),
    /// An audio analyzer the host did not register: its id is malformed or
    /// already registered, or its label is empty.
    #[error("analyzer refused: {0}")]
    AnalyzerRefused(String),
    /// An importer or exporter the host did not register: its id is empty or
    /// taken, or an extension is malformed.
    #[error("importer or exporter refused: {0}")]
    InterchangeRefused(String),
    /// An encoder the host did not register: its id is empty or taken, or its
    /// extension is malformed.
    #[error("encoder refused: {0}")]
    EncoderRefused(String),
    /// A decoder the host did not register: its id is empty or taken, or an
    /// extension is malformed or missing.
    #[error("decoder refused: {0}")]
    DecoderRefused(String),
    /// A driver language the host did not register: its id is malformed or
    /// taken, or its name is empty.
    #[error("driver language refused: {0}")]
    DriverLanguageRefused(String),
    /// A job the host did not start: no job registry is mounted.
    #[error("job refused: {0}")]
    JobRefused(String),
    /// A command, menu entry, or key default the host did not register: an id
    /// or title is empty, the id is taken, or the place is already held.
    #[error("command refused: {0}")]
    CommandRefused(String),
    /// A settings section the host did not register: a field cannot be drawn,
    /// a key is outside the plugin's id or held by another section.
    #[error("settings refused: {0}")]
    SettingsRefused(String),
    /// A panel the host did not register: its id or title is empty, its id is
    /// taken, or its location is not a docked pane.
    #[error("panel refused: {0}")]
    PanelRefused(String),
    /// A viewer overlay the host did not register: its id or title is empty,
    /// or its id is taken.
    #[error("viewer overlay refused: {0}")]
    OverlayRefused(String),
    /// An animation preset the host did not register: its definition failed
    /// validation, or a preset with the same name is already registered by this
    /// plugin.
    #[error("preset refused: {0}")]
    PresetRefused(String),
    /// A theme the host did not register: its id is malformed or already
    /// registered by this plugin, or its text is too long.
    #[error("theme refused: {0}")]
    ThemeRefused(String),
    /// An AI tool the host did not register: its name is malformed or taken,
    /// its description is empty, or its schema is not an object schema.
    #[error("AI tool refused: {0}")]
    AiToolRefused(String),
    /// A scope the host did not register: its id is malformed or taken, its
    /// name is empty, or a side of its image is out of range.
    #[error("scope refused: {0}")]
    ScopeRefused(String),
    /// A workspace the host did not register: its id is malformed or taken by
    /// the same plugin, its name is blank or too long, or a pane size is out
    /// of range.
    #[error("workspace refused: {0}")]
    WorkspaceRefused(String),
    /// A clip type the catalog did not list: its id is malformed or taken, its
    /// name is blank or too long, it names no generator, a handle is empty or
    /// repeated, or its icon is not an SVG the host reads.
    #[error("clip type refused: {0}")]
    ClipTypeRefused(String),
    /// A plugin texture the host did not register: its id is malformed or
    /// taken, a side is zero or above `MAX_TEXTURE_SIDE`, or the pixel buffer
    /// is not `width * height * 4` bytes.
    #[error("texture refused: {0}")]
    TextureRefused(#[from] aria_compositor::TextureError),
}
