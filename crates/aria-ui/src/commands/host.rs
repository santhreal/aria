//! Where a palette command comes from.
//!
//! The palette used to be a compiled table, so a plugin could add a service and
//! a menu item and still have no way to offer an action by name. Commands are
//! entries in this registry instead: the shipped ones register through the same
//! call a plugin makes, and the palette draws whatever is registered.

use std::sync::atomic::{AtomicU64, Ordering};

use aria_plugin::{PluginContext, Service};
use parking_lot::RwLock;

use crate::app::AppState;

/// What a command does with the state alone. A plain function pointer, not a
/// closure: a command acts on `AppState` and holds no state of its own.
pub type CommandRun = fn(&mut AppState);

/// What a command does when it needs the entity context: start a job, spawn a
/// task, drive a channel back into the state.
///
/// A second signature rather than one that takes the context everywhere, because
/// most commands do not need it and `AppState` has no context of its own — a
/// headless state (a test, the mobile FFI) can still run every plain command.
pub type WindowedRun = fn(&mut AppState, &mut gpui::Context<AppState>);

/// One contributed command that runs against the state alone.
#[derive(Clone, Copy)]
pub struct CommandEntry {
    /// Element id, also the identity the registry de-duplicates on.
    pub id: &'static str,
    /// What the palette shows and filters on.
    pub label: &'static str,
    pub run: CommandRun,
}

/// One contributed command that needs the entity context.
#[derive(Clone, Copy)]
pub struct WindowedEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub run: WindowedRun,
}

/// How a registered command is invoked.
#[derive(Clone, Copy)]
pub enum CommandBody {
    Plain(CommandRun),
    Windowed(WindowedRun),
}

/// One registered command, whichever kind it is.
#[derive(Clone, Copy)]
pub struct Registered {
    pub id: &'static str,
    pub label: &'static str,
    pub body: CommandBody,
}

/// Every command the palette can offer, in registration order.
#[derive(Default)]
pub struct CommandHost {
    entries: RwLock<Vec<Registered>>,
    revision: AtomicU64,
}

impl Service for CommandHost {}

impl CommandHost {
    /// Add a command. A second registration of the same id replaces the first, so
    /// a plugin can override a shipped command rather than double it.
    pub fn add(&self, entry: CommandEntry) {
        self.register(Registered {
            id: entry.id,
            label: entry.label,
            body: CommandBody::Plain(entry.run),
        });
    }

    /// Add a command that needs the entity context. Same identity rules: a
    /// windowed registration replaces a plain one of the same id, and the other
    /// way round, because an id is one row in the palette either way.
    pub fn add_windowed(&self, entry: WindowedEntry) {
        self.register(Registered {
            id: entry.id,
            label: entry.label,
            body: CommandBody::Windowed(entry.run),
        });
    }

    fn register(&self, entry: Registered) {
        let mut entries = self.entries.write();
        if let Some(existing) = entries.iter_mut().find(|held| held.id == entry.id) {
            *existing = entry;
            self.revision.fetch_add(1, Ordering::Relaxed);
            return;
        }
        entries.push(entry);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// Every command, in registration order. That order is the palette's order, so
    /// an empty query offers the shipped commands first.
    pub fn all(&self) -> Vec<Registered> {
        self.entries.read().clone()
    }

    /// Monotonic registration revision for caches derived from this registry.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Every registered id, in order. For the tests and the Plugins list.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.read().iter().map(|entry| entry.id).collect()
    }
}

/// The registry the palette reads, or nothing when no host mounted it.
///
/// A missing registry is not an error: a headless `AppState` has no palette and
/// nothing asks it for a command.
pub fn host_of(ctx: &PluginContext) -> Option<std::sync::Arc<CommandHost>> {
    ctx.try_get::<CommandHost>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_: &mut AppState) {}

    fn entry(id: &'static str, label: &'static str) -> CommandEntry {
        CommandEntry {
            id,
            label,
            run: noop,
        }
    }

    /// Registration order is the palette's order, because an empty query lists
    /// everything and the first hit is what Enter runs.
    #[test]
    fn commands_keep_the_order_they_registered_in() {
        let host = CommandHost::default();
        host.add(entry("pal-a", "A"));
        host.add(entry("pal-b", "B"));
        assert_eq!(host.ids(), vec!["pal-a", "pal-b"]);
    }

    /// Overriding is how a plugin replaces a shipped command; doubling it would
    /// show two identical rows and break GPUI element identity.
    #[test]
    fn a_second_registration_of_an_id_replaces_the_first() {
        let host = CommandHost::default();
        host.add(entry("pal-a", "A"));
        host.add(entry("pal-a", "Replaced"));
        assert_eq!(host.ids(), vec!["pal-a"]);
        assert_eq!(host.all()[0].label, "Replaced");
    }

    /// A windowed command shares the id space with a plain one, because the
    /// palette draws one list: a plugin that replaces a shipped detector command
    /// must replace the row, not add a second one with the same name.
    #[test]
    fn a_windowed_registration_replaces_a_plain_one_of_the_same_id() {
        let host = CommandHost::default();
        host.add(entry("pal-a", "A"));
        host.add_windowed(WindowedEntry {
            id: "pal-a",
            label: "Windowed",
            run: |_, _| {},
        });
        assert_eq!(host.ids(), vec!["pal-a"]);
        assert!(matches!(host.all()[0].body, CommandBody::Windowed(_)));
    }

    /// And the kinds keep their order across both calls, since the palette's
    /// order is registration order.
    #[test]
    fn plain_and_windowed_commands_keep_one_order() {
        let host = CommandHost::default();
        host.add(entry("pal-a", "A"));
        host.add_windowed(WindowedEntry {
            id: "pal-b",
            label: "B",
            run: |_, _| {},
        });
        host.add(entry("pal-c", "C"));
        assert_eq!(host.ids(), vec!["pal-a", "pal-b", "pal-c"]);
        assert!(matches!(host.all()[1].body, CommandBody::Windowed(_)));
    }
}
