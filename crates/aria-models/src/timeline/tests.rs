use super::*;
use crate::clip::{Clip, Generator};
use crate::ids::MarkerId;
use crate::time::TimeRange;
use crate::track::{Track, TrackKind};

const R24: Rational = Rational::FPS_24;

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R24)
}

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R24)
}

fn generated_clip(start: i64, duration: i64) -> Clip {
    Clip::generated(Generator::Adjustment, tr(start, duration))
}

fn timeline_with_track() -> (Timeline, TrackId) {
    let mut timeline = Timeline::new(R24);
    let track = timeline.add_track(Track::new(TrackKind::Adjustment, "FX"));
    (timeline, track)
}

// --- Timeline::new ----------------------------------------------------

#[test]
fn new_starts_empty_at_frame_rate() {
    let timeline = Timeline::new(R24);
    assert_eq!(timeline.frame_rate, R24);
    assert_eq!(timeline.track_count(), 0);
    assert_eq!(timeline.clip_count(), 0);
    assert!(timeline.order().is_empty());
    assert_eq!(timeline.duration(), rt(0));
}

// --- tracks -----------------------------------------------------------

#[test]
fn add_track_appends_visual_to_top_and_floors_audio() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));

    // Visual lanes stack bottom→top in insert order; the audio lane sinks
    // below them (index 0 = bottom of the stack / bottom of the UI).
    assert_eq!(timeline.order(), &[a1, v1, v2]);
    assert_eq!(timeline.track_count(), 3);
    assert_eq!(timeline.track(v1).unwrap().name, "V1");
    assert_eq!(timeline.track(a1).unwrap().kind, TrackKind::Audio);
}

#[test]
fn audio_always_sinks_below_video_regardless_of_add_order() {
    // Interleave kinds and confirm the CapCut zones: audio at the bottom,
    // the main video lane above it, overlay lanes next, text on top —
    // with relative order preserved inside each zone.
    let mut timeline = Timeline::new(R24);
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let a2 = timeline.add_track(Track::new(TrackKind::Audio, "A2"));
    let t1 = timeline.add_track(Track::new(TrackKind::Text, "T1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));

    assert_eq!(timeline.order(), &[a1, a2, v1, v2, t1]);
    assert_eq!(timeline.main_track(), Some(v1));
}

#[test]
fn insert_track_audio_sinks_and_visual_clamps_above_main() {
    let mut timeline = Timeline::new(R24);
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));

    // Requesting a video track at the very bottom (index 0) lifts it
    // above the audio block *and* the main lane: nothing but audio may
    // sit below the main track.
    let v0 = timeline.insert_track(Track::new(TrackKind::Video, "V0"), 0);
    assert_eq!(timeline.order(), &[a1, v1, v0]);
    assert_eq!(timeline.main_track(), Some(v1), "main status is sticky");

    // Requesting an audio track at the top sinks back into the audio block.
    let a2 = timeline.insert_track(Track::new(TrackKind::Audio, "A2"), 99);
    assert_eq!(timeline.order(), &[a1, a2, v1, v0]);
}

#[test]
fn move_track_reorders_within_zone_only() {
    let mut timeline = Timeline::new(Rational::new(24, 1));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    let v3 = timeline.add_track(Track::new(TrackKind::Video, "V3"));
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    assert_eq!(timeline.order(), &[a1, v1, v2, v3]);

    // Overlay lanes reorder freely within the overlay zone.
    timeline.move_track(v2, 3).unwrap();
    assert_eq!(timeline.order(), &[a1, v1, v3, v2]);

    // The main lane snaps back: it can't rise above overlays…
    timeline.move_track(v1, 3).unwrap();
    assert_eq!(timeline.order(), &[a1, v1, v3, v2]);
    // …and no overlay can sink below it.
    timeline.move_track(v3, 0).unwrap();
    assert_eq!(timeline.order(), &[a1, v1, v3, v2]);
}

#[test]
fn insert_track_places_at_order_index_and_clamps() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));

    // V1 is the main lane, so a bottom insert clamps to just above it.
    let above_main = timeline.insert_track(Track::new(TrackKind::Video, "V3"), 0);
    let middle = timeline.insert_track(Track::new(TrackKind::Video, "V4"), 2);
    let top = timeline.insert_track(Track::new(TrackKind::Video, "V5"), 99);

    assert_eq!(timeline.order(), &[v1, above_main, middle, v2, top]);
    assert_eq!(timeline.track_count(), 5);
}

#[test]
fn first_video_track_becomes_main_and_persists() {
    let mut timeline = Timeline::new(R24);
    assert_eq!(timeline.main_track(), None);

    // Non-video lanes never claim main status.
    timeline.add_track(Track::new(TrackKind::Text, "T1"));
    timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    assert_eq!(timeline.main_track(), None);

    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    assert_eq!(timeline.main_track(), Some(v1));
    assert!(timeline.track(v1).unwrap().main);

    // Later video lanes are overlays.
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    assert_eq!(timeline.main_track(), Some(v1));
    assert!(!timeline.track(v2).unwrap().main);
}

