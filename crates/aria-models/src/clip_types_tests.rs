//! The clip type catalog and the type a clip records.
//!
//! Closes the class "a clip type is listed, saved or drawn on terms the
//! registration did not set": every refusal rule, the guard that removes the
//! entry, the availability answer for each way a generator can fail to draw
//! the type, and the id a clip keeps through a save and a load whether or not
//! the type is registered. The catalog is process-global and shared with the
//! other tests of this crate, so every id here is unique to its test. Does not
//! cover placing a typed clip or drawing its handles; those are the editor's.

use crate::{
    CLIP_TYPE_ID_MAX_BYTES, CLIP_TYPE_MAX_HANDLES, CLIP_TYPE_NAME_MAX_BYTES, ClipTypeSpec,
    EffectParamKind, EffectParamSpec, EffectSpec, Generator, MarkerColor, ModelError, Project,
    Rational, TimeRange, clip_type_catalog, clip_type_spec, register_clip_type, register_effect,
};

fn spec(id: &str, generator: &str) -> ClipTypeSpec {
    ClipTypeSpec::new(id, "Lower third", generator, MarkerColor::Orange)
}

fn param(name: &str, kind: EffectParamKind) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind,
        default: 0.0,
        min: 0.0,
        max: 1.0,
        default_color: [0; 4],
        default_vec2: [0.5, 0.5],
        ..Default::default()
    }
}

fn effect(id: &str, generator: bool) -> EffectSpec {
    EffectSpec {
        id: id.into(),
        category: String::new(),
        label: "Bar".into(),
        params: vec![
            param("anchor", EffectParamKind::Vec2),
            param("height", EffectParamKind::Scalar),
        ],
        generator,
        stateful: false,
        ..Default::default()
    }
}

fn refusal(spec: ClipTypeSpec) -> String {
    match register_clip_type(spec) {
        Ok(_) => panic!("registered"),
        Err(reason) => reason,
    }
}

#[test]
fn a_registered_type_is_listed_until_its_guard_drops() {
    let before = clip_type_catalog().revision();
    let (guard, held) =
        register_clip_type(spec("test.ct.listed", "test.ct.listed-bar")).expect("registers");
    assert_eq!(held.label, "Lower third");
    let listed = clip_type_catalog();
    assert!(listed.revision() > before);
    assert!(listed.iter().any(|entry| entry.id == "test.ct.listed"));
    assert_eq!(
        clip_type_spec("test.ct.listed").as_deref(),
        Some(&*held),
        "the lookup returns the entry the catalog holds"
    );
    let registered = clip_type_catalog().revision();
    drop(guard);
    assert_eq!(clip_type_spec("test.ct.listed"), None);
    assert!(clip_type_catalog().revision() > registered);
}

#[test]
fn every_rule_refuses_with_its_reason() {
    let long_id = format!("test.{}", "x".repeat(CLIP_TYPE_ID_MAX_BYTES));
    let cases: Vec<(ClipTypeSpec, &str)> = vec![
        (spec("plain", "test.g"), "needs a namespace"),
        (spec("", "test.g"), "needs an id"),
        (spec(&long_id, "test.g"), "limited to 128 bytes"),
        (spec("test.ct.two words", "test.g"), "no spaces"),
        (
            ClipTypeSpec::new("test.ct.unnamed", "", "test.g", MarkerColor::Red),
            "has no label",
        ),
        (
            ClipTypeSpec::new("test.ct.blank", "   ", "test.g", MarkerColor::Red),
            "has no name",
        ),
        (
            ClipTypeSpec::new(
                "test.ct.wordy",
                "n".repeat(CLIP_TYPE_NAME_MAX_BYTES + 1),
                "test.g",
                MarkerColor::Red,
            ),
            "name is limited",
        ),
        (spec("test.ct.nogen", " "), "names no generator"),
        (
            ClipTypeSpec {
                handles: (0..=CLIP_TYPE_MAX_HANDLES)
                    .map(|i| format!("h{i}"))
                    .collect(),
                ..spec("test.ct.crowded", "test.g")
            },
            "the limit is 8",
        ),
        (
            ClipTypeSpec {
                handles: vec!["anchor".into(), "anchor".into()],
                ..spec("test.ct.twice", "test.g")
            },
            "declared twice",
        ),
        (
            ClipTypeSpec {
                handles: vec![String::new()],
                ..spec("test.ct.emptyhandle", "test.g")
            },
            "names no parameter",
        ),
    ];
    for (entry, reason) in cases {
        let id = entry.id.clone();
        let refused = refusal(entry);
        assert!(
            refused.contains(reason),
            "{id}: {refused:?} lacks {reason:?}"
        );
    }
}

