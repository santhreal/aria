//! What a project keeps about a contribution whose plugin is gone.
//!
//! Closes the class "a project loses the facts it needs to draw a missing
//! plugin's slate and badge": the generator flag is set at every entry point
//! that places an effect, saved only when set, and kept through a save and a
//! load after the plugin leaves; the badge id is the first unknown
//! contribution in the order `docs/spec/plugins.md` states. The catalogs are
//! process-global and shared with the other tests of this crate, so every
//! registered id is unique to its test. Does not cover drawing the slate,
//! which is the renderer's.

use super::*;
use crate::{
    AudioEffectInstance, CatalogKind, EffectInstance, EffectSpec, MissingPlugin, PRESET_VERSION,
    Preset, Sequence, register_effect,
};

fn generator_spec(id: &str) -> EffectSpec {
    EffectSpec {
        id: id.into(),
        label: "Bars".into(),
        category: String::new(),
        params: Vec::new(),
        generator: true,
        stateful: false,
        ..Default::default()
    }
}

/// The instance a plugin command or a preset file hands the model: no flag.
fn unflagged(id: &str) -> EffectInstance {
    serde_json::from_value(serde_json::json!({ "effect_id": id })).expect("parses")
}

fn solid() -> (Project, ClipId) {
    let (project, left, _, _) = project_with_abutting_pair();
    (project, left)
}

fn chain(project: &Project, clip: ClipId) -> &[EffectInstance] {
    &project.timeline().clip(clip).expect("clip").effects
}

#[test]
fn the_generator_flag_is_saved_only_when_set() {
    let plain = EffectInstance::new("gaussian_blur");
    assert!(!plain.generator);
    let json = serde_json::to_value(&plain).unwrap();
    assert!(json.get("generator").is_none(), "{json}");

    let mut flagged = plain;
    flagged.generator = true;
    let json = serde_json::to_value(&flagged).unwrap();
    assert_eq!(json["generator"], serde_json::json!(true));
    let back: EffectInstance = serde_json::from_value(json).unwrap();
    assert_eq!(back, flagged);

    assert!(
        !unflagged("gaussian_blur").generator,
        "absent loads as false"
    );
}

#[test]
fn every_entry_point_marks_a_registered_generator() {
    const ID: &str = "test.missing.entry-gen";
    let guard = register_effect(generator_spec(ID)).expect("registers");

    assert!(EffectInstance::new(ID).generator, "new");

    let (mut project, clip) = solid();
    project.set_clip_effects(clip, vec![unflagged(ID)]).unwrap();
    assert!(chain(&project, clip)[0].generator, "set_clip_effects");

    let (mut project, clip) = solid();
    let preset = Preset {
        version: PRESET_VERSION,
        name: "Bars".into(),
        effects: vec![unflagged(ID)],
        audio_effects: Vec::new(),
        properties: Vec::new(),
    };
    project.apply_preset(clip, &preset, rt(0)).unwrap();
    assert!(chain(&project, clip)[0].generator, "apply_preset");

    let (mut project, clip) = solid();
    project.add_effect(clip, ID).unwrap();
    assert!(!chain(&project, clip)[0].is_missing_generator());

    // The plugin leaves: the saved flag survives a save and a load, and the
    // instance now answers that its generator is missing.
    drop(guard);
    let saved = serde_json::to_string(&project).unwrap();
    let mut loaded: Project = serde_json::from_str(&saved).unwrap();
    let fx = &chain(&loaded, clip)[0];
    assert!(fx.generator);
    assert!(fx.is_missing_generator());
    let mut remarked = fx.clone();
    remarked.mark_from_catalog();
    assert!(remarked.generator, "an unknown id keeps its saved flag");
    // Placing a known effect beside it does not clear the missing flag.
    loaded.add_effect(clip, "gaussian_blur").unwrap();
    assert!(chain(&loaded, clip)[0].is_missing_generator());
}

