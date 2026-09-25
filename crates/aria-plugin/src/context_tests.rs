//! Tests for [`PluginContext`].
//!
//! Declared from `context.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;

struct TestService {
    value: u32,
}

#[test]
fn register_get_try_get() {
    let ctx = PluginContext::new();
    let _reg = ctx.register(TestService { value: 42 });
    assert_eq!(ctx.get::<TestService>().value, 42);
    assert!(ctx.try_get::<u64>().is_none());
}

#[test]
fn drop_unregisters() {
    let ctx = PluginContext::new();
    {
        let _reg = ctx.register(TestService { value: 99 });
        assert!(ctx.try_get::<TestService>().is_some());
    }
    assert!(ctx.try_get::<TestService>().is_none());
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ctx.get::<TestService>();
    }));
    assert!(panicked.is_err());
}

struct OtherService;

struct Consumer {
    manifest: PluginManifest,
    /// What the plugin observed while mounting: whether each service resolved.
    saw: std::sync::Mutex<Vec<(&'static str, bool)>>,
}

impl Plugin for Consumer {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let mut saw = self.saw.lock().expect("saw");
        saw.push(("TestService", ctx.try_get::<TestService>().is_some()));
        saw.push(("OtherService", ctx.has::<OtherService>()));
        Ok(())
    }
}

fn consumer(uses: &'static [&'static str]) -> Consumer {
    Consumer {
        manifest: PluginManifest {
            id: "test.consumer",
            name: "Consumer",
            version: "0.1.0",
            requires: &[],
            uses,
        },
        saw: std::sync::Mutex::new(Vec::new()),
    }
}

/// The whole point: a plugin reads what its manifest declares, and an
/// undeclared service is absent even though it is registered.
#[test]
fn a_plugin_reads_only_the_services_it_declares() {
    let ctx = PluginContext::new();
    let _a = ctx.register(TestService { value: 7 });
    let _b = ctx.register(OtherService);

    let plugin = consumer(&["TestService"]);
    ctx.mount(&plugin).expect("mounts");
    let saw = plugin.saw.lock().expect("saw").clone();
    assert_eq!(saw, vec![("TestService", true), ("OtherService", false)]);

    // The host's own context is unscoped, so the registry itself is unchanged.
    assert!(ctx.try_get::<OtherService>().is_some());
}

/// A refusal is data, not a log line: a plugin that forgot a declaration
/// otherwise degrades silently, because a surface with no service hides.
#[test]
fn an_undeclared_service_is_recorded_against_the_plugin_that_asked() {
    let ctx = PluginContext::new();
    let _a = ctx.register(TestService { value: 1 });
    ctx.mount(&consumer(&[])).expect("mounts");

    let denials = ctx.denials();
    assert_eq!(denials.len(), 2, "{denials:?}");
    for denial in &denials {
        assert_eq!(denial.plugin, "test.consumer");
    }
    assert!(
        denials
            .iter()
            .any(|d| short_service_name(d.service) == "TestService"),
        "{denials:?}"
    );
    assert!(
        denials
            .iter()
            .any(|d| short_service_name(d.service) == "OtherService"),
        "{denials:?}"
    );
}

/// Declaring nothing must not be a way to reach everything, and the scope has
/// to survive the clone a handler captures.
#[test]
fn a_clone_of_a_view_keeps_the_scope() {
    let ctx = PluginContext::new();
    let _a = ctx.register(TestService { value: 3 });
    let scoped = ctx.view("test.scoped", &[]);
    let cloned = scoped.clone();
    assert!(scoped.try_get::<TestService>().is_none());
    assert!(cloned.try_get::<TestService>().is_none());
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = cloned.get::<TestService>();
    }));
    assert!(panicked.is_err(), "an undeclared service must fail closed");
}

#[test]
fn a_short_service_name_drops_the_dyn_and_the_path() {
    assert_eq!(
        short_service_name("dyn aria_plugin::service::StorageService"),
        "StorageService"
    );
    assert_eq!(short_service_name("PanelHost"), "PanelHost");
    assert_eq!(
        short_service_name("dyn aria_plugin::Service<a::b::C>"),
        "Service<C>"
    );
    assert_eq!(
        short_service_name("dyn foo::bar::Service<a::b::C, d::e::F>"),
        "Service<C, F>"
    );
    assert_eq!(
        short_service_name(
            "parking_lot::lock_api::Mutex<parking_lot::raw_mutex::RawMutex, aria_engine::Engine>"
        ),
        "Mutex<RawMutex, Engine>"
    );
}

struct PanickingPlugin {
    manifest: PluginManifest,
}

impl Plugin for PanickingPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, _ctx: &PluginContext) -> Result<(), PluginError> {
        panic!("intentional mount panic for test");
    }
}

#[test]
fn panicking_plugin_resets_pending_and_records_failure() {
    let ctx = PluginContext::new();
    let bad = PanickingPlugin {
        manifest: PluginManifest {
            id: "test.panic",
            name: "Panicker",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        },
    };
    let err = ctx.mount(&bad);
    assert!(err.is_err());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "test.panic");
    assert!(failures[0].reason.contains("intentional mount panic"));

    // Next mount must succeed and not be poisoned by pending
    let good = consumer(&[]);
    assert!(ctx.mount(&good).is_ok());
}

#[test]
fn unmount_prunes_denials_and_failures() {
    let ctx = PluginContext::new();
    let _a = ctx.register(TestService { value: 1 });
    let good = consumer(&[]);
    ctx.mount(&good).expect("mounts");
    assert!(!ctx.denials().is_empty());

    ctx.unmount("test.consumer").expect("unmounts");
    assert!(ctx.denials().is_empty());
    assert!(ctx.failures().is_empty());
}
