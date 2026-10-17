//! Commands: actions a plugin adds to the command palette, and the menu rows
//! and default chords that offer them.
//!
//! A command runs on the thread that asks for it, through the registering
//! plugin's own view of the context, so it reads only the services its manifest
//! declares. A command that panics fails its plugin for the session. A menu
//! entry or key default holds no code: it names the command it runs.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;

use crate::command_icon::CommandIcon;
use crate::context::PluginContext;
use crate::enablement::{ContextKeys, Enablement};
use crate::error::PluginError;
use crate::events::panic_message;
use crate::keys::KeyDefault;
use crate::menus::MenuCommand;
use crate::plugin::PluginManifest;
use crate::registration::{Contribution, ContributionKind, Registration};

/// What the palette lists for one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Unique across the context. By convention `<plugin id>.<verb>`.
    pub id: String,
    /// The palette label.
    pub title: String,
    /// Drawn beside the title in the palette and in menus.
    pub icon: Option<CommandIcon>,
    /// The editor state the command needs; `None` runs in any state.
    pub enablement: Option<Enablement>,
}

impl CommandSpec {
    /// A command with no icon that runs in any state.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
            enablement: None,
        }
    }

    /// Whether the command runs while `keys` hold.
    pub fn enabled(&self, keys: ContextKeys) -> bool {
        self.enablement
            .as_ref()
            .is_none_or(|enablement| enablement.holds(keys))
    }
}

/// An action a plugin adds to the command palette.
pub trait PluginCommand: Send + Sync {
    /// Run the command. `ctx` is the registering plugin's view of the context.
    /// An `Err` is shown to the user as written.
    fn run(&self, ctx: &PluginContext) -> Result<(), String>;
}

impl<F> PluginCommand for F
where
    F: Fn(&PluginContext) -> Result<(), String> + Send + Sync,
{
    fn run(&self, ctx: &PluginContext) -> Result<(), String> {
        self(ctx)
    }
}

/// Why a command did not run to completion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// No command is registered under this id.
    #[error("no command is registered as {0}")]
    Unknown(String),
    /// The command returned this reason.
    #[error("{0}")]
    Failed(String),
    /// The command panicked with this message. Its plugin is failed.
    #[error("the command panicked: {0}")]
    Panicked(String),
    /// The command's enablement condition, quoted here, is false.
    #[error("needs {0}")]
    Disabled(String),
}

/// One registered command.
struct Entry {
    spec: Arc<CommandSpec>,
    /// The registering plugin's manifest, or `None` for the host.
    scope: Option<Arc<PluginManifest>>,
    handler: Arc<dyn PluginCommand>,
}

/// The commands registered on one context, in registration order, with the
/// menu entries and key defaults that offer them.
#[derive(Default)]
pub(crate) struct Commands {
    entries: RwLock<Vec<Entry>>,
    menus: RwLock<Vec<Placed<MenuCommand>>>,
    keys: RwLock<Vec<Placed<KeyDefault>>>,
    /// The serial the next menu entry or key default takes, so a drop removes
    /// the one it registered and not an equal one registered since.
    serial: AtomicU64,
    /// Moves on every registration and removal, for caches derived from the
    /// three lists.
    revision: AtomicU64,
    /// The [`ContextKeys`] the host last published.
    context: AtomicU32,
}

/// One menu entry or key default, with the serial its drop removes it by.
struct Placed<T> {
    serial: u64,
    item: Arc<T>,
}

/// What one [`Unregister`] removes.
enum Slot {
    Command(String),
    Menu(u64),
    Key(u64),
}

/// Removes one registration when its plugin's registration drops.
struct Unregister {
    registry: Weak<Commands>,
    slot: Slot,
}

impl Drop for Unregister {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        // What is removed drops after the lock is released, so a handler's drop
        // code may read the registry.
        match &self.slot {
            Slot::Command(id) => {
                let removed: Vec<_> = registry
                    .entries
                    .write()
                    .extract_if(.., |entry| entry.spec.id == *id)
                    .collect();
                drop(removed);
            }
            Slot::Menu(serial) => {
                registry.menus.write().retain(|held| held.serial != *serial);
            }
            Slot::Key(serial) => {
                registry.keys.write().retain(|held| held.serial != *serial);
            }
        }
        registry.revision.fetch_add(1, Ordering::Relaxed);
    }
}

fn refused(reason: String) -> PluginError {
    PluginError::CommandRefused(reason)
}

