//! Plugin trait and manifest.

use crate::context::PluginContext;
use crate::error::PluginError;

/// Metadata describing a plugin: its id, dependencies, version, and the services
/// it is allowed to consume.
#[derive(Debug, Clone, Copy)]
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    /// Plugins that must be mounted first. `aria_plugin::load_order` derives the
    /// mount order from this, so a host lists its plugins in any order.
    pub requires: &'static [&'static str],
    /// Short names of the services this plugin reads, e.g. `"StorageService"`.
    ///
    /// The context handed to [`Plugin::mount`] resolves only these, so a plugin
    /// cannot reach a capability it never declared: asking for one gets `None`
    /// and is recorded as a denial the host can show. Declaring nothing means
    /// consuming nothing — a plugin that only registers services and handlers
    /// needs no entry here.
    pub uses: &'static [&'static str],
}

/// A plugin: a unit of capability that registers services into the context.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError>;
    fn unmount(&self, _ctx: &PluginContext) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::PluginContext;

    struct DummyService;

    struct DummyPlugin {
        manifest: PluginManifest,
    }

    impl Plugin for DummyPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            ctx.register(DummyService);
            Ok(())
        }
    }

    #[test]
    fn mount_and_unmount() {
        let ctx = PluginContext::new();
        let plugin = DummyPlugin {
            manifest: PluginManifest {
                id: "test.dummy",
                name: "Dummy",
                version: "0.1.0",
                requires: &[],
                uses: &[],
            },
        };
        ctx.mount(&plugin).unwrap();
        assert!(ctx.try_get::<DummyService>().is_some());
        assert_eq!(ctx.plugins(), vec!["test.dummy"]);
        ctx.unmount("test.dummy").unwrap();
        assert!(ctx.try_get::<DummyService>().is_none());
    }

    #[test]
    fn dependency_not_satisfied() {
        let ctx = PluginContext::new();
        let plugin = DummyPlugin {
            manifest: PluginManifest {
                id: "test.dep",
                name: "Dep",
                version: "0.1.0",
                requires: &["missing.plugin"],
                uses: &[],
            },
        };
        assert!(ctx.mount(&plugin).is_err());
    }
}