#[test]
fn removing_main_promotes_next_bottom_video() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    let v3 = timeline.add_track(Track::new(TrackKind::Video, "V3"));

    timeline.remove_track(v1);
    assert_eq!(timeline.main_track(), Some(v2));
    assert_eq!(timeline.order(), &[v2, v3]);

    timeline.remove_track(v2);
    assert_eq!(timeline.main_track(), Some(v3));

    timeline.remove_track(v3);
    assert_eq!(timeline.main_track(), None);
}

#[test]
fn text_lanes_stay_on_top_and_only_audio_below_main() {
    let mut timeline = Timeline::new(R24);
    let t1 = timeline.add_track(Track::new(TrackKind::Text, "T1"));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let s1 = timeline.add_track(Track::new(TrackKind::Sticker, "ST1"));
    let adj = timeline.add_track(Track::new(TrackKind::Adjustment, "ADJ1"));
    let t2 = timeline.add_track(Track::new(TrackKind::Text, "T2"));
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));

    // Zones bottom→top: audio, main video, overlays, text.
    assert_eq!(timeline.order(), &[a1, v1, s1, adj, t1, t2]);

    // A text lane moved to the bottom snaps back above the overlays.
    timeline.move_track(t1, 0).unwrap();
    assert_eq!(timeline.order(), &[a1, v1, s1, adj, t1, t2]);

    // An adjustment lane can never sink below the main track.
    timeline.move_track(adj, 0).unwrap();
    assert_eq!(timeline.order(), &[a1, v1, adj, s1, t1, t2]);
}

#[test]
fn normalize_lanes_derives_main_and_zones_for_legacy_files() {
    // Simulate a pre-main-flag file: build the raw state without going
    // through add_track (serde would produce exactly this shape).
    let mut timeline = Timeline::new(R24);
    let t1 = timeline.add_track(Track::new(TrackKind::Text, "T1"));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    // Strip the flags to fake the legacy load.
    timeline.track_mut(v1).unwrap().main = false;
    timeline.track_mut(v2).unwrap().main = false;

    timeline.normalize_lanes();
    assert_eq!(timeline.main_track(), Some(v1), "bottom video lane wins");
    assert_eq!(timeline.order(), &[v1, v2, t1]);

    // A corrupt file flagging a non-video lane is repaired.
    timeline.track_mut(t1).unwrap().main = true;
    timeline.track_mut(v1).unwrap().main = false;
    timeline.normalize_lanes();
    assert!(!timeline.track(t1).unwrap().main);
    assert_eq!(timeline.main_track(), Some(v1));
}

#[test]
fn main_flag_serde_round_trips_and_defaults_false() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));

    let json = serde_json::to_value(&timeline).unwrap();
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.main_track(), Some(v1));

    // Pre-main-flag track JSON loads as not-main (normalize_lanes
    // re-derives on file load).
    let legacy = serde_json::json!({
        "id": 1,
        "kind": "Video",
        "name": "V1",
        "enabled": true,
        "muted": false,
        "clips": []
    });
    let track: Track = serde_json::from_value(legacy).unwrap();
    assert!(!track.main);
}

#[test]
fn tracks_ordered_yields_bottom_to_top() {
    let mut timeline = Timeline::new(R24);
    timeline.add_track(Track::new(TrackKind::Video, "bottom"));
    timeline.add_track(Track::new(TrackKind::Video, "top"));
    let names: Vec<&str> = timeline.tracks_ordered().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["bottom", "top"]);
}

#[test]
fn track_mut_can_toggle_enabled() {
    let mut timeline = Timeline::new(R24);
    let id = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    timeline.track_mut(id).unwrap().enabled = false;
    assert!(!timeline.track(id).unwrap().enabled);
}

#[test]
fn restore_track_reinserts_stack_position_and_clip_index() {
    let (mut timeline, track_id) = timeline_with_track();
    let clip_id = timeline
        .add_clip(track_id, generated_clip(0, 10))
        .expect("clip");
    let track = timeline.remove_track(track_id).expect("remove");
    assert_eq!(timeline.track_count(), 0);
    assert!(timeline.clip(clip_id).is_none());

    timeline.restore_track(track, 0).expect("restore");
    assert_eq!(timeline.track_count(), 1);
    assert_eq!(timeline.track_of(clip_id), Some(track_id));
}

