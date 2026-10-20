//! A list of plugin contributions in registration order. Each entry leaves the
//! list when the [`Registration`](crate::registration::Registration) holding
//! its [`Unlist`] guard drops, and every insert and removal bumps the list's
//! revision.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::RwLock;

use crate::context::PluginContext;

/// The registering plugin's id and display name: its manifest name, or its id
/// when the name is empty. The host registers as `("", "Host")`.
pub(crate) fn owner(ctx: &PluginContext) -> (Arc<str>, Arc<str>) {
    match ctx.scope() {
        Some(manifest) => {
            let name = if manifest.name.is_empty() {
                manifest.id.as_ref()
            } else {
                manifest.name.as_ref()
            };
            (Arc::from(manifest.id.as_ref()), Arc::from(name))
        }
        None => (Arc::from(""), Arc::from("Host")),
    }
}

struct Entry<T> {
    serial: u64,
    /// The registering plugin's id, empty for the host.
    plugin_id: Arc<str>,
    item: T,
}

pub(crate) struct Listed<T> {
    entries: RwLock<Vec<Entry<T>>>,
    serial: AtomicU64,
    revision: AtomicU64,
}

impl<T> Default for Listed<T> {
    fn default() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            serial: AtomicU64::new(0),
            revision: AtomicU64::new(0),
        }
    }
}

impl<T> std::fmt::Debug for Listed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listed")
            .field("entries", &self.entries.read().len())
            .field("revision", &self.revision.load(Ordering::Acquire))
            .finish()
    }
}

/// Removes one entry from its list on drop.
pub(crate) struct Unlist<T> {
    registry: Weak<Listed<T>>,
    serial: u64,
}

impl<T> Drop for Unlist<T> {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let removed: Vec<_> = registry
            .entries
            .write()
            .extract_if(.., |entry| entry.serial == self.serial)
            .collect();
        if !removed.is_empty() {
            registry.revision.fetch_add(1, Ordering::Release);
        }
        drop(removed);
    }
}

impl<T: Clone> Listed<T> {
    /// Append `item` for `plugin_id`, or return `None` when `taken` matches an
    /// item that plugin already holds.
    pub(crate) fn insert(
        self: &Arc<Self>,
        plugin_id: &Arc<str>,
        item: T,
        taken: impl Fn(&T) -> bool,
    ) -> Option<Unlist<T>> {
        self.push_unless(plugin_id, item, |entry| {
            entry.plugin_id == *plugin_id && taken(&entry.item)
        })
        .ok()
    }

    /// Append `item` for `plugin_id`, or return the item `taken` matches when
    /// any plugin holds one.
    pub(crate) fn insert_unique(
        self: &Arc<Self>,
        plugin_id: &Arc<str>,
        item: T,
        taken: impl Fn(&T) -> bool,
    ) -> Result<Unlist<T>, T> {
        self.push_unless(plugin_id, item, |entry| taken(&entry.item))
    }

    fn push_unless(
        self: &Arc<Self>,
        plugin_id: &Arc<str>,
        item: T,
        clash: impl Fn(&Entry<T>) -> bool,
    ) -> Result<Unlist<T>, T> {
        let mut entries = self.entries.write();
        if let Some(held) = entries.iter().find(|entry| clash(entry)) {
            return Err(held.item.clone());
        }
        let serial = self.serial.fetch_add(1, Ordering::Relaxed);
        entries.push(Entry {
            serial,
            plugin_id: Arc::clone(plugin_id),
            item,
        });
        self.revision.fetch_add(1, Ordering::Release);
        Ok(Unlist {
            registry: Arc::downgrade(self),
            serial,
        })
    }

    /// Every item, in registration order.
    pub(crate) fn items(&self) -> Vec<T> {
        self.entries
            .read()
            .iter()
            .map(|entry| entry.item.clone())
            .collect()
    }

    /// `view` of every item, in registration order.
    pub(crate) fn entries<U>(&self, view: impl Fn(&T) -> U) -> Vec<U> {
        self.entries
            .read()
            .iter()
            .map(|entry| view(&entry.item))
            .collect()
    }

    /// The first item `matches` accepts, in registration order.
    pub(crate) fn find(&self, matches: impl Fn(&T) -> bool) -> Option<T> {
        self.entries
            .read()
            .iter()
            .find(|entry| matches(&entry.item))
            .map(|entry| entry.item.clone())
    }

    /// Counts inserts and removals.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}
