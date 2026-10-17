//! Themes contributed by plugins (`PluginContext::register_theme` and
//! `themes/*.toml` in plugin packages).
//!
//! A theme is the text of a theme file: a `[dark]` table, a `[light]` table,
//! or both, mapping token names to values, as in `<data dir>/themes`. The host
//! reads the tokens; a theme it cannot read is listed in Settings with the
//! reason, beside the data-dir files that failed.

use std::sync::Arc;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::listed::{Listed, owner};
use crate::registration::{Contribution, ContributionKind, Registration};

/// The largest theme text registered, in bytes.
pub const THEME_MAX_BYTES: usize = 64 * 1024;

/// The longest theme id, in bytes.
pub const THEME_ID_MAX_BYTES: usize = 64;

/// One theme contributed by a mounted plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTheme {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    /// Display name of the registering plugin.
    pub plugin: Arc<str>,
    /// Unique within the plugin: ASCII letters, digits, `-`, `_` and `.`, not
    /// starting with `.`.
    pub id: Arc<str>,
    /// The theme file text.
    pub text: Arc<str>,
}

impl PluginTheme {
    /// The name a host saves for this theme: `<plugin id>/<theme id>`. A
    /// data-dir theme file name holds no `/`, so the two never collide.
    pub fn key(&self) -> String {
        format!("{}/{}", self.plugin_id, self.id)
    }
}

/// The themes registered on one [`PluginContext`], in registration order.
pub(crate) type Themes = Listed<PluginTheme>;

/// Why `id` is not a theme id, or `None` when it is one.
pub(crate) fn id_fault(id: &str) -> Option<String> {
    if id.is_empty() {
        return Some("a theme needs an id".into());
    }
    if id.len() > THEME_ID_MAX_BYTES {
        return Some(format!(
            "theme id `{id}` is longer than {THEME_ID_MAX_BYTES} bytes"
        ));
    }
    if id.starts_with('.')
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Some(format!(
            "theme id `{id}` may hold ASCII letters, digits, `-`, `_` and `.`, and may not start with `.`"
        ));
    }
    None
}

impl PluginContext {
    /// Register a theme: `text` is a theme file, `id` names it within the
    /// registering plugin.
    ///
    /// Held on the terms of [`register_preset`](Self::register_preset): a
    /// registration made during mount drops when the plugin unmounts. Refused
    /// when the id is malformed or already registered by this plugin, or the
    /// text is longer than [`THEME_MAX_BYTES`]. The tokens are read by the
    /// host when it lists themes.
    pub fn register_theme(
        &self,
        id: &str,
        text: impl Into<Arc<str>>,
    ) -> Result<Registration, PluginError> {
        if let Some(fault) = id_fault(id) {
            return Err(PluginError::ThemeRefused(fault));
        }
        let text = text.into();
        if text.len() > THEME_MAX_BYTES {
            return Err(PluginError::ThemeRefused(format!(
                "theme `{id}` is {} bytes; the limit is {THEME_MAX_BYTES}",
                text.len()
            )));
        }
        let (plugin_id, plugin) = owner(self);
        let item = PluginTheme {
            plugin_id: Arc::clone(&plugin_id),
            plugin,
            id: Arc::from(id),
            text,
        };
        let guard = self
            .theme_registry()
            .insert(&plugin_id, item, |held| *held.id == *id)
            .ok_or_else(|| {
                PluginError::ThemeRefused(format!(
                    "plugin `{}` already registered a theme named `{id}`",
                    if plugin_id.is_empty() {
                        "host"
                    } else {
                        &*plugin_id
                    },
                ))
            })?;
        let contribution = Contribution::new(ContributionKind::Theme, id.to_owned());
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Every registered theme, in registration order.
    pub fn themes(&self) -> Vec<PluginTheme> {
        self.theme_registry().items()
    }

    /// Counts theme registrations and removals.
    pub fn themes_revision(&self) -> u64 {
        self.theme_registry().revision()
    }
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
