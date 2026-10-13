//! Built-in plugins mounted by the headed app.
//!
//! Engine crates stay cycle-free: `aria-plugin` already depends on compositor
//! and render, so every `Plugin` impl lives here and wraps the public crate
//! APIs. Mount order matches PLAN.md Phase 2.2.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_plugin::{MountFailure, Plugin, PluginContext};

/// File types the decoder plugin claims when they are dropped. Reuses the
/// importer's list rather than keeping a second one, so a format the picker
/// offers and a format a drop accepts can never disagree.
const MEDIA_EXTENSIONS: &[&str] = crate::app::media::import::MEDIA_IMPORT_EXTENSIONS;

/// Case-insensitive extension test, for the drop claims below.
fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.iter().any(|want| ext.eq_ignore_ascii_case(want)))
}

/// Every built-in plugin, for a session on `data_dir`.
///
/// One owner: the ids, the objects a host needs to re-mount a disabled plugin, and
/// the set `mount_all` orders all come from here, so a new plugin cannot be added
/// to one list and forgotten in another. The order below is the order among
/// plugins that do not depend on each other; the rest is derived from each
/// manifest's `requires` by `aria_plugin::load_order`.
///
/// `data_dir` reaches exactly one plugin. Storage owns the root and registers a
/// `StorageLayout` for it; everything that used to resolve its own directory —
/// the transcription models, the cloud catalog cache — asks that service instead,
/// so a session on a scratch profile cannot half-write into the default one.
pub fn builtin(data_dir: &Path) -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(StoragePlugin {
            root: data_dir.to_path_buf(),
        }),
        Box::new(SettingsPlugin::default()),
        Box::new(DecoderPlugin),
        Box::new(EncoderPlugin),
        Box::new(CompositorPlugin),
        Box::new(RenderPlugin),
        // After `aria.render`, which registers the device they render on.
        Box::new(EffectPlugin),
        Box::new(TransitionPlugin),
        Box::new(EnginePlugin),
        Box::new(CloudPlugin::default()),
        Box::new(AnalysisPlugin),
        Box::new(JobsPlugin),
        Box::new(TranscriptionPlugin::default()),
        Box::new(AiPlugin),
        // Last: the panel registry is what the shell reads, so anything that wants
        // to contribute a panel has mounted by now.
        Box::new(UiPlugin),
    ]
}

/// The built-in plugin with this id, for re-mounting one that was disabled.
pub fn builtin_by_id(id: &str, data_dir: &Path) -> Option<Box<dyn Plugin>> {
    builtin(data_dir)
        .into_iter()
        .find(|plugin| plugin.manifest().id == id)
}

/// Mount every built-in plugin, then every external one, keeping going past a
/// failure.
///
/// A missing GPU or an unreadable settings file used to abort the whole mount and
/// take the editor with it. Each failure is recorded on the context instead, where
/// the Plugins list shows it, and the app starts with the capabilities that did
/// mount. Returns the failures for a headless caller.
///
/// External plugins mount last, so a claim they make loses to a built-in's rather
/// than silently taking a file type the app already handles.
///
/// `data_dir` is where an external plugin and a catalog override are looked for,
/// and it is the caller's because the session already has one: the mount used to
/// resolve its own from [`crate::settings::default_data_dir`], so a process
/// pointed at another profile — a second instance, or a test with a scratch
/// directory — kept its settings and its recovery file there while loading code
/// and catalogs out of the default one.
pub fn mount_all(ctx: &PluginContext, data_dir: &Path) -> Vec<MountFailure> {
    // Before anything reads a catalog: a catalog is read once, so telling the
    // model crate where the files are has to happen before the first plugin that
    // touches one.
    aria_models::catalog::set_root(catalog_dir(data_dir));
    // Read them all now, so a refused override is a fact the Catalogs list can
    // show at startup rather than one that surfaces when a panel first opens.
    aria_models::catalog::preload();
    // Mount order comes from the declared dependencies, not from the order above:
    // a plugin listed before the one it requires used to fail with a message that
    // read like a missing plugin.
    let plugins = builtin(data_dir);
    let manifests: Vec<_> = plugins.iter().map(|p| *p.manifest()).collect();
    match aria_plugin::load_order(&manifests) {
        Ok(order) => {
            for id in order {
                if let Some(plugin) = plugins.iter().find(|p| p.manifest().id == id) {
                    let _ = ctx.mount(plugin.as_ref());
                }
            }
        }
        // A cycle among the built-ins is a bug in this file, not a user's problem:
        // mount in the listed order so the editor still starts, and let the failure
        // ledger carry the reason.
        Err(err) => {
            tracing::error!(error = %err, "built-in plugin order is cyclic");
            for plugin in &plugins {
                let _ = ctx.mount(plugin.as_ref());
            }
        }
    }
    aria_plugin::external::mount_dir(ctx, &external_plugin_dir(data_dir));
    ctx.failures()
}

/// Where an external plugin library is looked for: `<data dir>/plugins`.
pub fn external_plugin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

/// Where a catalog override is looked for: `<data dir>/catalogs`.
pub fn catalog_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("catalogs")
}

fn register_dyn<T: ?Sized + Send + Sync + 'static>(ctx: &PluginContext, service: Arc<T>) {
    let _ = ctx.register_arc(service);
}

mod engine;
mod gpu;
mod host;
mod media;
pub mod overlay_host;
pub mod panel_host;
mod services;
mod ui;

