//! Nested sequence cases: a sequence clip places the sequence's whole frame,
//! every visible lane of it, with the clip's own placement and look.
//!
//! The class this closes: a sequence clip resolved to the topmost clip under
//! the playhead inside it and dropped every lane beneath, and a sequence that
//! showed itself through another recursed without bound. What this does not
//! catch: the pixels a nest draws, which `tests/render_frame.rs` reads back.

use super::*;
use aria_models::{Clip, Mask, MaskKind, ModelError, SequenceId};

fn sized(size: [u32; 2]) -> CanvasSettings {
    CanvasSettings {
        resolution: Some(size),
        ..CanvasSettings::default()
    }
}

/// A project on a `root` canvas holding a sequence on a `nest` canvas, whose
/// lanes hold a 1920×1080 video and a red solid, placed on the root's only
/// lane over ticks 0..100.
fn nested(root: [u32; 2], nest: [u32; 2]) -> (Project, ClipId, SequenceId) {
    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(sized(root));
    let media = project.add_media(video(1920, 1080));
    let sequence = project.create_sequence("Nest", FPS_24);
    project.set_active_sequence(Some(sequence)).unwrap();
    project.timeline_mut().set_canvas(sized(nest));
    let v1 = project.add_track(TrackKind::Video, "V1");
    project.add_clip(v1, media, tr(0, 100), rt(0)).unwrap();
    let s1 = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            s1,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project.set_active_sequence(None).unwrap();
    let lane = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_sequence_clip(lane, sequence, tr(0, 100), rt(0))
        .unwrap();
    (project, clip, sequence)
}

/// The nest the root scene places at `t`: its layer, scene, and frame.
fn nest_at(project: &Project, t: i64) -> (SceneLayer, Scene, [f32; 4]) {
    let scene = resolve(project, rt(t)).unwrap();
    assert_eq!(
        scene.layers.len(),
        1,
        "the root shows the one sequence clip"
    );
    let layer = scene.layers[0].clone();
    let LayerSource::Nest { scene, frame } = &layer.source else {
        panic!("a sequence clip places a nest, got {:?}", layer.source);
    };
    let (scene, frame) = ((**scene).clone(), *frame);
    (layer, scene, frame)
}

fn approx4(a: [f32; 4], b: [f32; 4]) {
    approx2([a[0], a[1]], [b[0], b[1]]);
    approx2([a[2], a[3]], [b[2], b[3]]);
}

#[test]
fn a_nest_holds_every_visible_lane_of_its_sequence() {
    let (project, clip, _) = nested([1920, 1080], [1920, 1080]);
    let (layer, scene, frame) = nest_at(&project, 5);
    assert_eq!(layer.clip, Some(clip));
    let mut kinds: Vec<&str> = scene.layers.iter().map(|l| l.source.kind()).collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["media", "solid"], "both lanes reach the nest");
    assert_eq!(scene.background, [0, 0, 0, 0], "a nest draws on nothing");
    approx4(frame, [0.0, 0.0, 1.0, 1.0]);
    let LayerSource::Media { source_time, .. } = scene
        .layers
        .iter()
        .find(|l| l.source.kind() == "media")
        .unwrap()
        .source
    else {
        unreachable!()
    };
    assert_eq!(source_time, rt(5), "the sequence reads its own clock");
}

#[test]
fn a_hidden_lane_of_a_sequence_stays_out_of_its_nest() {
    let (mut project, _, sequence) = nested([1920, 1080], [1920, 1080]);
    project.set_active_sequence(Some(sequence)).unwrap();
    let solid_lane = project
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == TrackKind::Sticker)
        .unwrap()
        .id;
    project
        .timeline_mut()
        .track_mut(solid_lane)
        .unwrap()
        .enabled = false;
    project.set_active_sequence(None).unwrap();
    let (_, scene, _) = nest_at(&project, 5);
    let kinds: Vec<&str> = scene.layers.iter().map(|l| l.source.kind()).collect();
    assert_eq!(kinds, ["media"]);
}