#[test]
fn an_id_is_listed_once() {
    let _first = register_clip_type(spec("test.ct.once", "test.g")).expect("registers");
    assert!(refusal(spec("test.ct.once", "test.g")).contains("already in the catalog"));
}

#[test]
fn availability_follows_the_generator() {
    let mut typed = spec("test.ct.avail", "test.ct.avail-bar");
    typed.handles = vec!["anchor".into()];
    let missing = typed.unavailable().expect("no generator yet");
    assert!(missing.contains("is not registered"), "{missing}");

    let generator = register_effect(effect("test.ct.avail-bar", true)).expect("registers");
    assert_eq!(typed.unavailable(), None);

    let mut scalar = typed.clone();
    scalar.handles = vec!["height".into()];
    let wrong = scalar.unavailable().expect("a scalar is no handle");
    assert!(
        wrong.contains("`height` names no vec2 parameter"),
        "{wrong}"
    );
    let mut absent = typed.clone();
    absent.handles = vec!["corner".into()];
    assert!(absent.unavailable().is_some());

    drop(generator);
    assert!(
        typed.unavailable().is_some(),
        "the generator left with its plugin"
    );

    let _plain = register_effect(effect("test.ct.avail-plain", false)).expect("registers");
    let not_generator = spec("test.ct.avail2", "test.ct.avail-plain")
        .unavailable()
        .expect("an effect is no generator");
    assert!(not_generator.contains("is an effect, not a generator"));
}

#[test]
fn a_clip_keeps_its_type_through_a_save_unregistered() {
    let mut project = Project::new("types", Rational::FPS_24);
    let lane = Generator::SolidColor { rgba: [0; 4] }.spec().lane;
    let track = project.add_track(lane, "V1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor { rgba: [0; 4] },
            TimeRange::at_rate(0, 48, Rational::FPS_24),
        )
        .expect("places");
    let plain = project.save_to_string().expect("saves");
    assert!(
        !plain.contains("clip_type"),
        "an untyped clip saves no type"
    );

    let before = project
        .set_clip_type(clip, Some("com.example.never-installed".into()))
        .expect("sets");
    assert_eq!(before, None);
    let loaded =
        Project::load_from_string(&project.save_to_string().expect("saves")).expect("loads");
    assert_eq!(
        loaded.clip(clip).and_then(|c| c.clip_type.as_deref()),
        Some("com.example.never-installed")
    );

    let cleared = project.set_clip_type(clip, None).expect("clears");
    assert_eq!(cleared.as_deref(), Some("com.example.never-installed"));
}

#[test]
fn a_malformed_type_id_is_refused_on_the_clip() {
    let mut project = Project::new("types", Rational::FPS_24);
    let lane = Generator::SolidColor { rgba: [0; 4] }.spec().lane;
    let track = project.add_track(lane, "V1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor { rgba: [0; 4] },
            TimeRange::at_rate(0, 48, Rational::FPS_24),
        )
        .expect("places");
    for id in [
        String::new(),
        "a b".to_owned(),
        "a\u{7}".to_owned(),
        "x".repeat(CLIP_TYPE_ID_MAX_BYTES + 1),
    ] {
        let refused = project.set_clip_type(clip, Some(id.clone()));
        assert!(
            matches!(refused, Err(ModelError::InvalidParam(_))),
            "{id:?} was {refused:?}"
        );
    }
    assert_eq!(project.clip(clip).and_then(|c| c.clip_type.clone()), None);
    assert!(matches!(
        project.set_clip_type(crate::ClipId::from_raw(u64::MAX), None),
        Err(ModelError::UnknownClip(_))
    ));
}
