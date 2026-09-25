//! Where a panel comes from.
//!
//! The shell used to name its panel modules directly, so a plugin could declare a
//! `UiPanelService` and nothing would draw it. Panels are entries in this registry
//! instead: the built-ins register through the same call a plugin makes, and the
//! shell asks by location.
//!
//! `aria-plugin` stays GPUI-free, so the trait it owns carries only the metadata
//! (id, title, location) and the drawing lives here as a function pointer. A
//! contributor registers both at once with [`PanelHost::add`].

use std::sync::Arc;

use aria_plugin::{PanelLocation, PluginContext, Service, UiPanelService};
use gpui::{AnyElement, Entity};
use parking_lot::RwLock;

use crate::app::AppState;
use crate::shell::Shell;

/// How a panel draws itself. A plain function pointer, not a closure: a panel is
/// stateless with respect to the registry, and its state lives in `AppState` like
/// every other panel's.
pub type PanelRender = fn(&Entity<AppState>, &mut gpui::Context<Shell>) -> AnyElement;

/// Whether a panel is on screen right now, read from state.
///
/// A docked pane is always on; a body is not. The preview and the assistant share
/// the viewer, and Home replaces the editor, so which body draws is a fact about
/// state — and keeping it on the entry is what stops the shell from naming the
/// modules again in an `if`.
pub type PanelVisible = fn(&AppState) -> bool;

/// A panel with no condition: every docked pane.
pub const ALWAYS: PanelVisible = |_| true;

/// One contributed panel.
#[derive(Clone, Copy)]
pub struct PanelEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub location: PanelLocation,
    pub render: PanelRender,
    pub visible: PanelVisible,
}

impl PanelEntry {
    /// Reads the metadata off a plugin's own service, so the id and title a plugin
    /// declares are the ones the shell shows.
    pub fn from_service(service: &dyn UiPanelService, render: PanelRender) -> Self {
        Self {
            id: service.panel_id(),
            title: service.panel_title(),
            location: service.panel_location(),
            render,
            visible: ALWAYS,
        }
    }
}

/// Every panel the shell can draw, in registration order.
#[derive(Default)]
pub struct PanelHost {
    entries: RwLock<Vec<PanelEntry>>,
    by_location: RwLock<Vec<(PanelLocation, Arc<[PanelEntry]>)>>,
}

impl Service for PanelHost {}

impl PanelHost {
    /// Add a panel. A second registration of the same id replaces the first, so a
    /// plugin can override a built-in rather than double it.
    pub fn add(&self, entry: PanelEntry) {
        let mut entries = self.entries.write();
        if let Some(existing) = entries.iter_mut().find(|held| held.id == entry.id) {
            *existing = entry;
        } else {
            entries.push(entry);
        }
        self.by_location.write().clear();
    }

    /// The panels at one location, in registration order. The first is the body of
    /// that slot; later ones are stacked after it.
    pub fn at(&self, location: PanelLocation) -> Vec<(&'static str, PanelRender)> {
        self.cached_at(location)
            .iter()
            .map(|entry| (entry.id, entry.render))
            .collect()
    }

    /// The panels at one location that this state puts on screen.
    pub fn visible_at(&self, location: PanelLocation, state: &AppState) -> Vec<PanelRender> {
        self.cached_at(location)
            .iter()
            .filter(|entry| (entry.visible)(state))
            .map(|entry| entry.render)
            .collect()
    }

    /// The immutable entries for one slot. Registration clears these sets;
    /// drawing clones only the shared slice handle.
    pub(crate) fn cached_at(&self, location: PanelLocation) -> Arc<[PanelEntry]> {
        if let Some((_, entries)) = self
            .by_location
            .read()
            .iter()
            .find(|(held, _)| *held == location)
        {
            return entries.clone();
        }
        let built: Arc<[PanelEntry]> = self
            .entries
            .read()
            .iter()
            .filter(|entry| entry.location == location)
            .copied()
            .collect();
        let mut cache = self.by_location.write();
        if let Some((_, entries)) = cache.iter().find(|(held, _)| *held == location) {
            return entries.clone();
        }
        cache.push((location, built.clone()));
        built
    }

    /// Every registered id, in order. For the Plugins list and the tests.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.read().iter().map(|entry| entry.id).collect()
    }
}

/// The registry the shell reads, or an empty one when no host mounted it.
///
/// A missing registry is not an error: a headless `AppState` has no panels and
/// nothing asks it to draw any.
pub fn host_of(ctx: &PluginContext) -> Option<std::sync::Arc<PanelHost>> {
    ctx.try_get::<PanelHost>()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn render(_: &Entity<AppState>, _: &mut gpui::Context<Shell>) -> AnyElement {
        panic!("cache tests do not render panels")
    }

    #[test]
    fn resolved_locations_are_shared_until_registration_changes() {
        let host = PanelHost::default();
        host.add(PanelEntry {
            id: "first",
            title: "First",
            location: PanelLocation::Sidebar,
            render,
            visible: ALWAYS,
        });
        let first = host.cached_at(PanelLocation::Sidebar);
        assert!(Arc::ptr_eq(&first, &host.cached_at(PanelLocation::Sidebar)));

        host.add(PanelEntry {
            id: "second",
            title: "Second",
            location: PanelLocation::Sidebar,
            render,
            visible: ALWAYS,
        });
        let rebuilt = host.cached_at(PanelLocation::Sidebar);
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        assert_eq!(rebuilt.len(), 2);
    }
}
