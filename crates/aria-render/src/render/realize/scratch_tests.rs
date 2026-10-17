use super::*;
use crate::scene::SizeSpec;

#[test]
fn frame_scratch_retains_its_container_allocations() {
    let mut scratch = RealizeScratch::default();
    scratch.realized.reserve(8);
    scratch.scene_indices.reserve(8);
    scratch.levels.push(0..1);
    scratch.item_levels.push(0..1);
    let realized = scratch.realized.as_ptr();
    let scene_indices = scratch.scene_indices.as_ptr();
    let levels = scratch.levels.as_ptr();
    let item_levels = scratch.item_levels.as_ptr();

    scratch.begin_frame();

    assert_eq!(scratch.realized.as_ptr(), realized);
    assert_eq!(scratch.scene_indices.as_ptr(), scene_indices);
    assert_eq!(scratch.levels.as_ptr(), levels);
    assert_eq!(scratch.item_levels.as_ptr(), item_levels);
    assert!(scratch.realized.is_empty() && scratch.scene_indices.is_empty());
    assert!(scratch.levels.is_empty() && scratch.item_levels.is_empty());
}

fn layer(source: LayerSource, effect: f32) -> SceneLayer {
    SceneLayer {
        clip: None,
        source,
        center: [8.0, 8.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([16.0, 16.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: vec![crate::scene::ResolvedPass::new(
            "gaussian_blur",
            vec![effect],
        )],
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

fn effect_value(effects: &[PassInstance]) -> f32 {
    effects[0].params[0]
}

#[test]
fn omitted_text_does_not_shift_the_next_plain_layers_effects() {
    let empty_text = layer(
        LayerSource::Text {
            content: String::new(),
            style: aria_text::TextStyle::new(24.0),
            animation: None,
        },
        1.0,
    );
    let visible = layer(LayerSource::Solid([255, 255, 255, 255]), 2.0);
    let mut scene = crate::scene::Scene::empty(16, 16, [0, 0, 0, 255]);
    scene.layers = vec![empty_text, visible];

    let effects = scene.drawn(1).effects.as_slice();

    assert_eq!(effect_value(effects), 2.0);
    assert_eq!(
        effects[0].params.as_ptr(),
        scene.layers[1].effects[0].params.as_ptr()
    );

    // A reference draws first, so every canvas layer's draw index moves by one.
    scene.references = vec![layer(LayerSource::Solid([0, 0, 0, 255]), 3.0)];
    assert_eq!(effect_value(&scene.drawn(0).effects), 3.0);
    assert_eq!(effect_value(&scene.drawn(2).effects), 2.0);
}

#[test]
fn transition_side_effects_remain_associated() {
    let outgoing = layer(LayerSource::Solid([255, 0, 0, 255]), 3.0);
    let incoming = layer(LayerSource::Solid([0, 0, 255, 255]), 4.0);
    let wrapper = layer(
        LayerSource::Transition {
            outgoing: Box::new(outgoing),
            incoming: Box::new(incoming),
            transition_id: "dissolve".into(),
            progress: 0.5,
        },
        99.0,
    );

    let (outgoing, incoming) = transition_effects(&wrapper);

    assert_eq!(effect_value(outgoing), 3.0);
    assert_eq!(effect_value(incoming), 4.0);
}

#[test]
fn matte_side_effects_remain_associated() {
    let content = layer(LayerSource::Solid([255, 0, 0, 255]), 5.0);
    let stencil = layer(LayerSource::Solid([0, 0, 0, 255]), 6.0);
    let wrapper = layer(
        LayerSource::Matte {
            layers: Box::new([content, stencil]),
            read: MatteSource::Alpha,
            invert: false,
            softness: 0.0,
        },
        99.0,
    );

    let (content, stencil) = matte_effects(&wrapper);

    assert_eq!(effect_value(content), 5.0);
    assert_eq!(effect_value(stencil), 6.0);
}