#[test]
fn restore_track_keeps_audio_below_video() {
    // Undo of removing a video lane must not slip it under the audio floor.
    let mut timeline = Timeline::new(R24);
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    assert_eq!(timeline.order(), &[a1, v1, v2]);

    let order_index = timeline.order().iter().position(|&t| t == v1).unwrap();
    let removed = timeline.remove_track(v1).expect("remove");
    assert_eq!(timeline.order(), &[a1, v2]);

    timeline
        .restore_track(removed, order_index)
        .expect("restore");
    assert_eq!(timeline.order(), &[a1, v1, v2]);
}

#[test]
fn remove_track_purges_clips_from_index() {
    let (mut timeline, track) = timeline_with_track();
    let clip = timeline.add_clip(track, generated_clip(0, 50)).unwrap();

    let removed = timeline.remove_track(track).unwrap();
    assert_eq!(removed.track.len(), 1);
    assert!(removed.detached.is_empty());
    assert_eq!(timeline.track_count(), 0);
    assert_eq!(timeline.clip_count(), 0);
    assert!(timeline.clip(clip).is_none());
    assert!(timeline.track_of(clip).is_none());
}

#[test]
fn remove_unknown_track_returns_none() {
    let mut timeline = Timeline::new(R24);
    // A raw id is only accidentally absent: ids come from one process-global
    // counter, so `from_raw(99)` names a live track in a long-running test
    // binary. An id allocated and never inserted is unknown at any count.
    assert!(timeline.remove_track(TrackId::next()).is_none());
}

// --- add_clip / clip index --------------------------------------------

#[test]
fn add_clip_registers_in_track_and_index() {
    let (mut timeline, track) = timeline_with_track();
    let clip = generated_clip(10, 40);
    let clip_id = clip.id;

    let returned = timeline.add_clip(track, clip).unwrap();
    assert_eq!(returned, clip_id);
    assert_eq!(timeline.clip_count(), 1);
    assert_eq!(timeline.track_of(clip_id), Some(track));
    assert_eq!(timeline.clip(clip_id).unwrap().timeline, tr(10, 40));
    assert_eq!(timeline.track(track).unwrap().len(), 1);
}

#[test]
fn add_clip_unknown_track_errors() {
    let (mut timeline, _) = timeline_with_track();
    let missing = TrackId::next();
    assert_eq!(
        timeline.add_clip(missing, generated_clip(0, 10)),
        Err(ModelError::UnknownTrack(missing))
    );
}

#[test]
fn add_clip_rejects_overlap_on_same_track() {
    let (mut timeline, track) = timeline_with_track();
    timeline.add_clip(track, generated_clip(0, 50)).unwrap();
    assert_eq!(
        timeline.add_clip(track, generated_clip(25, 50)),
        Err(ModelError::Overlap(track))
    );
}

#[test]
fn add_clip_rejects_duplicate_id_without_mutating() {
    // Defense in depth for the id-collision bug: a clip whose id already
    // lives on the timeline must be rejected, never silently overwrite the
    // existing clip (which would drop it from its track/index).
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX2"));
    let clip = generated_clip(0, 50);
    let id = clip.id;
    let mut dup = generated_clip(200, 10);
    dup.id = id;

    timeline.add_clip(v1, clip).unwrap();
    assert_eq!(
        timeline.add_clip(v2, dup),
        Err(ModelError::DuplicateClip(id))
    );
    // The original is untouched: still on v1, still 50 ticks at start 0.
    assert_eq!(timeline.clip_count(), 1);
    assert_eq!(timeline.track_of(id), Some(v1));
    assert_eq!(timeline.clip(id).unwrap().timeline, tr(0, 50));
    assert!(timeline.track(v2).unwrap().is_empty());
}

#[test]
fn add_clip_allows_same_range_on_different_tracks() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX2"));

    let c1 = timeline.add_clip(v1, generated_clip(0, 50)).unwrap();
    let c2 = timeline.add_clip(v2, generated_clip(0, 50)).unwrap();
    assert_ne!(c1, c2);
    assert_eq!(timeline.clip_count(), 2);
}

#[test]
fn add_clip_allows_adjacent_non_overlapping_clips() {
    let (mut timeline, track) = timeline_with_track();
    timeline.add_clip(track, generated_clip(0, 50)).unwrap();
    let second = timeline.add_clip(track, generated_clip(50, 50)).unwrap();
    assert_eq!(timeline.clip_count(), 2);
    assert_eq!(timeline.clip(second).unwrap().start().value, 50);
}

