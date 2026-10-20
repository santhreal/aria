use super::*;
use crate::clip::{Clip, ClipSource, Generator};
use crate::time::Rational;

const R24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R24)
}

/// A project written before lane display existed must load with the shipped
/// lane, and a default lane must not add bytes to the file: both halves of
/// "additive" are the contract, because a silent default change would move
/// every old timeline.
#[test]
fn a_lane_without_display_loads_with_the_shipped_one() {
    let track = Track::new(TrackKind::Video, "V1");
    let json = serde_json::to_string(&track).expect("serialize");
    assert!(
        !json.contains("display"),
        "a default lane writes no display block: {json}"
    );
    // The written form IS the pre-display form, since a default lane omits
    // the block: reading it back is the legacy load.
    let back: Track = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.display, TrackDisplay::default());
    assert_eq!(back.display.height, TrackDisplay::DEFAULT_HEIGHT);
}

#[test]
fn a_customized_lane_round_trips() {
    let mut track = Track::new(TrackKind::Audio, "A1");
    track.display = TrackDisplay {
        height: 120,
        waveform: false,
        filmstrip: true,
        collapsed: true,
    };
    let json = serde_json::to_string(&track).expect("serialize");
    assert!(
        json.contains("display"),
        "a changed lane is written: {json}"
    );
    let back: Track = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.display, track.display);
}

#[test]
fn a_height_is_clamped_into_the_accepted_range() {
    assert_eq!(TrackDisplay::clamp_height(0), *TrackDisplay::HEIGHT.start());
    assert_eq!(
        TrackDisplay::clamp_height(10_000),
        *TrackDisplay::HEIGHT.end()
    );
    assert_eq!(TrackDisplay::clamp_height(64), 64);
}

#[test]
fn height_presets_are_ordered_in_range_and_clamp_accepts_mini() {
    assert_eq!(TrackDisplay::clamp_height(20), 20);
    assert_eq!(TrackDisplay::clamp_height(19), 20);
    assert_eq!(TrackDisplay::clamp_height(200), 200);
    assert_eq!(TrackDisplay::clamp_height(201), 200);
    assert!(
        TrackDisplay::PRESETS
            .windows(2)
            .all(|pair| pair[0].1 < pair[1].1),
        "presets must be in strictly ascending height order: {:?}",
        TrackDisplay::PRESETS
    );
    for &(name, height) in &TrackDisplay::PRESETS {
        assert!(
            TrackDisplay::HEIGHT.contains(&height),
            "preset {name} ({height} px) lies outside {:?}",
            TrackDisplay::HEIGHT
        );
    }
    assert!(
        TrackDisplay::PRESETS
            .iter()
            .any(|&(_, h)| h == TrackDisplay::DEFAULT_HEIGHT),
        "DEFAULT_HEIGHT ({}) must be one of {:?}",
        TrackDisplay::DEFAULT_HEIGHT,
        TrackDisplay::PRESETS
    );
}

fn generated_clip(start: i64, duration: i64) -> Clip {
    Clip::generated(Generator::Adjustment, tr(start, duration))
}

fn video_track(name: &str) -> Track {
    Track::new(TrackKind::Video, name)
}

// --- Track::new -------------------------------------------------------

#[test]
fn new_wires_kind_name_and_defaults() {
    let track = video_track("V1");
    assert_eq!(track.kind, TrackKind::Video);
    assert_eq!(track.name, "V1");
    assert!(track.enabled);
    assert!(!track.muted);
    assert!(!track.locked);
    assert!(!track.duck_source);
    assert!(track.is_empty());
    assert!(track.id.raw() >= 1);
}

#[test]
fn new_accepts_owned_string_name() {
    let track = Track::new(TrackKind::Audio, String::from("A1"));
    assert_eq!(track.kind, TrackKind::Audio);
    assert_eq!(track.name, "A1");
}

#[test]
fn new_assigns_distinct_track_ids() {
    let a = video_track("A");
    let b = video_track("B");
    assert_ne!(a.id, b.id);
}

// --- insert / remove / lookup -----------------------------------------

