//! Tests for [`PluginContext`].
//!
//! Declared from `context.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;
use crate::editor_events::{PluginActivated, PluginDeactivated};
use crate::plugin::Plugin;

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
    saw: parking_lot::Mutex<Vec<(&'static str, bool)>>,
}

impl Plugin for Consumer {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let mut saw = self.saw.lock();
        saw.push(("TestService", ctx.try_get::<TestService>().is_some()));
        saw.push(("OtherService", ctx.has::<OtherService>()));
        Ok(())
    }
}

fn consumer(uses: &'static [&'static str]) -> Consumer {
    Consumer {
        manifest: PluginManifest::new("test.consumer", "Consumer", "0.1.0", &[], uses),
        saw: parking_lot::Mutex::new(Vec::new()),
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
    let saw = plugin.saw.lock().clone();
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
    let scoped = ctx.view(&PluginManifest::new(
        "test.scoped",
        "Scoped",
        "0.1.0",
        &[],
        &[],
    ));
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
        manifest: PluginManifest::new("test.panic", "Panicker", "0.1.0", &[], &[]),
    };
    let err = ctx.mount(&bad);
    assert!(err.is_err());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "test.panic");
    assert!(failures[0].reason.contains("intentional mount panic"));

    // Next mount must succeed after the failed one
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
    let seen: std::sync::Arc<parking_lot::Mutex<Vec<(&str, String)>>> = Default::default();
    let _activated = {
        let seen = std::sync::Arc::clone(&seen);
        ctx.on_event::<PluginActivated, _>(move |e| {
            seen.lock().push(("activated", e.id.to_string()));
        })
    };
    let _deactivated = {
        let seen = std::sync::Arc::clone(&seen);
        ctx.on_event::<PluginDeactivated, _>(move |e| {
            seen.lock().push(("deactivated", e.id.to_string()));
        })
    };

    let bad = PanickingPlugin {
        manifest: PluginManifest::new("test.panic", "Panicker", "0.1.0", &[], &[]),
    };
    assert!(ctx.mount(&bad).is_err());
    let good = consumer(&[]);
    ctx.mount(&good).expect("mounts");
    ctx.unmount("test.consumer").expect("unmounts");

    assert!(ctx.events().flush(std::time::Duration::from_secs(10)));
    assert_eq!(
        *seen.lock(),
        [
            ("activated", "test.consumer".to_owned()),
            ("deactivated", "test.consumer".to_owned())
        ]
    );
}

/// The mounted revision moves once for every change to the mounted set and
/// for nothing else: a mount that took, an unmount and a failure each move it
/// by one; a mount that failed, and an unmount or a failure of an id that is
/// not mounted, leave it. A host keys what it derives from the set on this, so
/// a missed move serves a stale derivation and a spurious one rebuilds for
/// nothing.
#[test]
fn the_mounted_revision_moves_once_per_change_to_the_mounted_set() {
    let ctx = PluginContext::new();
    let mut last = ctx.mounted_revision();
    let mut step = |what: &str, moved: bool, ctx: &PluginContext| {
        let now = ctx.mounted_revision();
        let want = if moved { last + 1 } else { last };
        assert_eq!(now, want, "{what}: the revision went from {last} to {now}");
        last = now;
    };

    let bad = PanickingPlugin {
        manifest: PluginManifest::new("test.panic", "Panicker", "0.1.0", &[], &[]),
    };
    assert!(ctx.mount(&bad).is_err());
    step("a mount that failed", false, &ctx);
    ctx.mount(&consumer(&[])).expect("mounts");
    step("a mount that took", true, &ctx);
    assert!(ctx.unmount("test.absent").is_err());
    step("an unmount of an id that is not mounted", false, &ctx);
    ctx.unmount("test.consumer").expect("unmounts");
    step("an unmount", true, &ctx);
    ctx.mount(&consumer(&[])).expect("mounts again");
    step("a second mount", true, &ctx);
    assert!(!ctx.fail("test.absent", "not mounted"));
    step("a failure of an id that is not mounted", false, &ctx);
    assert!(ctx.fail("test.consumer", "faulted"));
    step("a failure", true, &ctx);
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
            category: String::new(),
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
                ..Default::default()
            }],
            generator: false,
            stateful: false,
            ..Default::default()
        },
        passes: vec![fs.into()],
    }
}

fn transition_definition(id: &str) -> aria_compositor::TransitionDefinition {
    aria_compositor::TransitionDefinition {
        spec: aria_models::TransitionSpec {
            id: id.into(),
            label: "Mix".into(),
            ..Default::default()
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
            return Err(PluginError::MountFailed(
                self.manifest.id.clone(),
                "asked to".into(),
            ));
        }
        Ok(())
    }
}