#[test]
fn carries_own_audio_follows_lane_and_detach() {
    let mut timeline = Timeline::new(R24);
    let v = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let a = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let fx = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));

    let media = crate::ids::MediaId::from_raw(1);
    let media_clip = |start, dur| Clip::from_media(media, tr(start, dur), tr(start, dur));

    // CapCut keeps a video's sound on the clip, so a video clip carries its
    // own audio; audio lanes always do; non-AV lanes never do.
    let vc = timeline.add_clip(v, media_clip(0, 24)).unwrap();
    let ac = timeline.add_clip(a, media_clip(0, 24)).unwrap();
    let fxc = timeline.add_clip(fx, generated_clip(0, 24)).unwrap();
    assert!(timeline.carries_own_audio(vc));
    assert!(timeline.carries_own_audio(ac));
    assert!(!timeline.carries_own_audio(fxc));

    // "Separate audio": link the video clip to a clip on an audio lane and
    // the video half defers its sound there (silent on its own lane).
    let companion = timeline.add_clip(a, media_clip(48, 24)).unwrap();
    let link = crate::ids::LinkId::next();
    timeline.clip_mut(vc).unwrap().link = Some(link);
    timeline.clip_mut(companion).unwrap().link = Some(link);
    assert!(
        !timeline.carries_own_audio(vc),
        "detached video half is silent"
    );
    assert!(
        timeline.carries_own_audio(companion),
        "the audio-lane partner sounds"
    );
}

/// [`Timeline::audio_lane_links`] is the one-pass form of the per-clip scans:
/// for every clip, the set gives the answer [`Timeline::detached_to_audio_lane`]
/// and [`Timeline::carries_own_audio`] give, and
/// [`Timeline::audio_lane_partner`] finds the audio-lane member of its link
/// group. Closes the class where a caller deriving every clip through the set
/// disagrees with one asking about a single clip. The shapes: a video clip
/// linked to an audio-lane partner, two video clips linked only to each other,
/// a linked freeze frame, a link from a lane that carries no sound, and
/// unlinked clips on every lane.
#[test]
fn the_audio_lane_link_set_answers_as_the_scans_do() {
    fn link(timeline: &mut Timeline, clips: &[ClipId]) {
        let link = crate::ids::LinkId::next();
        for clip in clips {
            timeline.clip_mut(*clip).unwrap().link = Some(link);
        }
    }
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let fx = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));
    let media = crate::ids::MediaId::from_raw(1);
    let media_clip = |start, dur| Clip::from_media(media, tr(start, dur), tr(start, dur));

    let picture = timeline.add_clip(v1, media_clip(0, 24)).unwrap();
    let sound = timeline.add_clip(a1, media_clip(0, 24)).unwrap();
    link(&mut timeline, &[picture, sound]);
    let left = timeline.add_clip(v1, media_clip(48, 24)).unwrap();
    let right = timeline.add_clip(v2, media_clip(48, 24)).unwrap();
    link(&mut timeline, &[left, right]);
    let mut still = media_clip(96, 24);
    still.freeze_frame = true;
    let frozen = timeline.add_clip(v2, still).unwrap();
    let held = timeline.add_clip(a1, media_clip(96, 24)).unwrap();
    link(&mut timeline, &[frozen, held]);
    let adjust = timeline.add_clip(fx, generated_clip(144, 24)).unwrap();
    let under = timeline.add_clip(a1, media_clip(144, 24)).unwrap();
    link(&mut timeline, &[adjust, under]);
    for lane in [v1, v2, a1] {
        timeline.add_clip(lane, media_clip(200, 24)).unwrap();
    }
    timeline.add_clip(fx, generated_clip(200, 24)).unwrap();

    let links = timeline.audio_lane_links();
    let partners = [
        (picture, sound),
        (sound, sound),
        (frozen, held),
        (held, held),
        (adjust, under),
        (under, under),
    ];
    let mut detached = Vec::new();
    for lane in timeline.tracks_ordered() {
        for clip in lane.clips() {
            let case = format!("{:?} on {}", clip.id, lane.name);
            assert_eq!(
                links.detached(clip),
                timeline.detached_to_audio_lane(clip.id),
                "{case}"
            );
            assert_eq!(
                links.carries_own_audio(lane, clip),
                timeline.carries_own_audio(clip.id),
                "{case}"
            );
            assert_eq!(
                clip.link
                    .and_then(|link| timeline.audio_lane_partner(link))
                    .map(|partner| partner.id),
                partners
                    .iter()
                    .find(|(member, _)| *member == clip.id)
                    .map(|(_, partner)| *partner),
                "{case}: the audio-lane member of its link group"
            );
            if links.detached(clip) {
                detached.push(clip.id.raw());
            }
        }
    }
    detached.sort_unstable();
    let mut reaching = [picture, sound, frozen, held, adjust, under].map(ClipId::raw);
    reaching.sort_unstable();
    assert_eq!(
        detached, reaching,
        "every member of a link reaching an audio lane, and no other clip"
    );
}

// --- remove_clip / lookup ---------------------------------------------

#[test]
fn remove_clip_returns_clip_and_clears_index() {
    let (mut timeline, track) = timeline_with_track();
    let id = timeline.add_clip(track, generated_clip(0, 30)).unwrap();

    let removed = timeline.remove_clip(id).unwrap();
    assert_eq!(removed.id, id);
    assert_eq!(timeline.clip_count(), 0);
    assert!(timeline.clip(id).is_none());
    assert!(timeline.track_of(id).is_none());
    assert!(timeline.track(track).unwrap().is_empty());
}

