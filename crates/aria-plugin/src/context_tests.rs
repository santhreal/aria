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

/// A mount that took posts `PluginActivated` and an unmount posts
/// `PluginDeactivated`; a mount that failed posts neither.
#[test]
fn mounting_and_unmounting_announce_the_plugin() {
    let ctx = PluginContext::new();
    let seen: std::sync::Arc<std::sync::Mutex<Vec<(&str, &str)>>> = Default::default();
    let _activated = {
        let seen = std::sync::Arc::clone(&seen);
        ctx.on_event::<PluginActivated, _>(move |e| {
            seen.lock().expect("seen").push(("activated", e.id));
        })
    };
    let _deactivated = {
        let seen = std::sync::Arc::clone(&seen);
        ctx.on_event::<PluginDeactivated, _>(move |e| {
            seen.lock().expect("seen").push(("deactivated", e.id));
        })
    };

    let bad = PanickingPlugin {
        manifest: PluginManifest {
            id: "test.panic",
            name: "Panicker",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        },
    };
    assert!(ctx.mount(&bad).is_err());
    let good = consumer(&[]);
    ctx.mount(&good).expect("mounts");
    ctx.unmount("test.consumer").expect("unmounts");

    assert!(ctx.events().flush(std::time::Duration::from_secs(10)));
    assert_eq!(
        *seen.lock().expect("seen"),
        [("activated", "test.consumer"), ("deactivated", "test.consumer")]
    );
}

/// A plugin that adds one effect and one transition while it mounts, and fails
/// the mount after adding them when `fail` is set.
struct Passes {
    manifest: PluginManifest,
    effect: &'static str,
    transition: &'static str,
    fail: bool,
}

const EFFECT_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv) * param_gain();
}";

const TRANSITION_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(outgoing_tex, aria_sampler, in.uv);
    let b = textureSample(incoming_tex, aria_sampler, in.uv);
    return mix(a, b, aria.progress);
}";

fn effect_definition(id: &str, fs: &str) -> aria_compositor::EffectDefinition {
    aria_compositor::EffectDefinition {
        spec: aria_models::EffectSpec {
            id: id.into(),
            label: "Gain".into(),
            params: vec![aria_models::EffectParamSpec {
                name: "gain".into(),
                label: "Gain".into(),
                kind: aria_models::EffectParamKind::Scalar,
                default: 1.0,
                min: 0.0,
                max: 2.0,
                default_color: [0; 4],
                default_vec2: [0.0; 2],
            }],
            generator: false,
        },
        passes: vec![fs.into()],
    }
}

fn transition_definition(id: &str) -> aria_compositor::TransitionDefinition {
    aria_compositor::TransitionDefinition {
        spec: aria_models::TransitionSpec {
            id: id.into(),
            label: "Mix".into(),
        },
        shader: TRANSITION_FS.into(),
    }
}

impl Plugin for Passes {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // Retained by the context, so the returned guards are empty.
        ctx.register_effect(effect_definition(self.effect, EFFECT_FS))?;
        ctx.register_transition(transition_definition(self.transition))?;
        if self.fail {
            return Err(PluginError::MountFailed(self.manifest.id, "asked to".into()));
        }
        Ok(())
    }
}

fn passes(id: &'static str, effect: &'static str, transition: &'static str, fail: bool) -> Passes {
    Passes {
        manifest: PluginManifest {
            id,
            name: "Passes",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        },
        effect,
        transition,
        fail,
    }
}

/// Whether the catalog lists `id` and the compositor draws it.
fn effect_listed(id: &str) -> bool {
    let listed = aria_models::effect_spec(id).is_some();
    assert_eq!(
        listed,
        aria_compositor::renders_effect(id),
        "the catalog and the compositor disagree about {id}"
    );
    listed
}

fn transition_listed(id: &str) -> bool {
    let listed = aria_models::transition_spec(id).is_some();
    assert_eq!(
        listed,
        aria_compositor::renders_transition(id),
        "the catalog and the compositor disagree about {id}"
    );
    listed
}

/// What a plugin registers while it mounts lives as long as the plugin: listed
/// and drawn after the mount, gone after the unmount.
#[test]
fn a_mounted_plugins_effect_and_transition_last_until_it_unmounts() {
    let ctx = PluginContext::new();
    let (effect, transition) = ("test.ctx.mounted_gain", "test.ctx.mounted_mix");
    ctx.mount(&passes("test.passes", effect, transition, false))
        .expect("mounts");
    assert!(effect_listed(effect), "the effect left with the mount call");
    assert!(transition_listed(transition));

    ctx.unmount("test.passes").expect("unmounts");
    assert!(!effect_listed(effect), "the effect outlived its plugin");
    assert!(!transition_listed(transition));
}

/// A mount that fails after registering leaves nothing behind.
#[test]
fn a_failed_mount_removes_what_it_registered() {
    let ctx = PluginContext::new();
    let (effect, transition) = ("test.ctx.failed_gain", "test.ctx.failed_mix");
    let err = ctx
        .mount(&passes("test.failing", effect, transition, true))
        .expect_err("asked to fail");
    assert!(matches!(err, PluginError::MountFailed("test.failing", _)), "{err}");
    assert!(!effect_listed(effect), "a failed mount left its effect");
    assert!(!transition_listed(transition));
}

/// Outside a mount the caller holds the guard, and dropping it removes the
/// entry.
#[test]
fn outside_a_mount_the_caller_holds_the_registration() {
    let ctx = PluginContext::new();
    let id = "test.ctx.held_gain";
    let held = ctx
        .register_effect(effect_definition(id, EFFECT_FS))
        .expect("registers");
    assert!(effect_listed(id));
    drop(held);
    assert!(!effect_listed(id), "dropping the registration kept the effect");
}

/// A refused definition is reported as a refusal naming the pass and line, and
/// registers nothing.
#[test]
fn a_refused_shader_is_a_register_refusal() {
    let ctx = PluginContext::new();
    let id = "test.ctx.broken_gain";
    let broken = "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return nope;\n}";
    let Err(PluginError::RegisterRefused(refusal)) =
        ctx.register_effect(effect_definition(id, broken))
    else {
        panic!("a shader naming an unknown identifier registered");
    };
    assert_eq!(refusal.id, id);
    assert_eq!(refusal.pass, Some(0));
    assert_eq!(refusal.location.map(|(line, _)| line), Some(3));
    assert!(!effect_listed(id));
}
