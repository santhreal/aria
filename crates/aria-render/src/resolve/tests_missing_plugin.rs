//! What a clip draws when the plugin behind one of its contributions is gone.
//!
//! Closes the class "a project whose plugin is missing, failed, or disabled
//! draws something other than what `docs/spec/plugins.md` states": a generator
//! draws a slate stating its id, a transition plays as a cut, and an effect
//! passes its input through. Every id here is one no catalog lists, so no test
//! registers into the process-global catalogs. Does not cover the timeline
//! badge; that is the editor's projection.

use super::*;

const GONE_GENERATOR: &str = "com.example.gone.generator";

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

fn chain(project: &mut Project, clip: ClipId, effects: Vec<EffectInstance>) {
    project.timeline_mut().clip_mut(clip).unwrap().effects = effects;
}

fn gone(generator: bool) -> EffectInstance {
    let mut fx = EffectInstance::new(GONE_GENERATOR);
    fx.generator = generator;
    fx
}

#[test]
fn a_missing_generator_draws_a_slate_stating_its_id() {
    let (mut project, clip) = solid_project();
    chain(&mut project, clip, vec![gone(true)]);

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert_eq!(scene.layers[0].clip, Some(clip));
    match &scene.layers[0].source {
        LayerSource::Text { content, style, .. } => {
            assert_eq!(content, &format!("Missing generator\n{GONE_GENERATOR}"));
            assert_eq!(style.color, [232, 232, 232, 255]);
            assert!(style.background.is_some(), "the slate is an opaque card");
        }
        other => panic!("expected the slate, got {other:?}"),
    }
}

#[test]
fn the_slate_follows_the_saved_flag_through_a_save_and_a_load() {
    let (mut project, clip) = solid_project();
    chain(&mut project, clip, vec![gone(true)]);
    let saved = serde_json::to_string(&project).unwrap();
    let project: Project = serde_json::from_str(&saved).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert!(
        matches!(&scene.layers[0].source, LayerSource::Text { content, .. } if content.ends_with(GONE_GENERATOR)),
        "{:?}",
        scene.layers[0].source
    );
}

#[test]
fn a_missing_effect_that_was_no_generator_passes_the_picture_through() {
    let (mut project, clip) = solid_project();
    chain(&mut project, clip, vec![gone(false)]);

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert_eq!(scene.layers[0].source, LayerSource::Solid([255, 0, 0, 255]));
    assert!(scene.layers[0].effects.is_empty(), "nothing left to run");
}

#[test]
fn a_transition_no_catalog_lists_plays_as_a_cut() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 24),
        )
        .unwrap();
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(24, 24),
        )
        .unwrap();
    project.add_transition(left, "crossfade").unwrap();
    project.set_transition_duration(left, 24).unwrap();
    // The saved id of a transition whose plugin is gone, through the load
    // path a project from another machine takes.
    let mut doc = serde_json::to_value(&project).unwrap();
    doc["timeline"]["tracks"][0][1]["transitions"][0]["transition_id"] =
        serde_json::json!("com.example.gone.wipe");
    let project: Project = serde_json::from_value(doc).unwrap();

    // Both halves of the window [12, 36): the side under the playhead alone.
    for (tick, rgba) in [
        (12, [255, 0, 0, 255]),
        (23, [255, 0, 0, 255]),
        (24, [0, 0, 255, 255]),
        (35, [0, 0, 255, 255]),
    ] {
        let scene = resolve(&project, rt(tick)).unwrap();
        assert_eq!(scene.layers.len(), 1, "tick {tick}");
        assert_eq!(
            scene.layers[0].source,
            LayerSource::Solid(rgba),
            "tick {tick}"
        );
    }
}