pub use engine::{EnginePlugin, RenderPlugin};
pub use gpu::{CompositorPlugin, EffectPlugin, TransitionPlugin};
pub use host::{SettingsPlugin, StoragePlugin};
pub use media::{DecoderPlugin, EncoderPlugin};
pub(crate) use services::announce_jobs;
pub use services::{
    AiHost, AiPlugin, AnalysisPlugin, CloudPlugin, DetectorTuning, JobsPlugin, TranscriptionPlugin,
};
pub use ui::UiPlugin;

/// Ids every successful [`mount_all`] records, in order. Derived from
/// [`builtin`], so there is nothing to keep in step.
///
/// An id belongs to the plugin set, not to a profile, so the list is built
/// against no directory at all. Nothing here is mounted: a plugin object is a
/// manifest and a `mount` that has not been called.
pub fn builtin_plugin_ids() -> Vec<&'static str> {
    builtin(Path::new(""))
        .iter()
        .map(|plugin| plugin.manifest().id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use aria_decoder::probe;
    use aria_jobs::JobManager;
    use aria_plugin::{
        DecoderService, PluginError, PluginManifest, SettingsService, StorageService,
    };

    #[test]
    fn storage_and_settings_mount_without_gpu() {
        let dir = crate::scratch_pool::scratch("plugin-mount");
        let ctx = PluginContext::new();
        ctx.mount(&StoragePlugin {
            root: dir.join("storage"),
        })
        .unwrap();
        ctx.mount(&SettingsPlugin {
            path: dir.join("settings.toml"),
        })
        .unwrap();
        assert!(ctx.try_get::<dyn StorageService>().is_some());
        let settings = ctx.get::<dyn SettingsService>();
        assert!(settings.get("ai.source").is_some());
        ctx.mount(&DecoderPlugin).unwrap();
        ctx.mount(&EncoderPlugin).unwrap();
        ctx.mount(&JobsPlugin).unwrap();
        assert!(ctx.try_get::<JobManager>().is_some());
        assert!(
            ctx.get::<dyn DecoderService>()
                .can_decode(Path::new("/tmp/aria-missing-media.mp4"))
                == probe(Path::new("/tmp/aria-missing-media.mp4")).is_ok()
        );
    }

    /// A duplicate id would silently shadow a plugin in the list, and a manifest
    /// with no version or an unsatisfiable dependency would fail at boot.
    #[test]
    fn every_builtin_manifest_is_well_formed() {
        let ids = builtin_plugin_ids();
        assert!(!ids.is_empty(), "no built-in plugins");
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id), "duplicate plugin id {id}");
        }
        for plugin in builtin(Path::new("")) {
            let manifest = plugin.manifest();
            assert!(!manifest.name.is_empty(), "{} has no name", manifest.id);
            assert!(
                !manifest.version.is_empty(),
                "{} has no version",
                manifest.id
            );
            // A dependency must be a plugin that mounts earlier, or the mount
            // order in `builtin` cannot satisfy it.
            let position = ids
                .iter()
                .position(|id| *id == manifest.id)
                .expect("id comes from the same list");
            for dep in manifest.requires {
                let at = ids
                    .iter()
                    .position(|id| id == dep)
                    .unwrap_or_else(|| panic!("{} requires unknown {dep}", manifest.id));
                assert!(
                    at < position,
                    "{} requires {dep}, which mounts after it",
                    manifest.id
                );
            }
        }
    }

    /// A plugin that cannot mount must not take the rest of the list with it.
    #[test]
    fn a_failed_mount_is_recorded_and_the_rest_still_mount() {
        struct Broken(PluginManifest);

        impl Plugin for Broken {
            fn manifest(&self) -> &PluginManifest {
                &self.0
            }

            fn mount(&self, _: &PluginContext) -> Result<(), PluginError> {
                Err(PluginError::MountFailed("test.broken", "no device".into()))
            }
        }

        let ctx = PluginContext::new();
        let broken = Broken(PluginManifest {
            id: "test.broken",
            name: "Broken",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        });
        assert!(ctx.mount(&broken).is_err());
        let failures = ctx.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].id, "test.broken");
        assert!(
            failures[0].reason.contains("no device"),
            "the reason was lost: {}",
            failures[0].reason
        );
        assert!(!ctx.plugins().contains(&"test.broken"));

        // A dependent of the failed plugin fails too, and says what is missing.
        struct Dependent(PluginManifest);

        impl Plugin for Dependent {
            fn manifest(&self) -> &PluginManifest {
                &self.0
            }

            fn mount(&self, _: &PluginContext) -> Result<(), PluginError> {
                Ok(())
            }
        }

        let dependent = Dependent(PluginManifest {
            id: "test.dependent",
            name: "Dependent",
            version: "0.1.0",
            requires: &["test.broken"],
            uses: &[],
        });
        assert!(ctx.mount(&dependent).is_err());
        assert_eq!(ctx.failures().len(), 2);

        // An unrelated plugin still mounts, and its services are listed.
        ctx.mount(&StoragePlugin {
            root: crate::scratch_pool::scratch("plugin-failure"),
        })
        .expect("storage mounts");
        let mounted = ctx.mounted();
        assert_eq!(mounted.len(), 1);
        assert_eq!(mounted[0].manifest.id, "aria.storage");
        assert!(
            mounted[0]
                .services
                .iter()
                .any(|name| name.contains("StorageService")),
            "storage registered nothing visible: {:?}",
            mounted[0].services
        );
    }
}
