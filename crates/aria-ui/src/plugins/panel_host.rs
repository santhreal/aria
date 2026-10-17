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
use crate::dock::DefaultFrame;
use crate::shell::Shell;
use crate::types::ActivePanel;

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

/// How the panels plugins declare as `UiNode` trees draw at one location, in
/// the order the plugins registered them.
pub type DeclaredRender =
    fn(PanelLocation, &Entity<AppState>, &mut gpui::Context<Shell>) -> Vec<AnyElement>;

/// How a single declared or waiting plugin panel draws by id.
pub type DeclaredOneRender =
    fn(&str, &Entity<AppState>, &mut gpui::Context<Shell>) -> Option<AnyElement>;

/// Default frame, minimum size, and keyboard focus route of a dockable panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockSpec {
    pub default_frame: DefaultFrame,
    pub min_size: (f32, f32),
    pub focus: ActivePanel,
}

impl DockSpec {
    pub const DEFAULT_MIN_SIZE: (f32, f32) = (160.0, 80.0);
    pub const VIEWER_MIN_SIZE: (f32, f32) = (320.0, 180.0);
    pub const TIMELINE_MIN_SIZE: (f32, f32) = (400.0, 120.0);

    pub const fn new(
        default_frame: DefaultFrame,
        min_size: (f32, f32),
        focus: ActivePanel,
    ) -> Self {
        Self {
            default_frame,
            min_size,
            focus,
        }
    }

    pub const fn for_location(location: PanelLocation) -> Option<Self> {
        match location {
            PanelLocation::Sidebar => Some(Self::new(
                DefaultFrame::Left,
                Self::DEFAULT_MIN_SIZE,
                ActivePanel::Media,
            )),
            PanelLocation::Viewer => Some(Self::new(
                DefaultFrame::Center,
                Self::VIEWER_MIN_SIZE,
                ActivePanel::Preview,
            )),
            PanelLocation::Inspector => Some(Self::new(
                DefaultFrame::Right,
                Self::DEFAULT_MIN_SIZE,
                ActivePanel::Inspector,
            )),
            PanelLocation::Timeline => Some(Self::new(
                DefaultFrame::Bottom,
                Self::TIMELINE_MIN_SIZE,
                ActivePanel::Timeline,
            )),
            PanelLocation::Overlay | PanelLocation::Home => None,
        }
    }
}

/// One contributed panel.
#[derive(Clone, Copy)]
pub struct PanelEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub location: PanelLocation,
    pub render: PanelRender,
    pub visible: PanelVisible,
    /// When `Some`, this entry is a dockable tab in the frame tree; when `None`,
    /// it is a route body (`Home`) or a conditional sub-entry stacked inside
    /// its `location`'s primary dock panel.
    pub dock: Option<DockSpec>,
}

impl PanelEntry {
    /// Reads the metadata off a plugin's own service, so the id and title a plugin
    /// declares are the ones the shell shows.
    pub fn from_service(service: &dyn UiPanelService, render: PanelRender) -> Self {
        let location = service.panel_location();
        Self {
            id: service.panel_id(),
            title: service.panel_title(),
            location,
            render,
            visible: ALWAYS,
            dock: DockSpec::for_location(location),
        }
    }
}

impl std::fmt::Debug for PanelEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelEntry")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("location", &self.location)
            .field("dock", &self.dock)
            .finish_non_exhaustive()
    }
}

/// Every panel the shell can draw, in registration order.
#[derive(Default)]
pub struct PanelHost {
    entries: RwLock<Vec<PanelEntry>>,
    by_location: RwLock<Vec<(PanelLocation, Arc<[PanelEntry]>)>>,
    declared: RwLock<Option<DeclaredRender>>,
    declared_one: RwLock<Option<DeclaredOneRender>>,
}

impl std::fmt::Debug for PanelHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelHost")
            .field("len", &self.entries.read().len())
            .finish_non_exhaustive()
    }
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

    /// Look up one registered panel by id.
    pub fn get(&self, id: &str) -> Option<PanelEntry> {
        self.entries
            .read()
            .iter()
            .find(|entry| entry.id == id)
            .copied()
    }

    /// Every dockable tab panel in registration order.
    pub fn dock_panels(&self) -> Vec<PanelEntry> {
        self.entries
            .read()
            .iter()
            .filter(|entry| entry.dock.is_some())
            .copied()
            .collect()
    }

    /// Visible stacked sub-entries (`dock.is_none()`) at `location`.
    pub fn sub_entries_at(&self, location: PanelLocation, state: &AppState) -> Vec<PanelRender> {
        self.cached_at(location)
            .iter()
            .filter(|entry| entry.dock.is_none() && (entry.visible)(state))
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

    /// Set how declared plugin panels draw. They follow the registered entries
    /// in every slot.
    pub fn set_declared(&self, render: DeclaredRender) {
        *self.declared.write() = Some(render);
    }

    /// How declared plugin panels draw, when a mount set it.
    pub fn declared(&self) -> Option<DeclaredRender> {
        *self.declared.read()
    }

    /// Set how a single declared or waiting plugin panel draws by id.
    pub fn set_declared_one(&self, render: DeclaredOneRender) {
        *self.declared_one.write() = Some(render);
    }

    /// How a single declared or waiting plugin panel draws by id.
    pub fn declared_one(&self) -> Option<DeclaredOneRender> {
        *self.declared_one.read()
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
            dock: DockSpec::for_location(PanelLocation::Sidebar),
        });
        let first = host.cached_at(PanelLocation::Sidebar);
        assert!(Arc::ptr_eq(&first, &host.cached_at(PanelLocation::Sidebar)));

        host.add(PanelEntry {
            id: "second",
            title: "Second",
            location: PanelLocation::Sidebar,
            render,
            visible: ALWAYS,
            dock: DockSpec::for_location(PanelLocation::Sidebar),
        });
        let rebuilt = host.cached_at(PanelLocation::Sidebar);
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        assert_eq!(rebuilt.len(), 2);
    }
}
