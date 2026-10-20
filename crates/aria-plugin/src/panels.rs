//! Panels: docked views a plugin declares as a [`UiNode`] tree.
//!
//! A panel is built off the drawing thread by [`crate::live`]: on the event
//! delivery thread when a host first reads the panel, after each event the
//! panel receives, and after its plugin asks for a refresh. A host reads the
//! last built tree without waiting, and the waker it sets runs whenever a new
//! one lands. A panic in a panel's code fails its plugin for the session, as a
//! command's does.

use std::sync::Arc;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::live::{Access, Live, LiveKind};
use crate::registration::{Contribution, ContributionKind, Registration};
use crate::service::PanelLocation;
use crate::ui_tree::{EventMismatch, PanelEvent, UiNode};

/// Where a declared panel docks and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSpec {
    /// Unique across the context. By convention `<plugin id>.<panel>`.
    pub id: String,
    /// The title drawn above the panel.
    pub title: Arc<str>,
    /// One of [`DOCKED_LOCATIONS`].
    pub location: PanelLocation,
}

/// The locations a declared panel may dock at: the stacked panes. The viewer
/// and Home hold a whole body, so a declared panel there is refused.
pub const DOCKED_LOCATIONS: [PanelLocation; 3] = [
    PanelLocation::Sidebar,
    PanelLocation::Inspector,
    PanelLocation::Timeline,
];

/// A panel's code: the tree it shows, and what it does with an event.
pub trait PanelView: Send + Sync {
    /// The tree to draw. `ctx` is the registering plugin's view of the context.
    fn view(&self, ctx: &PluginContext) -> UiNode;

    /// A control in the tree was used. An `Err` is shown in the panel as
    /// written. The tree is built again afterwards either way.
    fn event(&self, ctx: &PluginContext, event: &PanelEvent) -> Result<(), String>;
}

/// What a host draws for one panel.
#[derive(Debug, Clone)]
pub struct PanelState {
    pub spec: Arc<PanelSpec>,
    /// The latest tree that built and passed [`UiNode::check`], or `None`
    /// before the first.
    pub tree: Option<Arc<UiNode>>,
    /// Why the latest build was refused. Cleared by the next build that passes;
    /// the tree before it stays in `tree`.
    pub refused: Option<Arc<str>>,
    /// The error the latest event's handler returned. Cleared by the next
    /// event the handler accepts.
    pub event_error: Option<Arc<str>>,
}

/// Why a panel call did not reach the panel.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PanelError {
    #[error("no panel is registered as {0}")]
    Unknown(String),
    #[error("panel {0} belongs to another plugin")]
    NotOwned(String),
    #[error("panel {0} has not drawn a tree yet")]
    NotDrawn(String),
    #[error("panel {panel}: {mismatch}")]
    Mismatch {
        panel: String,
        mismatch: EventMismatch,
    },
}

/// Declared panels, for [`crate::live`].
pub(crate) enum PanelKind {}

impl LiveKind for PanelKind {
    type Spec = PanelSpec;
    type View = dyn PanelView;
    type Content = UiNode;
    type Event = PanelEvent;

    const NOUN: &'static str = "panel";
    const CONTENT: &'static str = "a tree";

    fn id(spec: &PanelSpec) -> &str {
        &spec.id
    }

    fn build(view: &dyn PanelView, ctx: &PluginContext) -> UiNode {
        view.view(ctx)
    }

    fn handle(view: &dyn PanelView, ctx: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        view.event(ctx, event)
    }

    fn check(tree: &UiNode) -> Result<(), String> {
        tree.check().map_err(|err| err.to_string())
    }
}

/// The panels registered on one context, in registration order.
pub(crate) type Panels = Live<PanelKind>;

fn refused(reason: String) -> PluginError {
    PluginError::PanelRefused(reason)
}

fn access(id: &str, denied: Access) -> PanelError {
    match denied {
        Access::Unknown => PanelError::Unknown(id.to_owned()),
        Access::NotOwned => PanelError::NotOwned(id.to_owned()),
    }
}