#[test]
fn insert_and_remove_clip() {
    let mut track = video_track("V1");
    let clip = generated_clip(0, 50);
    let id = clip.id;

    assert!(track.insert_clip(clip).is_none());
    assert_eq!(track.len(), 1);
    assert_eq!(track.clip(id).unwrap().timeline, tr(0, 50));

    let removed = track.remove_clip(id).unwrap();
    assert_eq!(removed.id, id);
    assert!(track.is_empty());
    assert!(track.clip(id).is_none());
}

#[test]
fn insert_replaces_same_id() {
    let mut track = video_track("V1");
    let first = generated_clip(0, 10);
    let id = first.id;
    assert!(track.insert_clip(first).is_none());
    let other = generated_clip(40, 10);
    let other_id = other.id;
    assert!(track.insert_clip(other).is_none());

    let replacement = Clip {
        id,
        content: ClipSource::Generated(Generator::Adjustment),
        timeline: tr(60, 30),
        ..generated_clip(60, 30)
    };
    let displaced = track.insert_clip(replacement).unwrap();
    assert_eq!(displaced.timeline, tr(0, 10));
    assert_eq!(track.clip(id).unwrap().timeline, tr(60, 30));
    assert_eq!(track.len(), 2);
    // The replaced clip leaves its old slot in the order and takes the one its
    // new start sorts to: one entry per clip, by start.
    let ordered: Vec<_> = track.clips_ordered().iter().map(|c| c.id).collect();
    assert_eq!(ordered, [other_id, id]);
}

#[test]
fn clip_mut_reorders_after_timeline_change() {
    let mut track = video_track("V1");
    let early = generated_clip(10, 10);
    let late = generated_clip(50, 10);
    let early_id = early.id;
    let late_id = late.id;
    track.insert_clip(early);
    track.insert_clip(late);

    track.clip_timeline_mut(late_id).unwrap().timeline = tr(0, 10);

    let ordered: Vec<_> = track.clips_ordered().map(|clip| clip.id).collect();
    assert_eq!(ordered, vec![late_id, early_id]);
}

#[test]
fn clips_and_for_each_clip_mut_iterate_all() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 10));
    track.insert_clip(generated_clip(20, 10));
    assert_eq!(track.clips().count(), 2);

    track.for_each_clip_mut(|clip| {
        clip.timeline = tr(clip.timeline.start.value + 1, clip.timeline.duration.value);
    });
    // `clips()` iterates the backing hash map — unordered by contract
    // (FxHash order shifts with the globally allocated clip ids).
    let mut starts: Vec<i64> = track.clips().map(|c| c.start().value).collect();
    starts.sort_unstable();
    assert_eq!(starts, vec![1, 21]);
}

// --- clips_ordered ----------------------------------------------------

#[test]
fn clips_ordered_sorts_by_start_then_id() {
    let mut track = video_track("V1");
    let late = generated_clip(100, 10);
    let early = generated_clip(0, 10);
    let early_id = early.id;
    let late_id = late.id;
    track.insert_clip(late);
    track.insert_clip(early);

    let ordered: Vec<ClipId> = track.clips_ordered().iter().map(|c| c.id).collect();
    assert_eq!(ordered, vec![early_id, late_id]);
}

#[test]
fn clips_ordered_breaks_start_ties_by_clip_id() {
    let mut track = video_track("V1");
    let a = Clip {
        id: ClipId::from_raw(2),
        ..generated_clip(10, 5)
    };
    let b = Clip {
        id: ClipId::from_raw(1),
        ..generated_clip(10, 5)
    };
    track.insert_clip(a);
    track.insert_clip(b);

    let ordered: Vec<ClipId> = track.clips_ordered().iter().map(|c| c.id).collect();
    assert_eq!(ordered, vec![ClipId::from_raw(1), ClipId::from_raw(2)]);
}

