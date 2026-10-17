//! Layer-reference parameters: an effect may read another clip's picture, but
//! never its own clip's, never a clip the sequence lacks, and never through a
//! chain that leads back to itself, because the renderer would recurse.
//!
//! The cases set references through `Project::set_effect_instance`, the one
//! setter every command reaches. A diamond (two paths to the same clip) is
//! not a cycle and must stay accepted. Each case registers its own effect id,
//! because the catalog is process-wide and cases run in parallel.

use super::*;
use crate::effects::{EffectInstance, EffectParamSpec, EffectSpec};

struct Fixture {
    project: Project,
    clips: [ClipId; 3],
    media: MediaId,
    track: TrackId,
    effect: &'static str,
    /// Keeps the effect registered. A test binds it by name (`_kept`): a `..`
    /// rest pattern leaves it in the temporary, which drops at the end of the
    /// `let` and unregisters the effect before the test reads it.
    _registered: crate::Registered,
}

/// Register `effect` with one layer parameter and place three clips on one
/// video lane, each carrying one instance of it.
fn three_clips_with(effect: &'static str) -> Fixture {
    let (registered, _) = crate::register_effect(EffectSpec {
        id: effect.into(),
        label: "Layer ref".into(),
        category: "Stylize".into(),
        params: vec![EffectParamSpec::layer("source", "Source")],
        ..Default::default()
    })
    .expect("register the layer-ref effect");
    let (mut project, media, track) = project_with_media(300);
    let clips = [0, 1, 2].map(|n| {
        let clip = project
            .add_clip(track, media, tr(n * 50, 50), rt(n * 50))
            .expect("add clip");
        project.add_effect(clip, effect).expect("add effect");
        clip
    });
    Fixture {
        project,
        clips,
        media,
        track,
        effect,
        _registered: registered,
    }
}

/// Point the layer parameter of `clip`'s effect at `index` to `target`.
fn point(
    project: &mut Project,
    clip: ClipId,
    index: usize,
    target: ClipId,
) -> Result<(), ModelError> {
    let mut effect: EffectInstance = project.effect_instance(clip, index).unwrap().clone();
    effect.set_layer_param(0, Some(target)).unwrap();
    project.set_effect_instance(clip, index, effect)
}

#[test]
fn a_reference_to_another_clip_is_kept() {
    let Fixture {
        mut project,
        clips: [a, b, _],
        _registered: _kept,
        ..
    } = three_clips_with("test.models.layer_ref_kept");
    point(&mut project, a, 0, b).expect("a reads b");
    let stored = project.effect_instance(a, 0).unwrap();
    assert_eq!(stored.layer_params.get("source"), Some(&b.raw()));
}

#[test]
fn a_clip_cannot_read_itself() {
    let Fixture {
        mut project,
        clips: [a, ..],
        _registered: _kept,
        ..
    } = three_clips_with("test.models.layer_ref_self");
    let err = point(&mut project, a, 0, a).unwrap_err().to_string();
    assert!(err.contains("cannot reference itself"), "{err}");
    assert!(
        project
            .effect_instance(a, 0)
            .unwrap()
            .layer_params
            .is_empty()
    );
}

#[test]
fn a_missing_clip_is_refused() {
    let Fixture {
        mut project,
        clips: [a, ..],
        _registered: _kept,
        ..
    } = three_clips_with("test.models.layer_ref_missing");
    let gone = ClipId::from_raw(9_999);
    let err = point(&mut project, a, 0, gone).unwrap_err().to_string();
    assert!(err.contains("references missing clip 9999"), "{err}");
}

#[test]
fn a_chain_back_to_the_clip_is_a_cycle_at_every_length() {
    let Fixture {
        mut project,
        clips: [a, b, c],
        _registered: _kept,
        ..
    } = three_clips_with("test.models.layer_ref_cycle");
    // Two clips: a → b, then b → a.
    point(&mut project, a, 0, b).unwrap();
    let err = point(&mut project, b, 0, a).unwrap_err().to_string();
    assert!(err.contains("layer-reference cycle"), "{err}");
    // Three clips: a → b → c, then c → a.
    point(&mut project, b, 0, c).unwrap();
    let err = point(&mut project, c, 0, a).unwrap_err().to_string();
    assert!(err.contains("layer-reference cycle"), "{err}");
    assert!(
        project
            .effect_instance(c, 0)
            .unwrap()
            .layer_params
            .is_empty()
    );
}

#[test]
fn a_diamond_is_not_a_cycle() {
    let Fixture {
        mut project,
        clips: [a, b, c],
        media,
        track,
        effect,
        _registered: _kept,
        ..
    } = three_clips_with("test.models.layer_ref_diamond");
    // a → b → c and a → c: c is reached twice from a.
    point(&mut project, b, 0, c).unwrap();
    point(&mut project, a, 0, b).unwrap();
    project.add_effect(a, effect).unwrap();
    point(&mut project, a, 1, c).expect("a reads c beside b");
    // A fourth clip reading a walks c twice and closes no loop.
    let d = project
        .add_clip(track, media, tr(150, 50), rt(150))
        .unwrap();
    project.add_effect(d, effect).unwrap();
    point(&mut project, d, 0, a).expect("a diamond under d is not a cycle");
}
