//! Look-ahead contracts: every media use a scene draws maps to its own decoder
//! key, clip entrances are found in the playback direction within the window,
//! playback through decode-ahead shows the same pixels as frames decoded cold,
//! a clip about to come on screen has its decoder open before it does, and
//! idle decoders drop their frames and close past the cap.
//!
//! Not covered: hardware decode paths (the fixture decodes in software on the
//! test hosts) and the decode pool's timing.

use std::path::PathBuf;

use aria_compositor::RgbaImage;
use aria_models::{
    ClipId, MediaId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind,
};

use super::*;
use crate::scene::{LayerSource, Scene, SceneLayer, SizeSpec};

const FPS_24: Rational = Rational::FPS_24;
const FPS_30: Rational = Rational::FPS_30;

fn layer(clip: Option<ClipId>, source: LayerSource) -> SceneLayer {
    SceneLayer {
        clip,
        source,
        center: [8.0, 8.0],
        anchor_point: [0.5, 0.5],
        size: SizeSpec::Fixed([16.0, 16.0]),
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: aria_models::BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

fn media_layer(media: MediaId, clip: ClipId, tick: i64) -> SceneLayer {
    layer(
        Some(clip),
        LayerSource::Media {
            media,
            source_time: RationalTime::new(tick, FPS_24),
        },
    )
}

fn uses(layers: &[SceneLayer]) -> Vec<(DecoderKey, i64)> {
    let mut found = Vec::new();
    media_uses(layers, &mut |key, time| found.push((key, time.value)));
    found
}

#[test]
fn each_clip_of_one_media_has_its_own_key_inside_wrappers() {
    let media = MediaId::from_raw(7);
    let [a, b, c, d] = [1, 2, 3, 4].map(ClipId::from_raw);
    let transition = layer(
        None,
        LayerSource::Transition {
            outgoing: Box::new(media_layer(media, a, 10)),
            incoming: Box::new(media_layer(media, b, 0)),
            transition_id: "crossfade".into(),
            progress: 0.5,
        },
    );
    let matte = layer(
        None,
        LayerSource::Matte {
            layers: Box::new([media_layer(media, c, 5), media_layer(media, d, 6)]),
            read: aria_models::MatteSource::Alpha,
            invert: false,
            softness: 0.0,
        },
    );
    let still = layer(Some(a), LayerSource::Still { media });
    let key = |clip| DecoderKey {
        media,
        clip: Some(clip),
    };
    assert_eq!(
        uses(&[transition, still, matte]),
        [(key(a), 10), (key(b), 0), (key(c), 5), (key(d), 6)]
    );
}

/// A nest decodes the media its own scene shows, references included.
#[test]
fn a_nest_reads_the_media_of_its_scene() {
    let media = MediaId::from_raw(7);
    let [a, b] = [5, 6].map(ClipId::from_raw);
    let mut scene = Scene::empty(16, 16, [0, 0, 0, 0]);
    scene.references.push(media_layer(media, a, 3));
    scene.layers.push(media_layer(media, b, 4));
    let nest = layer(
        None,
        LayerSource::Nest {
            scene: Box::new(scene),
            frame: [0.0, 0.0, 1.0, 1.0],
        },
    );
    let key = |clip| DecoderKey {
        media,
        clip: Some(clip),
    };
    assert_eq!(uses(&[nest]), [(key(a), 3), (key(b), 4)]);
}

#[test]
fn a_pool_preview_layer_is_keyed_apart_from_timeline_clips() {
    let media = MediaId::from_raw(3);
    let clip = ClipId::from_raw(9);
    let pool = layer(
        None,
        LayerSource::Media {
            media,
            source_time: RationalTime::new(0, FPS_24),
        },
    );
    let found = uses(&[pool, media_layer(media, clip, 0)]);
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].0, found[1].0);
}