#[test]
fn a_registered_effect_that_is_no_generator_is_never_missing_generator() {
    const ID: &str = "test.missing.plain-fx";
    let mut spec = generator_spec(ID);
    spec.generator = false;
    let guard = register_effect(spec).expect("registers");
    let mut fx = EffectInstance::new(ID);
    assert!(!fx.generator);
    // A stale flag from an older registration follows the catalog again.
    fx.generator = true;
    fx.mark_from_catalog();
    assert!(!fx.generator);
    drop(guard);
    assert!(!fx.is_missing_generator());
}

#[test]
fn the_badge_states_the_clip_type_then_effects_then_audio_effects() {
    let (mut project, id) = solid();
    let clip = project.timeline_mut().clip_mut(id).unwrap();
    assert_eq!(
        clip.missing_plugin_id(),
        None,
        "a solid with no plugin parts"
    );

    clip.clip_type = Some("com.example.gone.type".into());
    clip.effects = vec![
        EffectInstance::new("gaussian_blur"),
        EffectInstance::new("com.example.gone.fx"),
    ];
    clip.audio_effects = vec![AudioEffectInstance::new("com.example.gone.afx")];
    assert_eq!(clip.missing_plugin_id(), Some("com.example.gone.type"));

    clip.clip_type = None;
    assert_eq!(clip.missing_plugin_id(), Some("com.example.gone.fx"));

    clip.effects.pop();
    assert_eq!(
        clip.missing_plugin_id(),
        Some("com.example.gone.afx"),
        "a known effect is no missing plugin"
    );

    clip.audio_effects = vec![AudioEffectInstance::new(
        crate::audio_effect_catalog()
            .iter()
            .next()
            .expect("shipped")
            .id
            .clone(),
    )];
    assert_eq!(clip.missing_plugin_id(), None);
}

/// `Project::missing_plugins` over every place an id sits: a clip's type,
/// effects, generator, and audio effects, a lane's inserts, a cut's
/// transition, in the root timeline and in a nested sequence. One entry per
/// `(id, kind)`, ordered by id then kind, with one use per instance; a listed
/// id is no entry.
#[test]
fn the_project_lists_each_unlisted_id_once_per_kind_with_its_uses() {
    let (mut project, left) = solid();
    assert_eq!(project.missing_plugins(), Vec::new(), "only shipped ids");

    let mut generator = EffectInstance::new("c.gone");
    generator.generator = true;
    let clip = project.timeline_mut().clip_mut(left).unwrap();
    clip.clip_type = Some("a.gone".into());
    clip.effects = vec![
        EffectInstance::new("c.gone"),
        EffectInstance::new("gaussian_blur"),
        generator,
        EffectInstance::new("c.gone"),
    ];
    clip.audio_effects = vec![AudioEffectInstance::new("b.gone")];
    project.add_transition(left, "crossfade").unwrap();
    for audio in crate::AudioTransition::ALL {
        project.transition_at_mut(left).unwrap().transition_id = audio.id().into();
        let kinds: Vec<_> = (project.missing_plugins().iter())
            .map(|missing| missing.kind)
            .collect();
        assert!(!kinds.contains(&CatalogKind::Transition), "{}", audio.id());
    }
    project.transition_at_mut(left).unwrap().transition_id = "d.gone".into();

    let (mut nested, nested_clip, _, nested_track) = project_with_abutting_pair();
    let lane = nested.timeline_mut().track_mut(nested_track).unwrap();
    lane.audio_effects = vec![AudioEffectInstance::new("b.gone")];
    nested.timeline_mut().clip_mut(nested_clip).unwrap().effects =
        vec![EffectInstance::new("e.gone")];
    let mut sequence = Sequence::new("nest", R24);
    sequence.timeline = nested.timeline().clone();
    project.add_sequence(sequence);

    let entry = |id, kind, uses| MissingPlugin { id, kind, uses };
    assert_eq!(
        project.missing_plugins(),
        vec![
            entry("a.gone", CatalogKind::ClipType, 1),
            entry("b.gone", CatalogKind::AudioEffect, 2),
            entry("c.gone", CatalogKind::Generator, 1),
            entry("c.gone", CatalogKind::Effect, 2),
            entry("d.gone", CatalogKind::Transition, 1),
            entry("e.gone", CatalogKind::Effect, 1),
        ]
    );
}
