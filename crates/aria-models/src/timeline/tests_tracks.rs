use super::*;

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