#[test]
fn remove_clip_unknown_returns_none() {
    let (mut timeline, _) = timeline_with_track();
    assert!(timeline.remove_clip(ClipId::next()).is_none());
}

#[test]
fn clip_mut_updates_timeline_range() {
    let (mut timeline, track) = timeline_with_track();
    let id = timeline.add_clip(track, generated_clip(0, 50)).unwrap();

    timeline.clip_timeline_mut(id).unwrap().timeline = tr(10, 40);
    assert_eq!(timeline.clip(id).unwrap().timeline, tr(10, 40));
}

#[test]
fn clip_lookup_finds_across_tracks() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX2"));
    let on_v2 = timeline.add_clip(v2, generated_clip(100, 20)).unwrap();
    timeline.add_clip(v1, generated_clip(0, 10)).unwrap();

    assert_eq!(timeline.track_of(on_v2), Some(v2));
    assert_eq!(timeline.clip(on_v2).unwrap().start().value, 100);
}

// --- duration ---------------------------------------------------------

#[test]
fn duration_empty_timeline_is_zero() {
    let timeline = Timeline::new(R24);
    assert_eq!(timeline.duration(), rt(0));
}

#[test]
fn duration_is_max_end_across_tracks() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Adjustment, "FX2"));
    timeline.add_clip(v1, generated_clip(0, 100)).unwrap();
    timeline.add_clip(v2, generated_clip(50, 200)).unwrap(); // ends at 250

    assert_eq!(timeline.duration().value, 250);
    assert_eq!(timeline.duration().rate, R24);
}

#[test]
fn duration_ignores_gap_between_clips_on_same_track() {
    let (mut timeline, track) = timeline_with_track();
    timeline.add_clip(track, generated_clip(0, 50)).unwrap();
    timeline.add_clip(track, generated_clip(100, 30)).unwrap(); // ends 130

    assert_eq!(timeline.duration().value, 130);
}

// --- markers ------------------------------------------------------------

fn marker_at(tick: i64) -> Marker {
    Marker::new(rt(tick), "", MarkerColor::Teal)
}

#[test]
fn add_marker_keeps_tick_order_and_resamples() {
    let mut timeline = Timeline::new(R24);
    let late = timeline.add_marker(marker_at(100)).unwrap();
    let early = timeline.add_marker(marker_at(10)).unwrap();
    // 2 s at 48 ticks/s resamples to 24 ticks at the 24 fps timeline.
    let resampled = timeline
        .add_marker(Marker::new(
            RationalTime::new(96, Rational::new(48, 1)),
            "beat",
            MarkerColor::Red,
        ))
        .unwrap();

    let order: Vec<MarkerId> = timeline.markers().iter().map(|m| m.id).collect();
    assert_eq!(order, [early, resampled, late]);
    let beat = timeline.marker(resampled).unwrap();
    assert_eq!(beat.tick, rt(48));
    assert_eq!(beat.name, "beat");
    assert_eq!(beat.color, MarkerColor::Red);
}

#[test]
fn add_marker_rejects_negative_tick_and_duplicate_id() {
    let mut timeline = Timeline::new(R24);
    assert_eq!(
        timeline.add_marker(marker_at(-1)),
        Err(ModelError::InvalidRange)
    );
    let marker = marker_at(5);
    let dup = marker.clone();
    timeline.add_marker(marker).unwrap();
    assert_eq!(timeline.add_marker(dup), Err(ModelError::InvalidRange));
    assert_eq!(timeline.marker_count(), 1);
}

#[test]
fn remove_marker_returns_snapshot_for_undo() {
    let mut timeline = Timeline::new(R24);
    let id = timeline
        .add_marker(Marker::new(rt(7), "drop", MarkerColor::Pink))
        .unwrap();

    let removed = timeline.remove_marker(id).unwrap();
    assert_eq!(removed.name, "drop");
    assert_eq!(timeline.marker_count(), 0);
    assert!(timeline.remove_marker(id).is_none());

    // Restoring the snapshot keeps the same id (undo of remove).
    timeline.add_marker(removed).unwrap();
    assert_eq!(timeline.marker(id).unwrap().tick, rt(7));
}

