//! Registration guard — reversible service and event-handler registration.

use std::any::TypeId;
use std::sync::{Arc, Weak};

use crate::context::ServiceMap;
use crate::events::EventBus;

/// A guard that unregisters a service or event handler when dropped.
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
            RegistrationKind::Handler { .. } | RegistrationKind::None => None,
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
            RegistrationKind::None => {}
        }
    }
}
