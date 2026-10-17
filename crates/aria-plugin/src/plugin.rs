//! Plugin trait and manifest.

use std::borrow::Cow;

use crate::context::PluginContext;
use crate::error::PluginError;

/// Metadata describing a plugin: its id, dependencies, version, and the services
/// it is allowed to consume.
///
/// A plugin compiled into the app borrows every string from its own static
/// data ([`PluginManifest::new`]); one read from a package owns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub version: Cow<'static, str>,
    /// Plugins that must be mounted first. `aria_plugin::load_order` derives the
    /// mount order from this, so a host lists its plugins in any order.
    pub requires: Names,
    /// Short names of the services this plugin reads, e.g. `"StorageService"`.
    ///
    /// The context handed to [`Plugin::mount`] resolves only these, so a plugin
    /// cannot reach a capability it never declared: asking for one gets `None`
    /// and is recorded as a denial the host can show. Declaring nothing means
    /// consuming nothing: a plugin that only registers services and handlers
    /// needs no entry here.
    pub uses: Names,
}

impl PluginManifest {
    /// A manifest of static strings, for a plugin compiled into the app. Held
    /// in a `static`, since a manifest has owned forms and so is not promoted
    /// to a constant.
    pub const fn new(
        id: &'static str,
        name: &'static str,
        version: &'static str,
        requires: &'static [&'static str],
        uses: &'static [&'static str],
    ) -> Self {
        Self {
            id: Cow::Borrowed(id),
            name: Cow::Borrowed(name),
            version: Cow::Borrowed(version),
            requires: Names::Static(requires),
            uses: Names::Static(uses),
        }
    }
}

/// A list of plugin or service names: static for a compiled plugin, owned for
/// one read from a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Names {
    Static(&'static [&'static str]),
    Owned(Vec<String>),
}

impl Names {
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        let statics: &[&'static str] = match self {
            Names::Static(names) => names,
            Names::Owned(_) => &[],
        };
        let owned: &[String] = match self {
            Names::Owned(names) => names,
            Names::Static(_) => &[],
        };
        statics
            .iter()
            .copied()
            .chain(owned.iter().map(String::as_str))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.iter().any(|held| held == name)
    }

    pub fn len(&self) -> usize {
        match self {
            Names::Static(names) => names.len(),
            Names::Owned(names) => names.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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
            manifest: PluginManifest::new("test.dummy", "Dummy", "0.1.0", &[], &[]),
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
            manifest: PluginManifest::new("test.dep", "Dep", "0.1.0", &["missing.plugin"], &[]),
        };
        assert!(ctx.mount(&plugin).is_err());
    }

    /// A package's owned manifest mounts, scopes and orders like a static one.
    #[test]
    fn an_owned_manifest_mounts_like_a_static_one() {
        let ctx = PluginContext::new();
        let plugin = DummyPlugin {
            manifest: PluginManifest {
                id: Cow::Owned("com.example.owned".to_owned()),
                name: Cow::Owned("Owned".to_owned()),
                version: Cow::Owned("1.0.0".to_owned()),
                requires: Names::Owned(Vec::new()),
                uses: Names::Owned(vec!["DummyService".to_owned()]),
            },
        };
        ctx.mount(&plugin).unwrap();
        assert_eq!(ctx.plugins(), vec!["com.example.owned"]);
        assert_eq!(ctx.mounted()[0].manifest, plugin.manifest);
    }

    #[test]
    fn names_iterate_in_order_in_both_forms() {
        let statics = Names::Static(&["a", "b"]);
        let owned = Names::Owned(vec!["a".to_owned(), "b".to_owned()]);
        for names in [&statics, &owned] {
            assert_eq!(names.iter().collect::<Vec<_>>(), ["a", "b"]);
            assert!(names.contains("b"));
            assert!(!names.contains("c"));
            assert_eq!(names.len(), 2);
        }
        assert!(Names::Static(&[]).is_empty());
        assert!(Names::Owned(Vec::new()).is_empty());
    }
}