#[test]
fn set_marker_moves_renames_recolors_annotates_and_resorts() {
    let mut timeline = Timeline::new(R24);
    let a = timeline.add_marker(marker_at(10)).unwrap();
    let b = timeline.add_marker(marker_at(20)).unwrap();

    timeline
        .set_marker(
            a,
            rt(30),
            "outro".into(),
            MarkerColor::Green,
            "fix the fade".into(),
        )
        .unwrap();
    let order: Vec<MarkerId> = timeline.markers().iter().map(|m| m.id).collect();
    assert_eq!(order, [b, a], "tick change re-sorts");
    let moved = timeline.marker(a).unwrap();
    assert_eq!((moved.tick, moved.name.as_str()), (rt(30), "outro"));
    assert_eq!(moved.color, MarkerColor::Green);
    assert_eq!(moved.comment, "fix the fade");

    let missing_marker = MarkerId::next();
    assert_eq!(
        timeline.set_marker(
            missing_marker,
            rt(0),
            String::new(),
            MarkerColor::Teal,
            String::new()
        ),
        Err(ModelError::UnknownMarker(missing_marker))
    );
    // A rejected move leaves the marker untouched, comment included.
    assert_eq!(
        timeline.set_marker(a, rt(-5), String::new(), MarkerColor::Teal, String::new()),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(timeline.marker(a).unwrap().tick, rt(30));
    assert_eq!(timeline.marker(a).unwrap().name, "outro");
    assert_eq!(timeline.marker(a).unwrap().comment, "fix the fade");
}

/// A comment is absent from a save while empty, so a file written before
/// comments existed loads with no comment rather than failing to parse.
#[test]
fn a_marker_comment_round_trips_and_is_absent_while_empty() {
    let bare = serde_json::to_value(Marker::new(rt(5), "cut", MarkerColor::Teal)).unwrap();
    assert!(bare.get("comment").is_none(), "{bare}");
    let loaded: Marker = serde_json::from_value(bare).unwrap();
    assert_eq!(loaded.comment, "");

    let annotated = serde_json::to_value(
        Marker::new(rt(5), "cut", MarkerColor::Teal).with_comment("regrade this"),
    )
    .unwrap();
    assert_eq!(annotated["comment"], "regrade this");
    let loaded: Marker = serde_json::from_value(annotated).unwrap();
    assert_eq!(loaded.comment, "regrade this");
}

#[test]
fn markers_serialize_only_when_present() {
    let mut timeline = Timeline::new(R24);
    let empty = serde_json::to_value(&timeline).unwrap();
    assert!(
        empty.get("markers").is_none(),
        "marker-free timelines serialize without the field"
    );
    // Pre-marker files (no `markers` key) deserialize to an empty list.
    let loaded: Timeline = serde_json::from_value(empty).unwrap();
    assert_eq!(loaded.marker_count(), 0);

    timeline
        .add_marker(Marker::new(rt(12), "intro", MarkerColor::Blue))
        .unwrap();
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(json["markers"][0]["name"], "intro");
    assert_eq!(json["markers"][0]["color"], "blue");
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.markers(), timeline.markers());
}

#[test]
fn marker_color_cycles_through_the_palette() {
    assert_eq!(MarkerColor::cycle(0), MarkerColor::Teal);
    assert_eq!(MarkerColor::cycle(7), MarkerColor::Green);
    assert_eq!(MarkerColor::cycle(8), MarkerColor::Teal);
    for color in MarkerColor::ALL {
        assert_eq!(color.rgba()[3], 0xFF, "palette colors are opaque");
        assert!(!color.name().is_empty());
    }
}

#[test]
fn clip_marker_moves_with_clip_and_validates_duration_against_source_and_sequence() {
    let mut timeline = Timeline::new(R24);
    let track = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let clip = Clip::from_media(crate::MediaId::next(), tr(10, 80), tr(20, 80));
    let clip_id = timeline.add_clip(track, clip).unwrap();

    // Add a clip marker at source tick 35 (15 ticks into the clip -> timeline tick 35 + 10 = 45).
    let marker_id = timeline
        .add_clip_marker(
            clip_id,
            Marker::new(rt(35), "Cue", MarkerColor::Pink)
                .with_duration(rt(20))
                .with_kind(MarkerKind::Chapter),
        )
        .unwrap();
    assert_eq!(timeline.marker_clip(marker_id), Some(clip_id));
    assert_eq!(timeline.marker_count(), 1);
    assert_eq!(
        timeline
            .clip(clip_id)
            .unwrap()
            .timeline_tick_for_clip_time(35),
        Some(45)
    );

    // Move the clip from timeline start 20 to timeline start 120 (+100 ticks).
    timeline
        .clip_timeline_mut(clip_id)
        .unwrap()
        .timeline
        .start
        .value = 120;
    // The clip marker's source tick is still 35, and its timeline position moved by +100 to 145.
    assert_eq!(timeline.marker(marker_id).unwrap().tick, rt(35));
    assert_eq!(
        timeline
            .clip(clip_id)
            .unwrap()
            .timeline_tick_for_clip_time(35),
        Some(145)
    );

    // Refuses negative duration on clip marker.
    let neg_err = timeline
        .edit_marker(
            marker_id,
            rt(35),
            rt(-1),
            "Cue".into(),
            MarkerColor::Pink,
            String::new(),
            MarkerKind::Chapter,
        )
        .unwrap_err();
    assert!(
        neg_err.to_string().contains("non-negative"),
        "expected non-negative error, got: {neg_err}"
    );

    // Refuses duration running past clip source end (10 + 80 = 90; 35 + 60 = 95 > 90).
    let past_clip_err = timeline
        .edit_marker(
            marker_id,
            rt(35),
            rt(60),
            "Cue".into(),
            MarkerColor::Pink,
            String::new(),
            MarkerKind::Chapter,
        )
        .unwrap_err();
    assert!(
        past_clip_err
            .to_string()
            .contains("exceeds clip source end"),
        "expected clip source bound error, got: {past_clip_err}"
    );

    // Sequence marker duration past sequence end (120 + 80 = 200) is refused.
    let seq_marker = timeline
        .add_marker(Marker::new(rt(180), "Tail", MarkerColor::Teal))
        .unwrap();
    let past_seq_err = timeline
        .edit_marker(
            seq_marker,
            rt(180),
            rt(30),
            "Tail".into(),
            MarkerColor::Teal,
            String::new(),
            MarkerKind::Segmentation,
        )
        .unwrap_err();
    assert!(
        past_seq_err.to_string().contains("exceeds sequence end"),
        "expected sequence end error, got: {past_seq_err}"
    );
}