/// V1: `a` 0..48, `b` 48..96 with a crossfade at the cut (36..60), `c`
/// 200..248. An audio lane's clip at 30 is not an entrance.
fn lane_project() -> Project {
    let mut project = Project::new("look ahead", FPS_24);
    let media = project.add_media(MediaSource::new("/v.mp4", 64, 36, FPS_24, 1000, false));
    let track = project.add_track(TrackKind::Video, "V1");
    let range = |start, duration| TimeRange::at_rate(start, duration, FPS_24);
    let at = |tick| RationalTime::new(tick, FPS_24);
    let a = project.add_clip(track, media, range(0, 48), at(0)).unwrap();
    project
        .add_clip(track, media, range(100, 48), at(48))
        .unwrap();
    project
        .add_clip(track, media, range(300, 48), at(200))
        .unwrap();
    project.add_transition(a, "crossfade").unwrap();
    let audio = project.add_track(TrackKind::Audio, "A1");
    let sound = project.add_media(MediaSource::new("/a.wav", 0, 0, FPS_24, 1000, true));
    project
        .add_clip(audio, sound, range(0, 10), at(30))
        .unwrap();
    project
}

fn entrances_from(project: &Project, tick: i64, step: i64) -> Vec<i64> {
    let mut found = Vec::new();
    entrances(project.timeline(), tick, step, &mut found);
    found
}

#[test]
fn entrances_are_the_clip_and_transition_starts_ahead_nearest_first() {
    let project = lane_project();
    // The window is two seconds: 48 ticks at 24 fps.
    assert_eq!(entrances_from(&project, 10, 1), [36, 48]);
    assert_eq!(entrances_from(&project, 0, 8), [36, 48]);
    assert_eq!(entrances_from(&project, 151, 1), Vec::<i64>::new());
    assert_eq!(entrances_from(&project, 152, 1), [200]);
    // A start the playhead is on has already entered.
    assert_eq!(entrances_from(&project, 200, 1), Vec::<i64>::new());
}

#[test]
fn reverse_entrances_are_the_last_frames_behind() {
    let project = lane_project();
    // Going backward a clip enters at its last frame, a transition at its own.
    assert_eq!(entrances_from(&project, 120, -1), [95]);
    assert_eq!(entrances_from(&project, 70, -2), [59, 47]);
    assert_eq!(entrances_from(&project, 250, -1), [247]);
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4")
}

/// The fixture twice on one lane: clip `a` shows source 0..30 at 0..30, clip
/// `b` source 45..75 at 30..60.
fn fixture_project() -> (Project, MediaId, ClipId, ClipId) {
    let mut project = Project::new("decode ahead", FPS_30);
    let path = fixture();
    let media = project.add_media(MediaSource::new(path, 640, 360, FPS_30, 90, false));
    let track = project.add_track(TrackKind::Video, "V1");
    let range = |start, duration| TimeRange::at_rate(start, duration, FPS_30);
    let at = |tick| RationalTime::new(tick, FPS_30);
    let a = project.add_clip(track, media, range(0, 30), at(0)).unwrap();
    let b = project
        .add_clip(track, media, range(45, 30), at(30))
        .unwrap();
    (project, media, a, b)
}

fn frame(renderer: &mut Renderer, project: &Project, tick: i64) -> RgbaImage {
    renderer
        .render_frame_fit(project, RationalTime::new(tick, FPS_30), 160, 90)
        .unwrap_or_else(|error| panic!("frame {tick}: {error}"))
}