impl PluginContext {
    /// Add a command to the palette.
    ///
    /// Held on the terms of [`register_importer`](Self::register_importer):
    /// a registration made during mount drops when the plugin unmounts.
    /// Refused when the id is empty or taken, or the title is empty.
    pub fn register_command(
        &self,
        spec: CommandSpec,
        command: impl PluginCommand + 'static,
    ) -> Result<Registration, PluginError> {
        if spec.id.trim().is_empty() {
            return Err(refused("a command needs an id".into()));
        }
        if spec.title.trim().is_empty() {
            return Err(refused(format!("{}: a command needs a title", spec.id)));
        }
        let registry = self.command_registry();
        let id = spec.id.clone();
        {
            let mut entries = registry.entries.write();
            if entries.iter().any(|entry| entry.spec.id == id) {
                return Err(refused(format!("a command is registered as {id}")));
            }
            entries.push(Entry {
                spec: Arc::new(spec),
                scope: self.scope(),
                handler: Arc::new(command),
            });
        }
        registry.revision.fetch_add(1, Ordering::Relaxed);
        let contribution = Contribution::new(ContributionKind::Command, id.clone());
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                slot: Slot::Command(id),
            },
        )))
    }

    /// Offer a command as a menu row.
    ///
    /// Held like [`register_command`](Self::register_command). The command may
    /// be registered later, or by another plugin: a host draws the row while a
    /// command by that id is registered. Refused when the command id is empty
    /// or the same command is already in the same place.
    pub fn register_menu_entry(&self, entry: MenuCommand) -> Result<Registration, PluginError> {
        if entry.command.trim().is_empty() {
            return Err(refused(format!(
                "a menu entry in {} names no command",
                entry.path
            )));
        }
        let registry = self.command_registry();
        let contribution = Contribution::new(
            ContributionKind::Menu,
            format!("{} in {}", entry.command, entry.path),
        );
        let serial = {
            let mut menus = registry.menus.write();
            if menus.iter().any(|held| *held.item == entry) {
                return Err(refused(format!(
                    "{} is already in {}",
                    entry.command, entry.path
                )));
            }
            let serial = registry.serial.fetch_add(1, Ordering::Relaxed);
            menus.push(Placed {
                serial,
                item: Arc::new(entry),
            });
            serial
        };
        registry.revision.fetch_add(1, Ordering::Relaxed);
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                slot: Slot::Menu(serial),
            },
        )))
    }

    /// Give a command a default chord.
    ///
    /// Held like [`register_command`](Self::register_command). Whether the chord
    /// is free is the host's to settle when it binds: a chord already taken is
    /// refused there, and recorded where the user rebinds keys. Refused here
    /// when the command id is empty or the command already has a default on the
    /// same route.
    pub fn register_key_default(&self, key: KeyDefault) -> Result<Registration, PluginError> {
        if key.command.trim().is_empty() {
            return Err(refused(format!(
                "the key {} names no command",
                key.chord.label()
            )));
        }
        let registry = self.command_registry();
        let contribution = Contribution::new(
            ContributionKind::Key,
            format!("{} for {}", key.chord.label(), key.command),
        );
        let serial = {
            let mut keys = registry.keys.write();
            if let Some(held) = keys
                .iter()
                .find(|held| held.item.command == key.command && held.item.route == key.route)
            {
                return Err(refused(format!(
                    "{} already has the {} key {}",
                    key.command,
                    key.route.label(),
                    held.item.chord.label()
                )));
            }
            let serial = registry.serial.fetch_add(1, Ordering::Relaxed);
            keys.push(Placed {
                serial,
                item: Arc::new(key),
            });
            serial
        };
        registry.revision.fetch_add(1, Ordering::Relaxed);
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Unregister {
                registry: Arc::downgrade(registry),
                slot: Slot::Key(serial),
            },
        )))
    }

    /// Every menu entry, in registration order.
    pub fn menu_entries(&self) -> Vec<Arc<MenuCommand>> {
        self.command_registry()
            .menus
            .read()
            .iter()
            .map(|held| Arc::clone(&held.item))
            .collect()
    }

    /// Every key default, in registration order.
    pub fn key_defaults(&self) -> Vec<Arc<KeyDefault>> {
        self.command_registry()
            .keys
            .read()
            .iter()
            .map(|held| Arc::clone(&held.item))
            .collect()
    }

    /// Moves whenever a command, menu entry, or key default is registered or
    /// removed. A host caches what it draws from them against this.
    pub fn command_revision(&self) -> u64 {
        self.command_registry().revision.load(Ordering::Relaxed)
    }

    /// Publish the editor state enablement conditions read. The editor calls
    /// this with every drawn frame; a context no host publishes to holds no
    /// key.
    pub fn set_context_keys(&self, keys: ContextKeys) {
        self.command_registry()
            .context
            .store(keys.bits(), Ordering::Relaxed);
    }

    /// The keys last published with [`set_context_keys`](Self::set_context_keys).
    pub fn context_keys(&self) -> ContextKeys {
        ContextKeys::from_bits(self.command_registry().context.load(Ordering::Relaxed))
    }

    /// Every registered command, in registration order.
    pub fn commands(&self) -> Vec<Arc<CommandSpec>> {
        self.command_registry()
            .entries
            .read()
            .iter()
            .map(|entry| Arc::clone(&entry.spec))
            .collect()
    }

    /// Run the command registered as `id`.
    ///
    /// The command runs with no registry lock held, so it may register or run
    /// other commands. A panic fails the registering plugin for the session.
    /// Refused with [`CommandError::Disabled`] while its enablement condition
    /// is false under [`context_keys`](Self::context_keys).
    pub fn run_command(&self, id: &str) -> Result<(), CommandError> {
        let found = self
            .command_registry()
            .entries
            .read()
            .iter()
            .find(|entry| entry.spec.id == id)
            .map(|entry| {
                (
                    Arc::clone(&entry.spec),
                    entry.scope.clone(),
                    Arc::clone(&entry.handler),
                )
            });
        let Some((spec, scope, handler)) = found else {
            return Err(CommandError::Unknown(id.to_owned()));
        };
        if let Some(enablement) = &spec.enablement
            && !enablement.holds(self.context_keys())
        {
            return Err(CommandError::Disabled(enablement.source().to_owned()));
        }
        let view = self.scoped(scope.clone());
        match catch_unwind(AssertUnwindSafe(|| handler.run(&view))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(CommandError::Failed(reason)),
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                if let Some(manifest) = &scope {
                    self.fail(&manifest.id, format!("command {id} panicked: {message}"));
                }
                Err(CommandError::Panicked(message))
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
