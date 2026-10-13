//! Effects and transitions registered while the process runs.
//!
//! Its own test binary: the catalogs are process-wide, and the library tests
//! iterate them while they run. Every test here registers ids no other test
//! uses, so the tests stay independent while they share the catalogs.
//!
//! Covers registration and removal, every refusal rule, catalog order and
//! revisions, lookup, and an effect instance resolving against a registered
//! spec. Does not cover rendering a registered entry, which belongs to the
//! compositor.

use aria_models::{
    EffectInstance, EffectParamKind, EffectParamSpec, EffectSpec, ParamValue, TransitionSpec,
    effect_catalog, effect_spec, register_effect, register_transition,
    shipped_effect_catalog, shipped_transition_catalog, transition_catalog, transition_spec,
};

fn scalar(name: &str, default: f32, min: f32, max: f32) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Scalar,
        default,
        min,
        max,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
    }
}

fn color(name: &str, default: [u8; 4]) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Color,
        default: 0.0,
        min: 0.0,
        max: 0.0,
        default_color: default,
        default_vec2: [0.0; 2],
    }
}

fn effect(id: &str, params: Vec<EffectParamSpec>) -> EffectSpec {
    EffectSpec {
        id: id.into(),
        label: format!("{id} label"),
        params,
        generator: false,
    }
}

fn transition(id: &str) -> TransitionSpec {
    TransitionSpec {
        id: id.into(),
        label: format!("{id} label"),
    }
}

fn ids<T>(catalog: &aria_models::Catalog<T>, id: impl Fn(&T) -> &str) -> Vec<String> {
    catalog.iter().map(|entry| id(entry).to_owned()).collect()
}

#[test]
fn a_registered_effect_is_listed_and_resolves_until_its_guard_drops() {
    let before = effect_catalog();
    let (guard, held) = register_effect(effect(
        "test.listed",
        vec![scalar("amount", 0.5, 0.0, 1.0)],
    ))
    .expect("a namespaced effect registers");
    assert_eq!(held.id, "test.listed");

    let during = effect_catalog();
    assert!(during.revision() > before.revision());
    assert!(during.iter().any(|spec| spec.id == "test.listed"));
    let found = effect_spec("test.listed").expect("a registered id resolves");
    assert!(std::sync::Arc::ptr_eq(&found, &held));

    drop(guard);
    let after = effect_catalog();
    assert!(after.revision() > during.revision());
    assert!(!after.iter().any(|spec| spec.id == "test.listed"));
    assert!(effect_spec("test.listed").is_none());
    // A snapshot taken earlier still lists what it listed.
    assert!(during.iter().any(|spec| spec.id == "test.listed"));
    // The held spec outlives the entry.
    assert_eq!(held.params[0].name, "amount");
}

#[test]
fn a_registered_transition_is_listed_and_resolves_until_its_guard_drops() {
    let (guard, held) =
        register_transition(transition("test.wipe")).expect("a namespaced transition registers");
    assert_eq!(held.label, "test.wipe label");
    assert!(transition_catalog().iter().any(|spec| spec.id == "test.wipe"));
    assert_eq!(transition_spec("test.wipe").unwrap().label, "test.wipe label");
    drop(guard);
    assert!(transition_spec("test.wipe").is_none());
    assert!(!transition_catalog().iter().any(|spec| spec.id == "test.wipe"));
}

#[test]
fn shipped_entries_come_first_then_registered_ones_in_registration_order() {
    let (_a, _) = register_effect(effect("test.order_a", Vec::new())).unwrap();
    let (_b, _) = register_effect(effect("test.order_b", Vec::new())).unwrap();
    let listed = ids(&effect_catalog(), |spec| &spec.id);
    let shipped: Vec<String> = shipped_effect_catalog()
        .iter()
        .map(|spec| spec.id.clone())
        .collect();
    assert_eq!(listed[..shipped.len()], shipped[..], "shipped prefix unchanged");
    let a = listed.iter().position(|id| id == "test.order_a").unwrap();
    let b = listed.iter().position(|id| id == "test.order_b").unwrap();
    assert!(shipped.len() <= a && a < b, "{listed:?}");

    let (_t1, _) = register_transition(transition("test.order_t1")).unwrap();
    let (_t2, _) = register_transition(transition("test.order_t2")).unwrap();
    let listed = ids(&transition_catalog(), |spec| &spec.id);
    let shipped: Vec<String> = shipped_transition_catalog()
        .iter()
        .map(|spec| spec.id.clone())
        .collect();
    assert_eq!(listed[..shipped.len()], shipped[..]);
    let t1 = listed.iter().position(|id| id == "test.order_t1").unwrap();
    let t2 = listed.iter().position(|id| id == "test.order_t2").unwrap();
    assert!(shipped.len() <= t1 && t1 < t2, "{listed:?}");
}

#[test]
fn removing_one_entry_keeps_the_others() {
    let (first, _) = register_effect(effect("test.keep_first", Vec::new())).unwrap();
    let (_second, _) = register_effect(effect("test.keep_second", Vec::new())).unwrap();
    drop(first);
    assert!(effect_spec("test.keep_first").is_none());
    assert!(effect_spec("test.keep_second").is_some());
}

