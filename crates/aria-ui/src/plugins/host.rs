//! Storage and settings: the two plugins the shell cannot run without.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_plugin::{
    Plugin, PluginContext, PluginError, PluginManifest, Service, SettingsService, StorageService,
};
use aria_settings::{Settings, default_config_path};
use aria_storage::StorageLayout;
use parking_lot::Mutex;

use super::register_dyn;

/// The data root every other plugin's directory hangs off.
///
/// No `Default`: the root is the session's profile and only the caller knows it,
/// so a host that forgot to say would have written into the operator's real
/// directory. See [`crate::plugins::builtin`].
pub struct StoragePlugin {
    pub root: PathBuf,
}

struct StoragePaths {
    cache: PathBuf,
    project: PathBuf,
}

impl Service for StoragePaths {}

impl StorageService for StoragePaths {
    fn cache_dir(&self) -> &Path {
        &self.cache
    }

    fn project_dir(&self) -> &Path {
        &self.project
    }
}

impl Plugin for StoragePlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.storage",
            name: "Storage",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let root = if self.root.is_absolute() {
            self.root.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/tmp"))
                .join(&self.root)
        };
        let _ = std::fs::create_dir_all(&root);
        let layout = StorageLayout::new(&root)
            .map_err(|err| PluginError::MountFailed("aria.storage", err.to_string()))?;
        let cache = layout.root().join("cache");
        let project = layout.root().join("projects");
        let _ = std::fs::create_dir_all(&cache);
        let _ = std::fs::create_dir_all(&project);
        ctx.register(layout);
        register_dyn::<dyn StorageService>(ctx, Arc::new(StoragePaths { cache, project }));
        Ok(())
    }
}

pub struct SettingsPlugin {
    pub path: PathBuf,
}

impl Default for SettingsPlugin {
    fn default() -> Self {
        Self {
            path: default_config_path(),
        }
    }
}

struct SettingsStore {
    path: PathBuf,
    inner: Mutex<Settings>,
}

impl Service for SettingsStore {}

impl SettingsService for SettingsStore {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        let settings = self.inner.lock();
        match key {
            "ai.source" => Some(serde_json::Value::String(settings.ai.source.key().into())),
            "ai.model" => Some(serde_json::Value::String(settings.ai.model.clone())),
            "ai.base_url" => Some(serde_json::Value::String(settings.ai.base_url.clone())),
            "cloud.base_url" => Some(serde_json::Value::String(settings.cloud.base_url.clone())),
            "storage.download_quota_mib" => {
                Some(serde_json::json!(settings.storage.download_quota_mib))
            }
            other => {
                if let Some(rest) = other.strip_prefix("providers.") {
                    let name = rest.strip_suffix(".configured")?;
                    return Some(serde_json::Value::Bool(
                        settings.provider(name).is_configured(),
                    ));
                }
                // A key this crate does not own belongs to the plugin that declared
                // it. Unknown keys used to be dropped on write and absent on read,
                // so a plugin's own setting could not be stored at all.
                settings
                    .plugin_value(other)
                    .map(|value| serde_json::Value::String(value.to_string()))
            }
        }
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        let mut settings = self.inner.lock();
        match key {
            "ai.source" => {
                if let Some(raw) = value.as_str()
                    && let Some(source) = aria_settings::AiSource::from_key(raw)
                {
                    settings.ai.source = source;
                }
            }
            "ai.model" => {
                if let Some(model) = value.as_str() {
                    settings.ai.model = model.to_string();
                }
            }
            "ai.base_url" => {
                if let Some(url) = value.as_str() {
                    settings.ai.base_url = url.to_string();
                }
            }
            "cloud.base_url" => {
                if let Some(url) = value.as_str() {
                    settings.cloud.base_url = url.to_string();
                }
            }
            other => {
                let stored = match &value {
                    serde_json::Value::String(text) => Some(text.clone()),
                    serde_json::Value::Null => None,
                    // Anything else is stored as it reads, so a plugin that keeps a
                    // number or a flag is not silently dropped.
                    other => Some(other.to_string()),
                };
                settings.set_plugin_value(other, stored.as_deref());
            }
        }
    }

    fn save(&self) -> Result<(), PluginError> {
        let settings = self.inner.lock().clone();
        aria_settings::save(&self.path, &settings)
            .map_err(|err| PluginError::MountFailed("aria.settings", err))
    }
}

impl Plugin for SettingsPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.settings",
            name: "Settings",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let settings = aria_settings::load(&self.path)
            .map_err(|err| PluginError::MountFailed("aria.settings", err))?;
        ctx.register(settings.clone());
        register_dyn::<dyn SettingsService>(
            ctx,
            Arc::new(SettingsStore {
                path: self.path.clone(),
                inner: Mutex::new(settings),
            }),
        );
        // The registry a plugin declares its own settings into. Registered here
        // because this plugin owns the file those settings land in, and it mounts
        // early enough for every later plugin to declare against it.
        register_dyn(ctx, Arc::new(aria_plugin::SettingsSchema::default()));
        Ok(())
    }
}