impl PluginContext {
    /// Dock a panel whose content is the tree `view` returns.
    ///
    /// Held on the terms of [`register_importer`](Self::register_importer): a
    /// registration made during mount drops when the plugin unmounts. Refused
    /// when the id is empty or taken, the title is empty, or the location is
    /// not one of [`DOCKED_LOCATIONS`]. Nothing of `view` runs until a host
    /// first reads the panel.
    pub fn register_panel(
        &self,
        spec: PanelSpec,
        view: impl PanelView + 'static,
    ) -> Result<Registration, PluginError> {
        if spec.id.trim().is_empty() {
            return Err(refused("a panel needs an id".into()));
        }
        if spec.title.trim().is_empty() {
            return Err(refused(format!("{}: a panel needs a title", spec.id)));
        }
        if !DOCKED_LOCATIONS.contains(&spec.location) {
            return Err(refused(format!(
                "{}: a declared panel docks in the sidebar, the inspector, or the timeline area, not {:?}",
                spec.id, spec.location
            )));
        }
        let contribution = Contribution::new(ContributionKind::Panel, spec.id.clone());
        let view: Arc<dyn PanelView> = Arc::new(view);
        let unregister = self
            .panel_registry()
            .insert(self, spec, view)
            .map_err(refused)?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, unregister)))
    }

    /// Every registered panel, in registration order.
    pub fn panels(&self) -> Vec<Arc<PanelSpec>> {
        self.panel_registry().specs()
    }

    /// What `id` shows now. Never waits: the first read queues the first build,
    /// and until it lands `tree` is `None`.
    pub fn panel(&self, id: &str) -> Option<PanelState> {
        let shown = self.panel_registry().read(self, id)?;
        Some(PanelState {
            spec: shown.spec,
            tree: shown.content,
            refused: shown.refused,
            event_error: shown.event_error,
        })
    }

    /// Deliver an input event on a control of panel `id`, then build its
    /// tree again. Returns once the event is queued.
    ///
    /// The event is held to the tree the panel shows now, through
    /// [`PanelEvent::conform`]: a control that is not there, a disabled button,
    /// or a value its control cannot produce is refused here and never reaches
    /// the plugin. A plugin may send events only to its own panels.
    pub fn send_panel_event(&self, id: &str, event: PanelEvent) -> Result<(), PanelError> {
        let registry = self.panel_registry();
        let entry = registry
            .owned(self, id)
            .map_err(|denied| access(id, denied))?;
        let tree = entry
            .content()
            .ok_or_else(|| PanelError::NotDrawn(id.to_owned()))?;
        let event = event
            .conform(&tree)
            .map_err(|mismatch| PanelError::Mismatch {
                panel: id.to_owned(),
                mismatch,
            })?;
        registry.send(self, &entry, event, |_| false);
        Ok(())
    }

    /// Build panel `id`'s tree again, for state that changed outside an event:
    /// a job finished, a project opened. Refreshes queued before the build
    /// starts share one build. A plugin may refresh only its own panels.
    pub fn refresh_panel(&self, id: &str) -> Result<(), PanelError> {
        let registry = self.panel_registry();
        let entry = registry
            .owned(self, id)
            .map_err(|denied| access(id, denied))?;
        registry.refresh(self, &entry);
        Ok(())
    }

    /// A number that grows whenever any panel's state, the set of panels, or
    /// the set of [waiting panels](Self::waiting_panels) changes. A host
    /// compares it with the one it drew.
    pub fn panel_revision(&self) -> u64 {
        self.panel_registry().revision()
    }

    /// Set what runs, on whatever thread made the change, whenever
    /// [`panel_revision`](Self::panel_revision) grows. A host's waker schedules
    /// a redraw.
    pub fn set_panel_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.panel_registry().set_waker(Arc::new(waker));
    }
}

#[cfg(test)]
#[path = "panels_tests.rs"]
mod tests;