#[test]
fn every_refusal_rule_rejects_and_leaves_the_catalog_unchanged() {
    let shipped_effect = shipped_effect_catalog()[0].id.clone();
    let shipped_transition = shipped_transition_catalog()[0].id.clone();
    let (_held, _) = register_effect(effect("test.taken", Vec::new())).unwrap();
    let (_held_t, _) = register_transition(transition("test.taken_t")).unwrap();

    let refused_effects = [
        ("empty id", effect("", Vec::new()), "needs an id"),
        ("no namespace", effect("bare", Vec::new()), "namespace"),
        (
            "shipped id",
            effect(&shipped_effect, Vec::new()),
            // Shipped ids have no dot, so the namespace rule refuses them first.
            "namespace",
        ),
        ("registered twice", effect("test.taken", Vec::new()), "already in the catalog"),
        (
            "empty label",
            EffectSpec {
                id: "test.unlabelled".into(),
                label: String::new(),
                params: Vec::new(),
                generator: false,
            },
            "no label",
        ),
        (
            "unnamed parameter",
            effect("test.unnamed", vec![scalar("", 0.0, 0.0, 1.0)]),
            "has no name",
        ),
        (
            "repeated parameter",
            effect(
                "test.repeated",
                vec![scalar("x", 0.0, 0.0, 1.0), scalar("x", 0.0, 0.0, 1.0)],
            ),
            "repeats parameter x",
        ),
        (
            "inverted range",
            effect("test.inverted", vec![scalar("x", 0.0, 1.0, 0.0)]),
            "inverted range",
        ),
    ];
    for (case, spec, expected) in refused_effects {
        let id = spec.id.clone();
        let before = effect_spec(&id).map(|held| held.label.clone());
        let error = register_effect(spec).expect_err(case);
        assert!(error.contains(expected), "{case}: {error}");
        assert_eq!(
            effect_spec(&id).map(|held| held.label.clone()),
            before,
            "{case}: a refusal must not change what {id} resolves to"
        );
    }

    let refused_transitions = [
        ("no namespace", transition("bare"), "namespace"),
        ("shipped id", transition(&shipped_transition), "namespace"),
        ("registered twice", transition("test.taken_t"), "already in the catalog"),
        (
            "empty label",
            TransitionSpec {
                id: "test.unlabelled_t".into(),
                label: String::new(),
            },
            "no label",
        ),
    ];
    for (case, spec, expected) in refused_transitions {
        let error = register_transition(spec).expect_err(case);
        assert!(error.contains(expected), "{case}: {error}");
    }
    assert!(transition_spec("test.unlabelled_t").is_none());
}

#[test]
fn an_inverted_colour_range_is_not_a_refusal() {
    // Colour components carry no range, so min and max are not read.
    let mut tint = color("tint", [255, 0, 0, 255]);
    tint.min = 1.0;
    tint.max = 0.0;
    let registered = register_effect(effect("test.colour_range", vec![tint]));
    assert!(registered.is_ok(), "{:?}", registered.err());
}

#[test]
fn an_instance_of_a_registered_effect_samples_validates_and_writes() {
    let (_guard, _) = register_effect(effect(
        "test.instance",
        vec![scalar("amount", 0.25, 0.0, 2.0), color("tint", [10, 20, 30, 255])],
    ))
    .unwrap();
    let mut instance = EffectInstance::new("test.instance");
    assert!(instance.validate().is_ok());
    assert_eq!(instance.sample_param("amount", 0.0), Some(0.25));
    assert_eq!(
        instance.sample_color_param("tint", 0.0),
        Some([10, 20, 30, 255])
    );

    instance.set_param_constant(0, 1.5).unwrap();
    assert_eq!(instance.sample_param("amount", 0.0), Some(1.5));
    assert!(
        instance.set_param_constant(0, 3.0).is_err(),
        "the registered range applies to writes"
    );
    assert!(
        instance
            .set_param_value_constant(1, ParamValue::Scalar(1.0))
            .is_err(),
        "the registered kind applies to writes"
    );
    assert!(instance.validate().is_ok());
}

#[test]
fn an_instance_whose_effect_is_removed_resolves_to_nothing_and_keeps_its_values() {
    let (guard, _) =
        register_effect(effect("test.removed", vec![scalar("amount", 0.0, 0.0, 1.0)])).unwrap();
    let mut instance = EffectInstance::new("test.removed");
    instance.set_param_constant(0, 0.75).unwrap();
    drop(guard);

    assert!(instance.spec().is_err());
    assert!(instance.validate().is_err());
    assert_eq!(instance.sample_param("amount", 0.0), None);
    assert!(instance.set_param_constant(0, 0.5).is_err());
    assert!(
        instance.params.contains_key("amount"),
        "the stored value survives the removal"
    );

    // Registering the id again resolves the same instance with its value.
    let (_again, _) =
        register_effect(effect("test.removed", vec![scalar("amount", 0.0, 0.0, 1.0)])).unwrap();
    assert_eq!(instance.sample_param("amount", 0.0), Some(0.75));
}

#[test]
fn concurrent_registration_and_lookup_finish_with_every_guard_removed() {
    let threads: Vec<_> = (0..8)
        .map(|n| {
            std::thread::spawn(move || {
                for round in 0..50 {
                    let id = format!("test.concurrent_{n}_{round}");
                    let (guard, _) = register_effect(effect(&id, Vec::new())).unwrap();
                    assert!(effect_spec(&id).is_some());
                    let _ = effect_catalog().len();
                    drop(guard);
                    assert!(effect_spec(&id).is_none());
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("a registering thread panicked");
    }
    assert!(
        !effect_catalog()
            .iter()
            .any(|spec| spec.id.starts_with("test.concurrent_"))
    );
}
