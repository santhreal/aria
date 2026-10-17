//! What a clip draws when effects in its chain are bypassed.
//!
//! Closes the class "a bypassed effect still renders, or bypassing one effect
//! drops or reorders another": every subset of a three-effect chain is bypassed
//! in turn and the layer must carry exactly the effects left on, in chain
//! order, with the parameters each was set to. The effect ids come from the
//! shipped pass table, so a renamed pass changes what is driven without an edit
//! here. A bypassed generator whose plugin is missing draws the clip's own
//! picture rather than the missing-plugin slate. Does not cover the GPU passes
//! themselves; the scene is what they receive.

use super::*;

/// A Sticker track holding one opaque red solid over `[0, 48)`.
fn solid_project() -> (Project, ClipId) {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 48),
        )
        .unwrap();
    (project, clip)
}

/// The first three shipped effects, each with its first scalar parameter
/// moved off its default so a pass that swapped places with another shows.
fn chain() -> Vec<EffectInstance> {
    let mut effects: Vec<EffectInstance> = aria_compositor::shipped_effect_descriptors()
        .iter()
        .take(3)
        .map(|descriptor| EffectInstance::new(descriptor.id))
        .collect();
    assert_eq!(effects.len(), 3, "the pass table ships three effects");
    for effect in &mut effects {
        let spec = effect.spec().expect("a shipped pass has a catalog entry");
        if let Some(slot) = spec
            .params
            .iter()
            .position(|param| param.kind == aria_models::EffectParamKind::Scalar)
        {
            let param = &spec.params[slot];
            let value = param.min + (param.max - param.min) * 0.37;
            effect.set_param_constant(slot, value).unwrap();
        }
    }
    effects
}

#[test]
fn a_layer_carries_exactly_the_effects_left_on_in_chain_order() {
    let effects = chain();
    let (mut project, clip) = solid_project();
    project.timeline_mut().clip_mut(clip).unwrap().effects = effects.clone();
    let all = resolve(&project, rt(5)).unwrap().layers[0].effects.clone();
    assert_eq!(all.len(), 3, "every shipped effect resolves to a pass");

    for mask in 0u8..8 {
        let mut bypassed = effects.clone();
        for (index, effect) in bypassed.iter_mut().enumerate() {
            effect.bypass = mask & (1 << index) != 0;
        }
        project.timeline_mut().clip_mut(clip).unwrap().effects = bypassed;
        let scene = resolve(&project, rt(5)).unwrap();
        let expected: Vec<_> = all
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) == 0)
            .map(|(_, pass)| pass.clone())
            .collect();
        assert_eq!(scene.layers[0].effects, expected, "bypass mask {mask:03b}");
    }
}

#[test]
fn a_bypassed_missing_generator_draws_the_clip_rather_than_the_slate() {
    let (mut project, clip) = solid_project();
    let mut gone = EffectInstance::new("com.example.gone.bypassed-generator");
    gone.generator = true;
    gone.bypass = true;
    project.timeline_mut().clip_mut(clip).unwrap().effects = vec![gone];

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers[0].source, LayerSource::Solid([255, 0, 0, 255]));
    assert!(scene.layers[0].effects.is_empty(), "nothing left to run");

    project.timeline_mut().clip_mut(clip).unwrap().effects[0].bypass = false;
    let scene = resolve(&project, rt(5)).unwrap();
    assert!(
        matches!(&scene.layers[0].source, LayerSource::Text { .. }),
        "switched back on, the missing generator draws its slate: {:?}",
        scene.layers[0].source
    );
}
