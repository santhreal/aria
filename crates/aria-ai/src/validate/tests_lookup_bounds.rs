//! Tests for validation lookup id bounds (Row 268) and overlap validations (Row 269).

use super::*;
use aria_models::{Marker, MediaId, MediaSource, RationalTime, TimeRange, TrackKind};

use crate::wire;

#[test]
fn list_ids_formats_empty_and_small_lists() {
    assert_eq!(list_ids(vec![]), "none");
    assert_eq!(list_ids(vec![42]), "42");
    assert_eq!(list_ids(vec![100, 3, 50, 7]), "3, 7, 50, 100");
}

#[test]
fn list_ids_exact_10_boundary_and_capped_overflow() {
    // Exactly 10 items: no overflow suffix
    let ten_ids: Vec<u64> = (1..=10).rev().collect();
    assert_eq!(list_ids(ten_ids), "1, 2, 3, 4, 5, 6, 7, 8, 9, 10");

    // 11 items: 10 items + (and 1 more)
    let eleven_ids: Vec<u64> = (1..=11).rev().collect();
    assert_eq!(
        list_ids(eleven_ids),
        "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (and 1 more)"
    );

    // 25 items: 10 items + (and 15 more)
    let twenty_five_ids: Vec<u64> = (1..=25).rev().collect();
    assert_eq!(
        list_ids(twenty_five_ids),
        "1, 2, 3, 4, 5, 6, 7, 8, 9, 10 (and 15 more)"
    );
}

#[test]
fn entity_lookups_bound_id_lists_in_rejection_messages() {
    let mut project = Project::new("large_project", R24);

    // Add 25 tracks
    let mut track_ids = Vec::new();
    for i in 0..25 {
        let t = project.add_track(TrackKind::Video, format!("V{i}"));
        track_ids.push(t.raw());
    }

    // Add 25 media sources
    let mut media_ids = Vec::new();
    for i in 0..25 {
        let m = project.add_media(MediaSource::new(
            format!("/tmp/media_{i}.mp4"),
            1920,
            1080,
            R24,
            240,
            true,
        ));
        media_ids.push(m.raw());
    }

    // Add 25 clips (one per track)
    let mut clip_ids = Vec::new();
    for (i, (&t_raw, &m_raw)) in track_ids.iter().zip(media_ids.iter()).enumerate() {
        let c = project
            .add_clip(
                aria_models::TrackId::from_raw(t_raw),
                MediaId::from_raw(m_raw),
                TimeRange::at_rate(0, 48, R24),
                RationalTime::new((i as i64) * 48, R24),
            )
            .unwrap();
        clip_ids.push(c.raw());
    }

    // Add 25 markers
    let mut marker_ids = Vec::new();
    for i in 0..25 {
        let m = project
            .timeline_mut()
            .add_marker(Marker::new(
                RationalTime::new((i as i64) * 24, R24),
                format!("M{i}"),
                aria_models::MarkerColor::Blue,
            ))
            .unwrap();
        marker_ids.push(m.raw());
    }

    // 1. Unknown clip rejection bounds listed clip IDs to 10 + count
    let err = reject(
        &project,
        WireCommand::TrimClip(wire::TrimClip {
            clip: 999999,
            start: 0.0,
            duration: 1.0,
        }),
    );
    assert!(err.contains("clip 999999 does not exist; clips on the timeline:"));
    assert!(err.contains("(and 15 more)"), "rejection message: {err}");

    // 2. Unknown track rejection bounds listed track IDs to 10 + count
    let err = reject(
        &project,
        WireCommand::RemoveTrack(wire::RemoveTrack { track: 999999 }),
    );
    assert!(err.contains("track 999999 does not exist; tracks on the timeline:"));
    assert!(err.contains("(and 15 more)"), "rejection message: {err}");

    // 3. Unknown media rejection bounds listed media IDs to 10 + count
    let err = reject(
        &project,
        WireCommand::AddClip(wire::AddClip {
            track: track_ids[0],
            media: 999999,
            start: 100.0,
            source_start: 0.0,
            source_duration: 1.0,
        }),
    );
    assert!(err.contains("media 999999 does not exist; media in the pool:"));
    assert!(err.contains("(and 15 more)"), "rejection message: {err}");

    // 4. Unknown marker rejection bounds listed marker IDs to 10 + count
    let err = reject(
        &project,
        WireCommand::RemoveMarker(wire::RemoveMarker { marker: 999999 }),
    );
    assert!(err.contains("marker 999999 does not exist; markers on the timeline:"));
    assert!(err.contains("(and 15 more)"), "rejection message: {err}");
}

#[test]
fn extract_audio_linked_clip_list_is_bounded_and_zero_alloc_on_success() {
    let mut project = Project::new("link_test", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/video.mp4",
        1920,
        1080,
        R24,
        240,
        true,
    ));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");

    let video_clip = project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 48, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();

    // Success path: unlinked video clip extracts audio to A1 without error or allocation
    let lowered = lower(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip.raw(),
            track: a1.raw(),
        }),
    );
    assert_eq!(
        lowered,
        EditCommand::ExtractAudio {
            clip: video_clip,
            to_track: Some(a1),
        }
    );

    // Rejection path: link with 15 other clips
    let link = aria_models::LinkId::next();
    project.timeline_mut().clip_mut(video_clip).unwrap().link = Some(link);
    for i in 0..15 {
        let track = project.add_track(TrackKind::Text, format!("T{i}"));
        let companion = project
            .add_generated(
                track,
                Generator::text(format!("text_{i}")),
                TimeRange::at_rate(0, 48, R24),
            )
            .unwrap();
        project.timeline_mut().clip_mut(companion).unwrap().link = Some(link);
    }

    let err = reject(
        &project,
        WireCommand::ExtractAudio(wire::ExtractAudio {
            clip: video_clip.raw(),
            track: a1.raw(),
        }),
    );
    assert!(err.contains("call unlink_clips first"));
    assert!(err.contains("(and 5 more)"), "rejection message: {err}");
}

#[test]
fn duplicate_clip_overlap_validation_and_success_path() {
    let mut project = Project::new("overlap_test", R24);
    let media = project.add_media(MediaSource::new("/tmp/vid.mp4", 1920, 1080, R24, 240, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let v2 = project.add_track(TrackKind::Video, "V2");

    let clip = project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 48, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();

    // Place existing clip on V2 from 0s to 2s (0..48 ticks)
    project
        .add_clip(
            v2,
            media,
            TimeRange::at_rate(0, 48, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();

    // Success path: duplicate at 2.0s (48 ticks) has no overlap and validates cleanly
    let lowered = lower(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip: clip.raw(),
            to_track: v2.raw(),
            start: 2.0,
        }),
    );
    assert_eq!(
        lowered,
        EditCommand::DuplicateClip {
            clip,
            to_track: v2,
            start: RationalTime::new(48, R24),
        }
    );

    // Overlap rejection: duplicate at 1.0s (24 ticks) collides with 0..48
    let err = reject(
        &project,
        WireCommand::DuplicateClip(wire::DuplicateClip {
            clip: clip.raw(),
            to_track: v2.raw(),
            start: 1.0,
        }),
    );
    assert!(err.contains("has a clip overlapping duplicate of clip"));
    assert!(err.contains("exact destination range 1.000s to 3.000s"));
}
