use super::*;
use crate::ids::MarkerId;

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