// --- canvas -------------------------------------------------------------

#[test]
fn canvas_defaults_to_auto_black_and_round_trips() {
    let mut timeline = Timeline::new(R24);
    assert!(timeline.canvas().is_default());
    assert_eq!(timeline.canvas().aspect, CanvasAspect::Auto);
    assert_eq!(timeline.canvas().background, [0, 0, 0]);

    // Default settings serialize without the field; pre-canvas files
    // (no `canvas` key) deserialize to the default.
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json.get("canvas").is_none());
    let loaded: Timeline = serde_json::from_value(json).unwrap();
    assert!(loaded.canvas().is_default());

    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Tall9x16,
        background: [20, 30, 40],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(json["canvas"]["aspect"], "9:16");
    assert_eq!(
        json["canvas"]["background"],
        serde_json::json!([20, 30, 40])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas(), timeline.canvas());
}

#[test]
fn canvas_partial_fields_serialize_independently() {
    let mut timeline = Timeline::new(R24);
    // Only the aspect set: the black background stays off the wire.
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(json["canvas"]["aspect"], "1:1");
    assert!(json["canvas"].get("background").is_none());

    // Only the background set: auto aspect stays off the wire.
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [255, 255, 255],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json["canvas"].get("aspect").is_none());
    assert_eq!(
        json["canvas"]["background"],
        serde_json::json!([255, 255, 255])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas().background, [255, 255, 255]);
    assert_eq!(back.canvas().aspect, CanvasAspect::Auto);
}

#[test]
fn canvas_aspect_names_round_trip() {
    for aspect in CanvasAspect::ALL {
        assert_eq!(CanvasAspect::from_name(aspect.name()), Some(aspect));
        match aspect.ratio() {
            None => assert_eq!(aspect, CanvasAspect::Auto),
            Some((w, h)) => {
                assert!(w > 0 && h > 0);
                assert_eq!(aspect.name(), format!("{w}:{h}"));
            }
        }
    }
    assert_eq!(CanvasAspect::from_name("4:3"), None);
}

/// A derived size stays off the wire and an explicit one is written, so a
/// project written before canvas sizes existed still loads, and a delivery size
/// survives a save.
#[test]
fn canvas_resolution_is_optional_on_the_wire() {
    let mut timeline = Timeline::new(R24);
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json["canvas"].get("resolution").is_none());

    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: Some([3840, 2160]),
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(
        json["canvas"]["resolution"],
        serde_json::json!([3840, 2160])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas().resolution, Some([3840, 2160]));
    assert!(!back.canvas().is_default());
}

/// The encodable bounds are the rule every writer checks, so they are stated
/// once: both edges even, both inside `MIN_DIM..=MAX_DIM`.
#[test]
fn canvas_resolution_bounds_refuse_what_cannot_be_encoded() {
    for size in [
        [1920, 1080],
        [CanvasSettings::MIN_DIM, CanvasSettings::MIN_DIM],
        [CanvasSettings::MAX_DIM, CanvasSettings::MAX_DIM],
    ] {
        assert!(
            CanvasSettings::resolution_is_valid(size),
            "{size:?} should be encodable"
        );
    }
    for size in [
        [0, 0],
        [1920, 0],
        [1921, 1080],
        [1920, 1081],
        [CanvasSettings::MIN_DIM - 2, 1080],
        [CanvasSettings::MAX_DIM + 2, 1080],
    ] {
        assert!(
            !CanvasSettings::resolution_is_valid(size),
            "{size:?} should be refused"
        );
    }
}

