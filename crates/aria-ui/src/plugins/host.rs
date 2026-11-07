//! Storage and settings: the two plugins the shell cannot run without.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_plugin::{
    Plugin, PluginContext, PluginError, PluginManifest, Service, SettingsService, StorageService,
    WeakContext,
};
use aria_settings::Settings;
use aria_storage::StorageLayout;
use parking_lot::Mutex;

use super::register_dyn;

/// The data root every other plugin's directory hangs off.
///
/// No `Default`: the root is the session's profile and only the caller knows it,
/// so a host that omitted it would have written into the real profile
/// directory. See [`crate::plugins::builtin`].
#[derive(Debug)]
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
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.storage", "Storage", "0.1.0", &[], &[]);
        &MANIFEST
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
            .map_err(|err| PluginError::MountFailed("aria.storage".into(), err.to_string()))?;
        let cache = layout.root().join("cache");
        let project = layout.root().join("projects");
        let _ = std::fs::create_dir_all(&cache);
        let _ = std::fs::create_dir_all(&project);
        ctx.register(layout);
        register_dyn::<dyn StorageService>(ctx, Arc::new(StoragePaths { cache, project }));
        Ok(())
    }
}

/// The session's config file, served to plugins as [`SettingsService`].
///
/// No `Default`: the file is the session profile's `config.toml`, and only the
/// caller knows the profile. See [`crate::plugins::builtin`].
#[derive(Debug)]
pub struct SettingsPlugin {
    pub path: PathBuf,
}

struct SettingsStore {
    path: PathBuf,
    inner: Mutex<StoreState>,
    /// The fields plugins declared, for the default an unset key reads as.
    schema: Arc<aria_plugin::SettingsSchema>,
    /// The context whose waiting packages a read or write of their key loads.
    /// Weak, because the context holds this store.
    ctx: WeakContext,
}

/// The file as last read, with the writes made since the last save.
///
/// The session writes the same file (the storage root, cache paths, quota, AI
/// and provider fields), so a save replays `pending` onto the file's current
/// contents instead of writing this view back over them, and the session's
/// writes reach this view through [`SettingsService::reload`]. A read never
/// touches the disk: the Settings rows read through it on every frame.
struct StoreState {
    settings: Settings,
    /// Every `set` since the last save, in order.
    pending: Vec<(String, serde_json::Value)>,
}

impl StoreState {
    /// `path`'s current contents with the unsaved writes on top.
    fn fresh(&self, path: &Path) -> Result<Settings, String> {
        let mut fresh = aria_settings::load(path)?;
        for (key, value) in &self.pending {
            apply(&mut fresh, key, value);
        }
        Ok(fresh)
    }
}

/// Write one `SettingsService::set` into `settings`.
fn apply(settings: &mut Settings, key: &str, value: &serde_json::Value) {
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
            let stored = match value {
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

impl SettingsStore {
    /// Load the packages waiting for `key`, before the lock is taken: a mount
    /// reads and writes settings.
    fn activate(&self, key: &str) {
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.activate_setting(key);
        }
    }

    fn read(&self, key: &str) -> Option<serde_json::Value> {
        let state = self.inner.lock();
        let settings = &state.settings;
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
                // it. An unset key reads as the default its field declares.
                match settings.plugin_value(other) {
                    Some(value) => Some(serde_json::Value::String(value.to_string())),
                    None => self
                        .schema
                        .field(other)
                        .and_then(|(_, spec)| spec.default)
                        .map(serde_json::Value::String),
                }
            }
        }
    }
}

impl Service for SettingsStore {}

impl SettingsService for SettingsStore {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.activate(key);
        self.read(key)
    }

    fn peek(&self, key: &str) -> Option<serde_json::Value> {
        self.read(key)
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        self.activate(key);
        let mut state = self.inner.lock();
        apply(&mut state.settings, key, &value);
        state.pending.push((key.to_string(), value));
    }

    fn save(&self) -> Result<(), PluginError> {
        let failed = |err: String| PluginError::MountFailed("aria.settings".into(), err);
        let mut state = self.inner.lock();
        let fresh = state.fresh(&self.path).map_err(failed)?;
        match aria_settings::save(&self.path, &fresh) {
            Err(err) if !err.is_committed() => Err(failed(err.into())),
            saved => {
                // A save whose folder could not be synced is in force all the same.
                state.settings = fresh;
                state.pending.clear();
                saved.map_err(|err| failed(err.into()))
            }
        }
    }

    /// A file left unreadable keeps the last good view; the next save reports
    /// the fault.
    fn reload(&self) {
        let mut state = self.inner.lock();
        if let Ok(fresh) = state.fresh(&self.path) {
            state.settings = fresh;
        }
    }
}

impl Plugin for SettingsPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.settings", "Settings", "0.1.0", &[], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let settings = aria_settings::load(&self.path)
            .map_err(|err| PluginError::MountFailed("aria.settings".into(), err))?;
        // The registry a plugin declares its own settings into. Registered here
        // because this plugin owns the file those settings land in, and it mounts
        // early enough for every later plugin to declare against it.
        let schema = Arc::new(aria_plugin::SettingsSchema::default());
        register_dyn(ctx, Arc::clone(&schema));
        register_dyn::<dyn SettingsService>(
            ctx,
            Arc::new(SettingsStore {
                path: self.path.clone(),
                inner: Mutex::new(StoreState {
                    settings,
                    pending: Vec::new(),
                }),
                schema,
                ctx: ctx.downgrade(),
            }),
        );
        Ok(())
    }
}
