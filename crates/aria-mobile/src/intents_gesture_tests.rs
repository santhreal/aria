//! One case per gesture the shells can send.
//!
//! The class these close: [`Intent`] carries twenty-two variants and eleven of
//! them had no test anywhere — the arm compiled, the wire tag round-tripped, and
//! nothing asserted what the edit did. A gesture that lands on the wrong lane,
//! writes the wrong ticks, or leaves a hole in the main track is invisible under
//! that arrangement, because the shells that would notice are out of tree.
//!
//! [`every_intent_variant_has_a_case`] is the fail-by-default half: the variant
//! list is read out of `intents_wire.rs` at run time, so a twenty-third gesture
//! turns this file red until it is named here.
//!
//! Catalog ids are taken from the catalogs rather than typed in, because the
//! sticker and transition sets are data files a host can replace: a hardcoded id
//! would test this checkout instead of the contract.
//!
//! The property gestures and the importing gestures' refusals live in the
//! sibling `intents_prop_tests.rs`, split at the file ceiling; the fixtures and
//! readers below are shared with it.
//!
//! Not covered: the happy path of the three intents that import a path
//! (`InsertMain`, `AddPip`, `ReplaceMedia`). That needs a real encoder to build
//! the file, so it lives in `tests/session.rs` and runs only where one exists.

use super::*;
use aria_models::{MediaSource, Transition};

pub(super) const FPS: Rational = Rational::FPS_30;

/// Two seconds of synthetic 1080p footage with sound. No file is touched: the
/// intents under test here never decode, and a probe would make the case depend
/// on the host's codecs.
pub(super) fn media(project: &mut Project, name: &str) -> MediaId {
    project.add_media(MediaSource::new(
        format!("/aria-test/{name}.mp4"),
        1920,
        1080,
        FPS,
        60,
        true,
    ))
}

/// A main video track carrying `count` two-second clips, butt-joined from zero.
pub(super) fn main_with(count: usize) -> (Engine, TrackId, Vec<ClipId>) {
    let mut project = Project::new("gestures", FPS);
    let source = media(&mut project, "source");
    let main = project.add_track(TrackKind::Video, "Main");
    let mut clips = Vec::with_capacity(count);
    for index in 0..count {
        let start = 60 * index as i64;
        clips.push(
            project
                .add_clip(
                    main,
                    source,
                    TimeRange::at_rate(0, 60, FPS),
                    RationalTime::new(start, FPS),
                )
                .expect("place a main clip"),
        );
    }
    let engine = Engine::with_project(aria_engine::EngineConfig::default(), project)
        .expect("engine over the fixture");
    (engine, main, clips)
}

/// Every clip on `track`, as `(start, duration)` ticks in timeline order.
pub(super) fn spans(engine: &Engine, track: TrackId) -> Vec<(i64, i64)> {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .clips_ordered()
        .iter()
        .map(|clip| (clip.timeline.start.value, clip.timeline.duration.value))
        .collect()
}

/// Clips on `track` in timeline order.
pub(super) fn order(engine: &Engine, track: TrackId) -> Vec<ClipId> {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .clips_ordered()
        .iter()
        .map(|clip| clip.id)
        .collect()
}

pub(super) fn clip_of(response: &serde_json::Value) -> ClipId {
    ClipId::from_raw(response["clip"].as_u64().expect("a clip id"))
}

pub(super) fn track_of(response: &serde_json::Value) -> TrackId {
    TrackId::from_raw(response["track"].as_u64().expect("a track id"))
}

/// The clip as the project holds it. Borrowed, not copied: every caller reads
/// it after the edit it is checking, so there is nothing to invalidate.
pub(super) fn clip_at(engine: &Engine, clip: ClipId) -> &Clip {
    engine.project().timeline().clip(clip).expect("the clip")
}

/// The transition at `left`'s right junction, if the junction carries one.
pub(super) fn junction(engine: &Engine, track: TrackId, left: ClipId) -> Option<&Transition> {
    engine
        .project()
        .timeline()
        .track(track)
        .expect("track")
        .transition_at(left)
}

// --- placement --------------------------------------------------------------