#[test]
fn clips_ordered_maintains_order_across_insert_remove_deserialize() {
    let mut track = video_track("V1");
    let c1 = generated_clip(50, 10);
    let c2 = generated_clip(10, 10);
    let c3 = generated_clip(100, 10);
    let id1 = c1.id;
    let id2 = c2.id;
    let id3 = c3.id;

    track.insert_clip(c1);
    track.insert_clip(c2);
    track.insert_clip(c3);

    let ordered = track.clips_ordered();
    assert_eq!(ordered.len(), 3);
    assert!(!ordered.is_empty());
    assert_eq!(ordered[0].id, id2);
    assert_eq!(ordered[1].id, id1);
    assert_eq!(ordered[2].id, id3);

    // Double-ended iteration
    let reversed: Vec<ClipId> = track.clips_ordered().rev().map(|c| c.id).collect();
    assert_eq!(reversed, vec![id3, id1, id2]);

    // Removal updates order
    track.remove_clip(id1);
    let ordered_after_remove: Vec<ClipId> = track.clips_ordered().iter().map(|c| c.id).collect();
    assert_eq!(ordered_after_remove, vec![id2, id3]);

    // Serialization / deserialization round-trip maintains order
    let json = serde_json::to_string(&track).expect("serialize");
    let deserialized: Track = serde_json::from_str(&json).expect("deserialize");
    let deserialized_order: Vec<ClipId> =
        deserialized.clips_ordered().iter().map(|c| c.id).collect();
    assert_eq!(deserialized_order, vec![id2, id3]);
}

#[test]
fn clips_ordered_lifecycle_insert_move_trim_remove() {
    let mut project = crate::Project::new("test", Rational::FPS_24);
    let track_id = project.add_track(TrackKind::Adjustment, "FX1");

    let a = project
        .add_generated(track_id, Generator::Adjustment, tr(100, 20))
        .expect("a");
    let b = project
        .add_generated(track_id, Generator::Adjustment, tr(0, 20))
        .expect("b");
    let c = project
        .add_generated(track_id, Generator::Adjustment, tr(50, 20))
        .expect("c");

    let get_order = |p: &crate::Project| -> Vec<ClipId> {
        p.timeline()
            .track(track_id)
            .unwrap()
            .clips_ordered()
            .iter()
            .map(|clip| clip.id)
            .collect()
    };

    // After insert: start order is b (0), c (50), a (100)
    assert_eq!(get_order(&project), vec![b, c, a]);

    // After move: move a from 100 to 25 -> order becomes b (0), a (25), c (50)
    project.move_clip(a, track_id, rt(25)).expect("move");
    assert_eq!(get_order(&project), vec![b, a, c]);

    // After trim: trim b head from 0..20 to 10..20 -> still b (10), a (25), c (50)
    project.trim_clip(b, tr(10, 10)).expect("trim");
    assert_eq!(get_order(&project), vec![b, a, c]);

    // After remove: remove a -> order becomes b (10), c (50)
    project.remove_clip(a);
    assert_eq!(get_order(&project), vec![b, c]);
}
// --- clip_at ----------------------------------------------------------

#[test]
fn clip_at_finds_occupant() {
    let mut track = video_track("V1");
    let clip = generated_clip(10, 20);
    let id = clip.id;
    track.insert_clip(clip);

    assert_eq!(track.clip_at(rt(10)).unwrap().map(|c| c.id), Some(id));
    assert_eq!(track.clip_at(rt(29)).unwrap().map(|c| c.id), Some(id));
    assert!(track.clip_at(rt(30)).unwrap().is_none());
    assert!(track.clip_at(rt(9)).unwrap().is_none());
}

#[test]
fn clip_at_rate_mismatch_errors() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 10));
    assert_eq!(
        track
            .clip_at(RationalTime::new(5, Rational::FPS_30))
            .unwrap_err(),
        ModelError::RateMismatch {
            expected: Rational::FPS_30,
            got: R24,
        }
    );
}

// --- has_overlap ------------------------------------------------------

#[test]
fn has_overlap_detects_partial_overlap() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 50));
    assert!(track.has_overlap(tr(25, 50), None).unwrap());
}

#[test]
fn has_overlap_touching_ranges_do_not_overlap() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 50));
    assert!(!track.has_overlap(tr(50, 50), None).unwrap());
    assert!(track.has_overlap(tr(0, 50), None).unwrap()); // identical placement collides
}