/// Each tick of `ticks` rendered by a renderer that only ever jumps, so no
/// frame is looked ahead at: the frames a cold decode shows.
fn cold_frames(project: &Project, ticks: impl Iterator<Item = i64>) -> Vec<(i64, RgbaImage)> {
    let Ok(mut renderer) = Renderer::new_headless() else {
        return Vec::new();
    };
    let mut ticks: Vec<i64> = ticks.collect();
    // A stride past MAX_STEP visits every tick in an order with no playback
    // step in it.
    let count = ticks.len();
    let stride = (MAX_STEP as usize + 9..)
        .find(|stride| gcd(*stride, count) == 1)
        .unwrap_or(1);
    let order: Vec<i64> = (0..count)
        .map(|index| ticks[index * stride % count])
        .collect();
    let mut frames: Vec<(i64, RgbaImage)> = order
        .into_iter()
        .map(|tick| (tick, frame(&mut renderer, project, tick)))
        .collect();
    frames.sort_by_key(|(tick, _)| *tick);
    ticks.sort_unstable();
    assert_eq!(
        frames.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
        ticks
    );
    frames
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn assert_same(played: &RgbaImage, cold: &RgbaImage, tick: i64, how: &str) {
    assert_eq!(
        (played.width, played.height),
        (cold.width, cold.height),
        "{how} frame {tick} size"
    );
    assert!(
        played.pixels == cold.pixels,
        "{how} frame {tick} differs from the frame a cold decode shows"
    );
}

#[test]
fn playback_shows_the_frames_a_cold_decode_shows() {
    let (project, _, _, _) = fixture_project();
    let Ok(mut renderer) = Renderer::new_headless() else {
        return;
    };
    let cold = cold_frames(&project, 0..60);
    for (tick, expected) in &cold {
        assert_same(
            &frame(&mut renderer, &project, *tick),
            expected,
            *tick,
            "forward",
        );
    }
    let Ok(mut reverse) = Renderer::new_headless() else {
        return;
    };
    for (tick, expected) in cold.iter().rev() {
        assert_same(
            &frame(&mut reverse, &project, *tick),
            expected,
            *tick,
            "reverse",
        );
    }
    let Ok(mut shuttle) = Renderer::new_headless() else {
        return;
    };
    for (tick, expected) in cold.iter().step_by(4) {
        assert_same(
            &frame(&mut shuttle, &project, *tick),
            expected,
            *tick,
            "4x shuttle",
        );
    }
}

#[test]
fn a_clip_coming_on_screen_opens_before_it_does() {
    let (project, media, a, b) = fixture_project();
    let Ok(mut renderer) = Renderer::new_headless() else {
        return;
    };
    let key = |clip| DecoderKey {
        media,
        clip: Some(clip),
    };
    frame(&mut renderer, &project, 0);
    let open: Vec<DecoderKey> = renderer.decoder_keys().copied().collect();
    assert_eq!(open, [key(a)], "a first frame has no step to look along");
    frame(&mut renderer, &project, 1);
    let mut open: Vec<DecoderKey> = renderer.decoder_keys().copied().collect();
    open.sort_by_key(|key| key.clip);
    assert_eq!(open, [key(a), key(b)], "clip b enters within two seconds");
    // A jump looks nowhere: a fresh renderer jumping into clip a opens only a.
    let Ok(mut jumping) = Renderer::new_headless() else {
        return;
    };
    frame(&mut jumping, &project, 20);
    frame(&mut jumping, &project, 5);
    let open: Vec<DecoderKey> = jumping.decoder_keys().copied().collect();
    assert_eq!(open, [key(a)]);
}

#[test]
fn idle_decoders_release_their_frames_and_close_past_the_cap() {
    let (project, media, a, _) = fixture_project();
    let Ok(mut renderer) = Renderer::new_headless() else {
        return;
    };
    for tick in 0..=(30 + RELEASE_AFTER as i64) {
        frame(&mut renderer, &project, tick);
    }
    let slot = |renderer: &Renderer, clip| {
        renderer
            .decoders
            .get(&DecoderKey {
                media,
                clip: Some(clip),
            })
            .map(|slot| slot.released)
    };
    assert_eq!(
        slot(&renderer, a),
        Some(true),
        "clip a left the screen at 30"
    );

    // One clip per pool preview of distinct media: more idle decoders than
    // the cap.
    let mut many = Project::new("many", FPS_30);
    let path = fixture();
    let sources: Vec<MediaId> = (0..MAX_DECODERS + 4)
        .map(|_| many.add_media(MediaSource::new(path.clone(), 640, 360, FPS_30, 90, false)))
        .collect();
    for media in &sources {
        renderer
            .render_media_frame_fit(&many, *media, RationalTime::new(0, FPS_30), 32, 18)
            .unwrap_or_else(|error| panic!("pool frame: {error}"));
    }
    let open = renderer.decoders.len();
    let recent = RELEASE_AFTER as usize + 1;
    assert!(
        open <= MAX_DECODERS + recent,
        "{open} decoders open, the cap is {MAX_DECODERS} idle plus {recent} recent"
    );
    let newest = sources[sources.len() - 1];
    assert!(
        renderer.decoders.contains_key(&DecoderKey {
            media: newest,
            clip: None
        }),
        "the most recent use was closed"
    );
}