#[test]
fn a_nest_takes_its_clips_placement_and_look() {
    let (mut project, clip, _) = nested([1920, 1080], [1920, 1080]);
    project
        .set_transform(
            clip,
            ClipTransform {
                position: [0.25, 0.0],
                scale: Scale2::uniform(0.5),
                opacity: 0.5,
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    project
        .add_clip_mask(clip, Mask::new(MaskKind::Circle))
        .unwrap();
    project.set_blend_mode(clip, BlendMode::Screen).unwrap();
    let (layer, _, _) = nest_at(&project, 5);
    assert_eq!(layer.size, SizeSpec::Fixed([960.0, 540.0]));
    approx2(layer.center, [1920.0 * 0.75, 540.0]);
    approx(layer.opacity, 0.5);
    assert_eq!(layer.masks.first().map(|m| m.kind), Some(MaskKind::Circle));
    assert_eq!(layer.blend_mode, BlendMode::Screen);
    approx4(layer.uv, [0.0, 0.0, 1.0, 1.0]);
}

/// A sequence larger than the canvas it is placed on resolves at that
/// canvas; one of another shape fills a centered frame of it.
#[test]
fn a_nest_fits_its_sequence_into_the_canvas_it_is_placed_on() {
    for (nest, frame, size) in [
        ([3840, 2160], [0.0, 0.0, 1.0, 1.0], [1920.0, 1080.0]),
        ([1080, 1080], [0.21875, 0.0, 0.78125, 1.0], [1080.0, 1080.0]),
        ([960, 540], [0.0, 0.0, 1.0, 1.0], [1920.0, 1080.0]),
    ] {
        let (project, _, _) = nested([1920, 1080], nest);
        let (layer, scene, got) = nest_at(&project, 5);
        assert_eq!((scene.width, scene.height), (1920, 1080), "{nest:?}");
        approx4(got, frame);
        approx4(layer.uv, frame);
        assert_eq!(layer.size, SizeSpec::Fixed(size), "{nest:?}");
        let media = scene
            .layers
            .iter()
            .find(|l| l.source.kind() == "media")
            .unwrap();
        approx2(media.center, [960.0, 540.0]);
    }
}

/// A sequence that shows itself through another is refused by name; the
/// model refuses to place one, so the cycle is written past it.
#[test]
fn a_sequence_that_shows_itself_is_a_cycle_not_a_recursion() {
    let (mut project, _, outer) = nested([1920, 1080], [1920, 1080]);
    let inner = project.create_sequence("Inner", FPS_24);
    project.set_active_sequence(Some(outer)).unwrap();
    let lane = project.add_track(TrackKind::Video, "V3");
    project
        .add_sequence_clip(lane, inner, tr(0, 100), rt(0))
        .unwrap();
    project.set_active_sequence(Some(inner)).unwrap();
    let lane = project.add_track(TrackKind::Video, "V1");
    project
        .timeline_mut()
        .add_clip(lane, Clip::from_sequence(outer, tr(0, 100), tr(0, 100)))
        .unwrap();
    project.set_active_sequence(None).unwrap();
    match resolve(&project, rt(5)) {
        Err(ModelError::SequenceCycle(id)) => assert_eq!(id, outer),
        other => panic!("expected the cycle through {outer:?}, got {other:?}"),
    }
}

#[test]
fn a_sequence_with_nothing_at_the_playhead_places_nothing() {
    let (project, _, _) = nested([1920, 1080], [1920, 1080]);
    let mut project = project;
    let sequence = project.create_sequence("Empty", FPS_24);
    let lane = project.add_track(TrackKind::Video, "V2");
    project
        .add_sequence_clip(lane, sequence, tr(0, 100), rt(0))
        .unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1, "the empty sequence adds no layer");
}
