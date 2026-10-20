//! Plugin key defaults as bindings.
//!
//! A plugin gives a command a default chord through
//! [`PluginContext::register_key_default`] or a `key` in its `plugin.toml`. The
//! registry holds those beside the shipped bindings, so dispatch, Settings,
//! rebinding and the conflict list read one table. A sync replaces every plugin
//! binding when the context's command revision moves; the shipped bindings and
//! the rebindings stay.

use std::sync::atomic::Ordering;

use aria_plugin::PluginContext;

use super::{After, Binding, KeyBody, KeyConflict, KeyHost};
use crate::interned::intern;

impl KeyHost {
    /// Place the plugin key defaults `ctx` holds, when they changed since the
    /// last sync or a rebinding moved what they may take.
    ///
    /// A default is placed when its command is registered. One whose chord
    /// another binding answers on the same route, given the rebindings in
    /// force, is refused and recorded with the shipped conflicts. Defaults are
    /// placed in registration order, so the first plugin to ask for a chord
    /// keeps it.
    pub fn sync_plugin_keys(&self, ctx: &PluginContext) {
        let revision = ctx.command_revision().wrapping_add(1);
        if self.plugin_revision.swap(revision, Ordering::Relaxed) == revision {
            return;
        }
        let commands = ctx.commands();
        let defaults = ctx.key_defaults();
        // The registry's lock order: overrides, then bindings.
        let overrides = self.overrides.read();
        let mut bindings = self.bindings.write();
        let mut conflicts = self.conflicts.write();
        let mut placed = self.plugin_keys.write();
        let mut refused = self.plugin_refused.write();
        bindings.retain(|binding| !matches!(binding.run, KeyBody::Command(_)));
        conflicts.retain(|conflict| !placed.iter().any(|(id, _)| *id == conflict.refused));
        placed.clear();
        refused.clear();
        for key in &defaults {
            let Some(spec) = commands.iter().find(|spec| spec.id == key.command) else {
                continue;
            };
            let id = intern(&spec.id);
            let title = intern(&spec.title);
            if !placed.iter().any(|(held, _)| *held == id) {
                placed.push((id, title));
            }
            let binding = Binding {
                id,
                chord: key.chord,
                route: key.route,
                run: KeyBody::Command(title),
                after: After::Nothing,
            };
            if let Some(chord) = Self::chord_for(overrides.as_slice(), &binding)
                && let Some(held) = bindings.iter().find(|other| {
                    other.route == binding.route
                        && Self::chord_for(overrides.as_slice(), other) == Some(chord)
                })
            {
                conflicts.push(KeyConflict {
                    held_by: held.id,
                    refused: id,
                    chord,
                });
                refused.push(binding);
                continue;
            }
            bindings.push(binding);
        }
    }

    /// The plugin key the last sync refused as `id`, if one was.
    pub(super) fn refused_plugin_key(&self, id: &str) -> Option<Binding> {
        self.plugin_refused
            .read()
            .iter()
            .find(|binding| binding.id == id)
            .copied()
    }

    /// Make the next sync place the plugin keys again. A rebinding calls it:
    /// a freed chord may be the one a refused plugin key wanted.
    pub(super) fn resync_plugin_keys(&self) {
        self.plugin_revision.store(0, Ordering::Relaxed);
    }

    /// What a binding is called in Settings and the status line: a plugin
    /// command's title, or a shipped id read as words.
    pub fn label_of(&self, id: &str) -> String {
        self.plugin_keys
            .read()
            .iter()
            .find(|(held, _)| *held == id)
            .map_or_else(
                || super::binding_label(id),
                |(_, title)| (*title).to_owned(),
            )
    }
}

#[cfg(test)]
#[path = "plugin_keys_tests.rs"]
mod tests;