fn passes(id: &'static str, effect: &'static str, transition: &'static str, fail: bool) -> Passes {
    Passes {
        manifest: PluginManifest::new(id, "Passes", "0.1.0", &[], &[]),
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
    assert!(
        matches!(&err, PluginError::MountFailed(id, _) if id == "test.failing"),
        "{err}"
    );
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
    assert!(
        !effect_listed(id),
        "dropping the registration kept the effect"
    );
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

/// A recovered fault is kept per mounted plugin, newest only, and goes with
/// the plugin. Failing a plugin drops its registrations, states the reason in
/// the failure ledger, and a later mount clears it.
#[test]
fn faults_and_failures_follow_the_plugin() {
    let ctx = PluginContext::new();
    let _a = ctx.register(TestService { value: 3 });
    let plugin = consumer(&["TestService"]);
    ctx.mount(&plugin).expect("mounts");
    ctx.report_fault("test.consumer", "first");
    ctx.report_fault("test.consumer", "second");
    ctx.report_fault("test.elsewhere", "not mounted");
    assert_eq!(
        ctx.faults(),
        vec![PluginFault {
            plugin: "test.consumer".into(),
            message: "second".into(),
        }]
    );

    assert!(ctx.fail("test.consumer", "it broke"));
    assert!(ctx.plugins().is_empty());
    assert!(ctx.faults().is_empty());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(
        (failures[0].id.as_str(), failures[0].name.as_str()),
        ("test.consumer", "Consumer")
    );
    assert_eq!(failures[0].reason, "it broke");
    assert!(
        !ctx.fail("test.consumer", "again"),
        "it is no longer mounted"
    );
    assert_eq!(ctx.failures().len(), 1);

    ctx.mount(&plugin).expect("mounts again");
    assert!(ctx.failures().is_empty());
}

/// Runtime frame and audio-block costs are tracked per plugin id without
/// cross-talk and cleared when a plugin unmounts or fails.
#[test]
fn costs_record_per_plugin_do_not_mix_and_clear_on_unmount_and_fail() {
    struct NoopPlugin {
        manifest: PluginManifest,
    }
    impl Plugin for NoopPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        fn mount(&self, _ctx: &PluginContext) -> Result<(), PluginError> {
            Ok(())
        }
    }

    let ctx = PluginContext::new();
    let alpha = NoopPlugin {
        manifest: PluginManifest::new("test.cost.alpha", "Alpha", "0.1.0", &[], &[]),
    };
    let beta = NoopPlugin {
        manifest: PluginManifest::new("test.cost.beta", "Beta", "0.1.0", &[], &[]),
    };
    ctx.mount(&alpha).expect("mounts alpha");
    ctx.mount(&beta).expect("mounts beta");

    let initial = ctx.costs();
    assert_eq!(initial.len(), 2);
    assert!(initial[0].frame.is_empty() && initial[0].block.is_empty());
    assert!(initial[1].frame.is_empty() && initial[1].block.is_empty());

    ctx.record_frame_cost("test.cost.alpha", Duration::from_millis(4));
    ctx.record_frame_cost("test.cost.alpha", Duration::from_millis(12));
    ctx.record_block_cost("test.cost.beta", Duration::from_micros(80));
    ctx.record_frame_cost("test.cost.unmounted", Duration::from_millis(50));

    let alpha_cost = ctx.cost_of("test.cost.alpha").expect("alpha cost");
    assert_eq!(alpha_cost.frame.calls, 2);
    assert_eq!(alpha_cost.frame.last, Duration::from_millis(12));
    assert_eq!(alpha_cost.frame.mean, Duration::from_millis(5));
    assert_eq!(alpha_cost.frame.max, Duration::from_millis(12));
    assert!(alpha_cost.block.is_empty());

    let beta_cost = ctx.cost_of("test.cost.beta").expect("beta cost");
    assert!(beta_cost.frame.is_empty());
    assert_eq!(beta_cost.block.calls, 1);
    assert_eq!(beta_cost.block.last, Duration::from_micros(80));
    assert_eq!(beta_cost.block.mean, Duration::from_micros(80));
    assert_eq!(beta_cost.block.max, Duration::from_micros(80));

    let mounted = ctx.mounted();
    assert_eq!(mounted[0].frame_cost, alpha_cost.frame);
    assert_eq!(mounted[0].block_cost, alpha_cost.block);
    assert_eq!(mounted[1].frame_cost, beta_cost.frame);
    assert_eq!(mounted[1].block_cost, beta_cost.block);

    ctx.unmount("test.cost.alpha").expect("unmounts alpha");
    assert!(ctx.cost_of("test.cost.alpha").is_none());
    ctx.mount(&alpha).expect("remounts alpha");
    let alpha_fresh = ctx.cost_of("test.cost.alpha").expect("fresh alpha cost");
    assert!(alpha_fresh.frame.is_empty());
    assert!(alpha_fresh.block.is_empty());

    assert!(ctx.fail("test.cost.beta", "processor crashed"));
    assert!(ctx.cost_of("test.cost.beta").is_none());
    ctx.mount(&beta).expect("remounts beta");
    let beta_fresh = ctx.cost_of("test.cost.beta").expect("fresh beta cost");
    assert!(beta_fresh.frame.is_empty());
    assert!(beta_fresh.block.is_empty());
}
