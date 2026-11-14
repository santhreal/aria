//! Which source frame each side of a picture transition reads.
//!
//! WHY: the class closed is "a picture transition reads source media past a
//! clip's edge". Each side plays live inside its own clip and holds its edge
//! frame elsewhere in the window: the outgoing clip holds its last frame past
//! the cut, and the incoming clip holds its first frame before it. Both clips
//! here sit inside a longer file, so a side that read on into its unused
//! media would read a tick outside its source window. The cases cover every
//! alignment in `TransitionAlignment::ALL` at two window lengths, every tick
//! of the timeline, and that the blend covers exactly the window. Not caught:
//! the decode of the resolved tick, which the look-ahead and decoder suites
//! cover.
use super::*;
use aria_models::{MediaId, TransitionAlignment, transition_window};

/// Source window start of the outgoing clip in the file.
const OUT_SOURCE: i64 = 100;
/// Source window start of the incoming clip in the file.
const IN_SOURCE: i64 = 300;
/// Timeline length of each clip, and the cut between them.
const CLIP: i64 = 48;

fn media_tick(layer: &SceneLayer) -> (MediaId, i64) {
    match &layer.source {
        LayerSource::Media { media, source_time } => (*media, source_time.value),
        other => panic!("a transition side read {other:?}"),
    }
}

#[test]
fn each_side_of_a_picture_transition_holds_its_edge_frame_outside_its_clip() {
    let mut project = Project::new("edges", FPS_24);
    let file = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let left = project
        .add_clip(track, file, tr(OUT_SOURCE, CLIP), rt(0))
        .expect("place the outgoing clip");
    project
        .add_clip(track, file, tr(IN_SOURCE, CLIP), rt(CLIP))
        .expect("place the incoming clip");
    project
        .add_transition(left, "crossfade")
        .expect("a crossfade at the cut");
    for alignment in TransitionAlignment::ALL {
        for duration in [24, CLIP] {
            project
                .set_transition_alignment(left, alignment)
                .expect("align the window");
            project
                .set_transition_duration(left, duration)
                .expect("the clips hold the window");
            let (start, end) = transition_window(alignment, duration, CLIP);
            let mut blended = 0;
            for t in 0..2 * CLIP {
                let scene = resolve(&project, rt(t)).expect("the scene resolves");
                let [layer] = &scene.layers[..] else {
                    panic!(
                        "{alignment:?} {duration}: tick {t} resolved {} layers",
                        scene.layers.len()
                    );
                };
                let LayerSource::Transition {
                    outgoing, incoming, ..
                } = &layer.source
                else {
                    assert!(
                        !(start..end).contains(&t),
                        "{alignment:?} {duration}: tick {t} inside the window did not blend"
                    );
                    continue;
                };
                assert!(
                    (start..end).contains(&t),
                    "{alignment:?} {duration}: tick {t} outside the window blended"
                );
                blended += 1;
                assert_eq!(
                    media_tick(outgoing),
                    (file, OUT_SOURCE + t.min(CLIP - 1)),
                    "{alignment:?} {duration}: outgoing side at tick {t}"
                );
                assert_eq!(
                    media_tick(incoming),
                    (file, IN_SOURCE + (t - CLIP).max(0)),
                    "{alignment:?} {duration}: incoming side at tick {t}"
                );
            }
            assert_eq!(blended, duration, "{alignment:?}: blended ticks");
        }
    }
}