/// A lane clip is re-placed at exactly the ticks the gesture names, and a
/// zero-length drag still leaves a clip a frame long rather than nothing.
#[test]
fn trim_lane_places_the_clip_at_the_ticks_it_names() {
    let (mut engine, _, _) = main_with(1);
    let added = run(
        &mut engine,
        Intent::AddSticker {
            at_seconds: 0.0,
            duration_seconds: 2.0,
            asset: None,
        },
    )
    .expect("a sticker lane");
    let sticker = clip_of(&added);
    let lane = track_of(&added);

    run(
        &mut engine,
        Intent::TrimLane {
            clip: sticker,
            start_seconds: 1.0,
            length_seconds: 0.5,
        },
    )
    .expect("re-place the lane clip");
    assert_eq!(spans(&engine, lane), vec![(30, 15)]);

    run(
        &mut engine,
        Intent::TrimLane {
            clip: sticker,
            start_seconds: 1.0,
            length_seconds: 0.0,
        },
    )
    .expect("a collapsed drag is still a clip");
    assert_eq!(
        spans(&engine, lane),
        vec![(30, 1)],
        "a zero-length trim must clamp to one frame, not delete the clip"
    );
}

/// A reorder on the main track leaves it gapless, and the moved clip is at the
/// slot it asked for rather than the tick it happened to be at.
#[test]
fn insert_into_main_reorders_without_leaving_a_hole() {
    let (mut engine, main, clips) = main_with(3);
    let last = clips[2];

    run(
        &mut engine,
        Intent::InsertIntoMain {
            clip: last,
            index: 0,
        },
    )
    .expect("reorder to the front");
    assert_eq!(
        spans(&engine, main),
        vec![(0, 60), (60, 60), (120, 60)],
        "the track must stay butt-joined through the reorder"
    );
    assert_eq!(
        order(&engine, main),
        vec![last, clips[0], clips[1]],
        "the moved clip did not land in slot 0"
    );

    // Asking for the slot it already holds is a no-op, not a re-shuffle.
    run(
        &mut engine,
        Intent::InsertIntoMain {
            clip: last,
            index: 0,
        },
    )
    .expect("a no-op reorder still answers");
    assert_eq!(order(&engine, main), vec![last, clips[0], clips[1]]);
    assert_eq!(spans(&engine, main), vec![(0, 60), (60, 60), (120, 60)]);
}

/// An index past the end lands at the end rather than erroring.
#[test]
fn insert_into_main_clamps_an_index_past_the_end() {
    let (mut engine, main, clips) = main_with(3);
    run(
        &mut engine,
        Intent::InsertIntoMain {
            clip: clips[0],
            index: 99,
        },
    )
    .expect("clamped reorder");
    assert_eq!(order(&engine, main), vec![clips[1], clips[2], clips[0]]);
    assert_eq!(spans(&engine, main), vec![(0, 60), (60, 60), (120, 60)]);
}

// --- generated content ------------------------------------------------------

/// A sticker drop creates its lane and its clip in one undoable step, and the
/// asset it carries comes from the catalog when the shell names none.
#[test]
fn a_sticker_drop_is_one_step_and_defaults_to_the_first_catalog_asset() {
    let expected = aria_models::sticker_catalog()
        .first()
        .expect("a bundled sticker")
        .id
        .as_str();
    let (mut engine, _, _) = main_with(1);
    let before = engine.project().timeline().clip_count();

    let added = run(
        &mut engine,
        Intent::AddSticker {
            at_seconds: 1.0,
            duration_seconds: 2.0,
            asset: None,
        },
    )
    .expect("sticker");
    let lane = track_of(&added);
    assert_eq!(
        engine.project().timeline().track(lane).expect("lane").kind,
        TrackKind::Sticker,
        "a sticker must not land on the main video track"
    );
    assert_eq!(spans(&engine, lane), vec![(30, 60)]);
    match &clip_at(&engine, clip_of(&added)).content {
        ClipSource::Generated(Generator::Sticker { asset }) => {
            assert_eq!(
                asset.as_str(),
                expected,
                "the drop must carry the catalog's first"
            );
        }
        other => panic!("a sticker drop must be generated content, got {other:?}"),
    }

    assert!(engine.undo(), "the whole drop is one history entry");
    assert_eq!(engine.project().timeline().clip_count(), before);
    assert!(
        engine.project().timeline().track(lane).is_none(),
        "undo must take the lane the drop created with it"
    );
}