#[test]
fn has_overlap_ignore_skips_clip() {
    let mut track = video_track("V1");
    let clip = generated_clip(0, 50);
    let id = clip.id;
    track.insert_clip(clip);
    assert!(!track.has_overlap(tr(0, 50), Some(id)).unwrap());
    assert!(track.has_overlap(tr(0, 50), None).unwrap());
}

#[test]
fn has_overlap_rate_mismatch_errors() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 50));
    let bad = TimeRange::at_rate(0, 10, Rational::FPS_30);
    assert_eq!(
        track.has_overlap(bad, None).unwrap_err(),
        ModelError::RateMismatch {
            expected: Rational::FPS_30,
            got: R24,
        }
    );
}

// --- content_end ------------------------------------------------------

#[test]
fn content_end_empty_is_zero() {
    assert_eq!(video_track("V1").content_end(), 0);
}

#[test]
fn content_end_is_max_exclusive_end() {
    let mut track = video_track("V1");
    track.insert_clip(generated_clip(0, 50));
    track.insert_clip(generated_clip(100, 30));
    assert_eq!(track.content_end(), 130);
}

/// `clip_at`, `has_overlap`, and `content_end` index `Track::order` by binary
/// search and tail lookup instead of scanning `clips`.
///
/// WHY: `resolve_timeline_into`, edit collision checks, and `Timeline::duration`
/// call these methods across every lane on every frame and edit; scanning a
/// 200-clip lane linearly turns each lookup into 200 hash-map probes. What this
/// does not catch: a caller outside `Track` that iterates `Track::clips`
/// directly instead of calling `clip_at`, `has_overlap`, or `content_end`.
#[test]
fn ordered_index_bounds_clip_at_has_overlap_and_content_end_probes() {
    let probes = || super::ORDER_PROBES.with(std::cell::Cell::get);
    let mut track = video_track("V1");
    assert_eq!(track.clip_at(rt(0)).unwrap(), None);
    assert!(!track.has_overlap(tr(0, 10), None).unwrap());
    assert_eq!(track.content_end(), 0);

    let mut ids = Vec::with_capacity(200);
    for i in 0..200_i64 {
        let clip = generated_clip(i * 20 + 10, 10);
        ids.push(clip.id);
        track.insert_clip(clip);
    }

    let before = probes();
    assert_eq!(track.content_end(), 199 * 20 + 20);
    assert_eq!(probes() - before, 1);

    // Before first clip, inside first/middle/last clips, at exact start/end
    // ticks, in gaps between clips, and past the last clip.
    for (tick, expected) in [
        (0, None),
        (9, None),
        (10, Some(ids[0])),
        (15, Some(ids[0])),
        (19, Some(ids[0])),
        (20, None),
        (29, None),
        (100 * 20 + 10, Some(ids[100])),
        (100 * 20 + 19, Some(ids[100])),
        (100 * 20 + 20, None),
        (199 * 20 + 10, Some(ids[199])),
        (199 * 20 + 19, Some(ids[199])),
        (199 * 20 + 20, None),
        (5_000, None),
    ] {
        let start = probes();
        assert_eq!(
            track.clip_at(rt(tick)).unwrap().map(|c| c.id),
            expected,
            "clip_at({tick})"
        );
        let used = probes() - start;
        assert!(
            used <= 12,
            "clip_at({tick}) inspected {used} clips on a 200-clip lane"
        );
    }

    for (range, ignore, expected) in [
        (tr(0, 10), None, false),
        (tr(0, 11), None, true),
        (tr(19, 2), None, true),
        (tr(20, 10), None, false),
        (tr(20, 11), None, true),
        (tr(100 * 20, 10), None, false),
        (tr(100 * 20 + 10, 10), Some(ids[100]), false),
        (tr(100 * 20 + 10, 21), Some(ids[100]), true),
        (tr(199 * 20 + 19, 2), None, true),
        (tr(199 * 20 + 20, 10), None, false),
    ] {
        let start = probes();
        assert_eq!(
            track.has_overlap(range, ignore).unwrap(),
            expected,
            "has_overlap({range:?}, {ignore:?})"
        );
        let used = probes() - start;
        assert!(
            used <= 12,
            "has_overlap({range:?}, {ignore:?}) inspected {used} clips on a 200-clip lane"
        );
    }

    for (range, expected) in [
        (tr(0, 10), vec![]),
        (tr(0, 11), vec![ids[0]]),
        (tr(19, 2), vec![ids[0]]),
        (tr(20, 10), vec![]),
        (tr(10, 35), vec![ids[0], ids[1]]),
        (tr(100 * 20 + 10, 10), vec![ids[100]]),
        (tr(100 * 20 + 15, 20), vec![ids[100], ids[101]]),
        (tr(199 * 20 + 20, 10), vec![]),
    ] {
        let start = probes();
        let got: Vec<ClipId> = track.clips_overlapping(range).map(|c| c.id).collect();
        assert_eq!(got, expected, "clips_overlapping({range:?})");
        let used = probes() - start;
        assert!(
            used <= 12,
            "clips_overlapping({range:?}) inspected {used} clips on a 200-clip lane"
        );
    }

    // Rate mismatch when the query range precedes every clip (idx == 0) or
    // when the only clip before `end` is ignored.
    assert!(
        track
            .clip_at(RationalTime::new(0, Rational::FPS_30))
            .is_err()
    );
    assert!(
        track
            .has_overlap(TimeRange::at_rate(0, 5, Rational::FPS_30), None)
            .is_err()
    );
    assert!(
        track
            .has_overlap(TimeRange::at_rate(10, 10, Rational::FPS_30), Some(ids[0]))
            .is_err()
    );
}

