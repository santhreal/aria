//! Registration guard — reversible service and event-handler registration.

use std::any::TypeId;
use std::sync::{Arc, Weak};

use crate::context::ServiceMap;
use crate::events::EventBus;

/// A guard that unregisters a service, an event handler, or a registered effect
/// or transition when dropped.
pub struct Registration {
    kind: RegistrationKind,
}

enum RegistrationKind {
    Service {
        type_id: TypeId,
        /// The registered type's name, so a host can list what a plugin provides
        /// without a second table to keep in step.
        name: &'static str,
        map: Weak<ServiceMap>,
    },
    Handler {
        type_id: TypeId,
        handler_id: usize,
        bus: Weak<EventBus>,
    },
    /// A guard whose own drop removes what it registered: a command, a panel,
    /// an importer, an exporter, an encoder, or a runtime effect, transition,
    /// or audio effect.
    Held {
        contribution: Contribution,
        _guard: Box<dyn std::any::Any + Send + Sync>,
    },
    None,
}

impl Registration {
    pub(crate) fn service(type_id: TypeId, name: &'static str, map: &Arc<ServiceMap>) -> Self {
        Self {
            kind: RegistrationKind::Service {
                type_id,
                name,
                map: Arc::downgrade(map),
            },
        }
    }

    /// The service type this registration holds, or `None` for a handler.
    pub fn service_name(&self) -> Option<&'static str> {
        match &self.kind {
            RegistrationKind::Service { name, .. } => Some(name),
            RegistrationKind::Handler { .. }
            | RegistrationKind::Held { .. }
            | RegistrationKind::None => None,
        }
    }

    /// The contribution this registration holds, or `None` for a service or a
    /// handler.
    pub fn contribution(&self) -> Option<&Contribution> {
        match &self.kind {
            RegistrationKind::Held { contribution, .. } => Some(contribution),
            RegistrationKind::Service { .. }
            | RegistrationKind::Handler { .. }
            | RegistrationKind::None => None,
        }
    }

    pub(crate) fn handler(type_id: TypeId, handler_id: usize, bus: &Arc<EventBus>) -> Self {
        Self {
            kind: RegistrationKind::Handler {
                type_id,
                handler_id,
                bus: Arc::downgrade(bus),
            },
        }
    }

    /// Hold `guard`, which removes `contribution` when it drops, until this
    /// registration drops.
    pub(crate) fn holding(contribution: Contribution, guard: impl Send + Sync + 'static) -> Self {
        Self {
            kind: RegistrationKind::Held {
                contribution,
                _guard: Box::new(guard),
            },
        }
    }

    /// Create a no-op registration. Useful for tests.
    pub fn none() -> Self {
        Self {
            kind: RegistrationKind::None,
        }
    }

    /// Manually unregister without waiting for drop.
    pub fn revoke(self) {
        drop(self);
    }
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            RegistrationKind::Service { name, .. } => write!(f, "Registration::Service({name})"),
            RegistrationKind::Handler { handler_id, .. } => {
                write!(f, "Registration::Handler({handler_id})")
            }
            RegistrationKind::Held { contribution, .. } => {
                write!(f, "Registration::Held({contribution})")
            }
            RegistrationKind::None => f.write_str("Registration::None"),
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        match &self.kind {
            RegistrationKind::Service { type_id, map, .. } => {
                if let Some(map) = map.upgrade() {
                    map.write().remove(type_id);
                }
            }
            RegistrationKind::Handler {
                type_id,
                handler_id,
                bus,
            } => {
                if let Some(bus) = bus.upgrade() {
                    bus.remove_handler(*type_id, *handler_id);
                }
            }
            RegistrationKind::Held { .. } | RegistrationKind::None => {}
        }
    }
}

/// What a plugin added through one registration, for a host to list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    pub kind: ContributionKind,
    /// The id it is registered under.
    pub id: String,
}

impl Contribution {
    pub(crate) fn new(kind: ContributionKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

impl std::fmt::Display for Contribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.kind.label(), self.id)
    }
}

/// The kinds of registration a host lists per plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContributionKind {
    Command,
    /// A menu row that runs a command.
    Menu,
    /// A command's default chord.
    Key,
    /// A section of fields in Settings.
    Settings,
    Panel,
    /// Drawing and handles over the Viewer frame.
    Overlay,
    Importer,
    Exporter,
    Encoder,
    /// A media format read for import and playback.
    Decoder,
    /// A language property drivers are written in.
    DriverLanguage,
    Effect,
    Transition,
    AudioEffect,
    /// An audio analyzer that inspects mixed blocks and writes findings.
    Analyzer,
    Preset,
    /// A theme file listed in Settings.
    Theme,
    /// A tool the assistant can call.
    AiTool,
    /// A measurement in the scope strip.
    Scope,
    /// A named arrangement of the editor's panes.
    Workspace,
    /// A kind of clip a generator draws, placed from the Generators tab.
    ClipType,
    /// An RGBA image effect passes read by id.
    Texture,
}

impl ContributionKind {
    /// Every kind, in the order a host lists them.
    pub const ALL: [Self; 22] = [
        Self::Command,
        Self::Menu,
        Self::Key,
        Self::Settings,
        Self::Panel,
        Self::Overlay,
        Self::Importer,
        Self::Exporter,
        Self::Encoder,
        Self::Decoder,
        Self::DriverLanguage,
        Self::Effect,
        Self::Transition,
        Self::AudioEffect,
        Self::Analyzer,
        Self::Preset,
        Self::Theme,
        Self::AiTool,
        Self::Scope,
        Self::Workspace,
        Self::ClipType,
        Self::Texture,
    ];

    /// The kind in lowercase words, such as `audio effect`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Menu => "menu item",
            Self::Key => "key",
            Self::Settings => "settings",
            Self::Panel => "panel",
            Self::Overlay => "viewer overlay",
            Self::Importer => "importer",
            Self::Exporter => "exporter",
            Self::Encoder => "encoder",
            Self::Decoder => "decoder",
            Self::DriverLanguage => "driver language",
            Self::Effect => "effect",
            Self::Transition => "transition",
            Self::AudioEffect => "audio effect",
            Self::Analyzer => "analyzer",
            Self::Preset => "preset",
            Self::Theme => "theme",
            Self::AiTool => "AI tool",
            Self::Scope => "scope",
            Self::Workspace => "workspace",
            Self::ClipType => "clip type",
            Self::Texture => "texture",
        }
    }
}