/// Each effect-bar flavour lands on the lane kind that renders it, carrying the
/// matching generator. Driven from the three [`EffectLaneKind`] variants, so a
/// fourth flavour is red here until it is placed.
#[test]
fn an_effect_bar_lands_on_the_lane_kind_that_renders_it() {
    let cases = [
        (EffectLaneKind::Effect, TrackKind::Effect, Generator::Effect),
        (EffectLaneKind::Filter, TrackKind::Filter, Generator::Filter),
        (
            EffectLaneKind::Adjustment,
            TrackKind::Adjustment,
            Generator::Adjustment,
        ),
    ];
    for (kind, lane_kind, generator) in cases {
        let (mut engine, _, _) = main_with(1);
        let added = run(
            &mut engine,
            Intent::AddEffect {
                kind,
                at_seconds: 0.5,
                duration_seconds: 1.0,
            },
        )
        .expect("effect bar");
        let lane = track_of(&added);
        assert_eq!(
            engine.project().timeline().track(lane).expect("lane").kind,
            lane_kind,
            "{kind:?} landed on the wrong lane kind"
        );
        assert_eq!(spans(&engine, lane), vec![(15, 30)]);
        assert_eq!(
            clip_at(&engine, clip_of(&added)).content,
            ClipSource::Generated(generator),
            "{kind:?} placed the wrong generator"
        );
    }
}

// --- the census -------------------------------------------------------------

/// Every [`Intent`] variant is named by a case somewhere in this crate's tests.
///
/// Both halves are read at run time, so neither can go stale: the variant list
/// is parsed out of the wire declaration, and the sources are every `*_tests.rs`
/// beside this file plus every integration test. A hardcoded source list is how
/// this census went wrong once already — it named three files, missed the
/// integration tests that drive the importing gestures through the JSON wire,
/// and reported seven covered variants as holes.
///
/// A variant counts as named by the typed form (`Intent::Split`) or by its wire
/// tag (`"intent": "split"`), because a shell-facing test sends JSON and never
/// types the enum. Naming a variant is the weakest possible proof, which is why
/// it is paired with the cases above rather than trusted alone.
#[test]
fn every_intent_variant_has_a_case() {
    const WIRE: &str = include_str!("intents_wire.rs");
    let body = WIRE
        .split_once("pub enum Intent {")
        .expect("the Intent declaration")
        .1
        .split_once("\n}")
        .expect("the end of the declaration")
        .0;
    let variants: Vec<&str> = body
        .lines()
        .filter_map(|line| {
            let name = line.strip_prefix("    ")?;
            let name = name.split(['{', ',', ' ']).next()?;
            (!name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_uppercase()))
            .then_some(name)
        })
        .collect();
    assert!(
        variants.len() >= 20,
        "only {} variants parsed — the declaration shape changed and this \
         census has stopped seeing them",
        variants.len()
    );
    let cases = case_sources();
    assert!(
        cases.len() >= 8,
        "only {} test sources found — the census is looking in the wrong place",
        cases.len()
    );
    let missing: Vec<&str> = variants
        .into_iter()
        .filter(|name| {
            let typed = format!("Intent::{name}");
            let tagged = format!("\"intent\": \"{}\"", wire_tag(name));
            !cases
                .iter()
                .any(|source| source.contains(&typed) || source.contains(&tagged))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "these gestures have no test case: {missing:?}"
    );
}

/// Every test source in the crate: the sibling unit files and the integration
/// tests. Read from disk so a new file is a census source the moment it exists.
fn case_sources() -> Vec<String> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    for (dir, suffix) in [
        (crate_dir.join("src"), "_tests.rs"),
        (crate_dir.join("tests"), ".rs"),
    ] {
        let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {dir:?}: {e}"));
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(suffix))
            {
                sources.push(std::fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    sources
}

/// A variant's serde tag: `SetSpeedPreset` is `set_speed_preset`. The enum is
/// `rename_all = "snake_case"`, so this is the wire's own rule, not a guess.
fn wire_tag(variant: &str) -> String {
    let mut tag = String::with_capacity(variant.len() + 4);
    for (index, ch) in variant.char_indices() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                tag.push('_');
            }
            tag.push(ch.to_ascii_lowercase());
        } else {
            tag.push(ch);
        }
    }
    tag
}