/// A baseline names the shorter edge, which is what "1080p" means in both a wide
/// and a vertical framing. Every preset resolves; `Auto` has no shape of its own.
#[test]
fn canvas_baseline_pixels_follow_each_preset_shape() {
    assert_eq!(
        CanvasAspect::Wide16x9.pixels_for_baseline(1080),
        Some([1920, 1080])
    );
    assert_eq!(
        CanvasAspect::Tall9x16.pixels_for_baseline(1080),
        Some([1080, 1920])
    );
    assert_eq!(
        CanvasAspect::Square1x1.pixels_for_baseline(720),
        Some([720, 720])
    );
    assert_eq!(CanvasAspect::Auto.pixels_for_baseline(1080), None);
    for aspect in CanvasAspect::ALL {
        let Some(size) = aspect.pixels_for_baseline(2160) else {
            continue;
        };
        assert!(
            CanvasSettings::resolution_is_valid(size),
            "{} at 2160 produced {size:?}, which is not encodable",
            aspect.name()
        );
        assert_eq!(size[0].min(size[1]), 2160);
    }
}

// --- Clone ------------------------------------------------------------

#[test]
fn clone_is_independent_snapshot() {
    let (mut timeline, track) = timeline_with_track();
    let clip = timeline.add_clip(track, generated_clip(0, 50)).unwrap();

    let mut cloned = timeline.clone();
    cloned.remove_clip(clip);
    assert_eq!(cloned.clip_count(), 0);
    assert_eq!(timeline.clip_count(), 1);
}

/// Solo is a per-kind gate, and the gate is the only owner of "does this lane
/// contribute". The class closed here: a solo that silences the soloed lane
/// itself, or that leaks across kinds so soloing a video lane mutes the audio.
#[test]
fn solo_shadows_only_sibling_lanes_of_the_same_kind() {
    let mut timeline = Timeline::new(R24);
    let a = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let b = timeline.add_track(Track::new(TrackKind::Audio, "A2"));
    let v = timeline.add_track(Track::new(TrackKind::Video, "V1"));

    // No solo anywhere: every unmuted, enabled lane contributes.
    assert!(!timeline.has_solo(TrackKind::Audio));
    for id in [a, b] {
        assert!(timeline.track_is_audible(timeline.track(id).unwrap()));
    }
    assert!(timeline.track_is_visible(timeline.track(v).unwrap()));

    timeline.track_mut(a).unwrap().solo = true;
    assert!(timeline.has_solo(TrackKind::Audio));
    assert!(!timeline.has_solo(TrackKind::Video));
    assert!(
        timeline.track_is_audible(timeline.track(a).unwrap()),
        "the soloed lane stays audible"
    );
    assert!(
        !timeline.track_is_audible(timeline.track(b).unwrap()),
        "a sibling audio lane is shadowed"
    );
    assert!(
        timeline.track_is_visible(timeline.track(v).unwrap()),
        "an audio solo must not touch the composite"
    );

    // Solo does not override the lane's own mute.
    timeline.track_mut(a).unwrap().muted = true;
    assert!(!timeline.track_is_audible(timeline.track(a).unwrap()));

    // Clearing the solo restores the sibling.
    timeline.track_mut(a).unwrap().solo = false;
    assert!(timeline.track_is_audible(timeline.track(b).unwrap()));
}

/// The visual side of the same gate, plus its interaction with `enabled`.
#[test]
fn a_visual_solo_drops_sibling_lanes_from_the_composite() {
    let mut timeline = Timeline::new(R24);
    let lower = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let upper = timeline.add_track(Track::new(TrackKind::Video, "V2"));

    timeline.track_mut(upper).unwrap().solo = true;
    assert!(timeline.track_is_visible(timeline.track(upper).unwrap()));
    assert!(!timeline.track_is_visible(timeline.track(lower).unwrap()));

    // A hidden lane stays hidden even while soloed.
    timeline.track_mut(upper).unwrap().enabled = false;
    assert!(!timeline.track_is_visible(timeline.track(upper).unwrap()));
    assert!(
        !timeline.track_is_visible(timeline.track(lower).unwrap()),
        "the solo still shadows the sibling"
    );
}

/// A project written before solo existed must load with no lane soloed, which is
/// indistinguishable from no solo at all.
#[test]
fn a_track_without_a_solo_field_loads_unsoloed() {
    let track: Track = serde_json::from_value(serde_json::json!({
        "id": 1,
        "kind": "Audio",
        "name": "A1",
        "enabled": true,
        "muted": false,
        "locked": false,
        "pinned": false,
        "main": false,
        "clips": [],
    }))
    .expect("legacy track deserializes");
    assert!(!track.solo);
}
