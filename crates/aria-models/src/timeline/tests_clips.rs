use super::*;

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