// --- flags / Clone ----------------------------------------------------

#[test]
fn enabled_muted_and_locked_are_mutable() {
    let mut track = Track::new(TrackKind::Audio, "A1");
    track.enabled = false;
    track.muted = true;
    track.locked = true;
    track.duck_source = true;
    assert!(!track.enabled);
    assert!(track.muted);
    assert!(track.locked);
    assert!(track.duck_source);
}

#[test]
fn duck_source_defaults_false_for_back_compat() {
    // A pre-Phase-4 track JSON (no duck_source field) loads as not a
    // voice source.
    let json = serde_json::json!({
        "id": 1,
        "kind": "Audio",
        "name": "A1",
        "enabled": true,
        "muted": false,
        "clips": []
    });
    let track: Track = serde_json::from_value(json).unwrap();
    assert!(!track.duck_source);
}

#[test]
fn clone_preserves_clips_and_metadata() {
    let mut track = video_track("V1");
    let clip = generated_clip(0, 25);
    let id = clip.id;
    track.insert_clip(clip);
    let cloned = track.clone();
    assert_eq!(cloned.name, "V1");
    assert_eq!(cloned.clip(id).unwrap().timeline, tr(0, 25));
    assert_eq!(cloned.len(), 1);
}

#[test]
fn track_kind_equality() {
    assert_eq!(TrackKind::Video, TrackKind::Video);
    assert_ne!(TrackKind::Video, TrackKind::Audio);
    assert_ne!(TrackKind::Text, TrackKind::Sticker);
}

#[test]
fn track_kind_accepts_clip_by_lane() {
    use crate::clip::{Clip, Generator};

    let media = Clip::from_media(crate::ids::MediaId::next(), tr(0, 10), tr(0, 10));
    let text = Clip::generated(Generator::text("hi"), tr(0, 10));
    let sticker = Clip::generated(Generator::SolidColor { rgba: [1, 2, 3, 4] }, tr(0, 10));
    let adj = Clip::generated(Generator::Adjustment, tr(0, 10));

    assert!(TrackKind::Video.accepts_clip(&media));
    assert!(!TrackKind::Video.accepts_clip(&text));
    assert!(TrackKind::Text.accepts_clip(&text));
    assert!(!TrackKind::Text.accepts_clip(&sticker));
    assert!(TrackKind::Sticker.accepts_clip(&sticker));
    assert!(TrackKind::Adjustment.accepts_clip(&adj));
    assert!(!TrackKind::Adjustment.accepts_clip(&text));
    assert!(TrackKind::Audio.accepts_clip(&media));
}
