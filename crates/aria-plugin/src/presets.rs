//! Animation presets contributed by plugins (`PluginContext::register_preset`
//! and `presets/*.ariapreset` in plugin packages).

use std::sync::Arc;

use aria_models::Preset;

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::listed::{Listed, owner};
use crate::registration::{Contribution, ContributionKind, Registration};

/// One preset contributed by a mounted plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPreset {
    /// Display name of the plugin that registered this preset.
    pub plugin: Arc<str>,
    /// The validated preset definition.
    pub preset: Arc<Preset>,
}

/// The presets registered on one [`PluginContext`], in registration order.
pub(crate) type Presets = Listed<PluginPreset>;

impl PluginContext {
    /// Register an animation preset on this context.
    ///
    /// The preset is validated and must have a unique name within the
    /// registering plugin. Dropping the returned [`Registration`] (or
    /// unmounting the plugin that registered it during `mount`) removes the
    /// preset from [`presets`](Self::presets).
    pub fn register_preset(&self, mut preset: Preset) -> Result<Registration, PluginError> {
        preset.name = preset.name.trim().to_string();
        preset
            .validate()
            .map_err(|err| PluginError::PresetRefused(err.to_string()))?;
        let (plugin_id, plugin) = owner(self);
        let name = preset.name.clone();
        let item = PluginPreset {
            plugin,
            preset: Arc::new(preset),
        };
        let guard = self
            .preset_registry()
            .insert(&plugin_id, item, |held| {
                held.preset.name.eq_ignore_ascii_case(&name)
            })
            .ok_or_else(|| {
                PluginError::PresetRefused(format!(
                    "plugin `{}` already registered a preset named `{name}`",
                    if plugin_id.is_empty() {
                        "host"
                    } else {
                        &*plugin_id
                    },
                ))
            })?;
        let contribution = Contribution::new(ContributionKind::Preset, name);
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Snapshot of every currently registered plugin preset, in registration order.
    pub fn presets(&self) -> Vec<PluginPreset> {
        self.preset_registry().items()
    }

    /// Monotonic counter incremented whenever a plugin preset is registered or dropped.
    pub fn presets_revision(&self) -> u64 {
        self.preset_registry().revision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{DataPlugin, MANIFEST_FILE, PRESETS_DIR, Package};
    use crate::plugin::{Plugin, PluginManifest};
    use aria_models::{EffectInstance, Param, ParamValue, PresetProperty};

    struct PresetPlugin {
        manifest: PluginManifest,
        presets: Vec<Preset>,
    }

    impl Plugin for PresetPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            for preset in &self.presets {
                ctx.register_preset(preset.clone())?;
            }
            Ok(())
        }
    }

    #[test]
    fn register_preset_validates_enforces_unique_names_tracks_revision_and_drops_on_unmount() {
        let ctx = PluginContext::new();
        assert_eq!(ctx.presets_revision(), 0);
        assert!(ctx.presets().is_empty());

        let plugin = PresetPlugin {
            manifest: PluginManifest::new("com.example.presets", "Preset Pack", "1.0.0", &[], &[]),
            presets: vec![Preset::new(
                "Warm Vignette",
                vec![EffectInstance::new("vignette")],
                Vec::new(),
                vec![PresetProperty {
                    id: "opacity".into(),
                    curve: Param::Constant(ParamValue::Scalar(0.8)),
                }],
            )],
        };
        ctx.mount(&plugin).unwrap();
        let rev_after_mount = ctx.presets_revision();
        assert!(rev_after_mount > 0);
        let listed = ctx.presets();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].plugin.as_ref(), "Preset Pack");
        assert_eq!(listed[0].preset.name, "Warm Vignette");

        // Duplicate name within the same plugin is refused.
        let scoped = ctx.view(&plugin.manifest);
        let dup = scoped.register_preset(Preset::new(
            "warm vignette",
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
        assert!(
            matches!(dup, Err(PluginError::PresetRefused(msg)) if msg.contains("already registered"))
        );

        // Invalid preset is refused.
        let bad = scoped.register_preset(Preset::new("   ", Vec::new(), Vec::new(), Vec::new()));
        assert!(matches!(bad, Err(PluginError::PresetRefused(msg)) if msg.contains("empty")));

        // Unmounting drops all registrations held by the plugin.
        ctx.unmount("com.example.presets").unwrap();
        assert!(ctx.presets().is_empty());
        assert!(ctx.presets_revision() > rev_after_mount);
    }

    #[test]
    fn package_loads_presets_dir_and_refuses_invalid_preset_naming_file() {
        let scratch = tempfile::tempdir().unwrap();
        let pkg_dir = scratch.path().join("com.example.pkg-presets");
        let presets_dir = pkg_dir.join(PRESETS_DIR);
        std::fs::create_dir_all(&presets_dir).unwrap();
        std::fs::write(
            pkg_dir.join(MANIFEST_FILE),
            "[plugin]\nid = \"com.example.pkg-presets\"\nname = \"Packaged Presets\"\nversion = \"0.1.0\"\napi = 1\n",
        )
        .unwrap();

        let preset = Preset::new(
            "Quick Blur",
            vec![EffectInstance::new("gaussian_blur")],
            Vec::new(),
            Vec::new(),
        );
        let good_path = presets_dir.join("quick-blur.ariapreset");
        std::fs::write(&good_path, preset.to_json().unwrap()).unwrap();

        let pkg = Package::open(&pkg_dir).unwrap();
        assert_eq!(pkg.presets, vec![preset]);

        let ctx = PluginContext::new();
        ctx.mount(&DataPlugin::new(pkg)).unwrap();
        let registered = ctx.presets();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].plugin.as_ref(), "Packaged Presets");
        assert_eq!(registered[0].preset.name, "Quick Blur");
        ctx.unmount("com.example.pkg-presets").unwrap();
        assert!(ctx.presets().is_empty());

        // Corrupt `.ariapreset` refuses the package naming the file.
        let bad_path = presets_dir.join("broken.ariapreset");
        std::fs::write(&bad_path, "{ not json").unwrap();
        let err = Package::open(&pkg_dir).unwrap_err();
        assert_eq!(err.path, bad_path);
        assert!(err.to_string().contains("broken.ariapreset"), "got: {err}");
    }
}
