//! [`Track::revision`] and [`Track::clip_revision`] follow the content, and a
//! lane or clip nothing wrote to is not hashed again.
//!
//! WHY: the timeline view rebuilds a lane only when its revision moved, and a
//! clip of a rebuilt lane only when the clip's revision moved. A revision that
//! misses a write leaves a stale lane or clip on screen; one that moves with no
//! change (a refused edit, a pass over every lane) rebuilds for nothing; and
//! one that hashes every clip of a lane on each write serializes the lane per
//! edit, a sixth of `AppState::sync` in an Inspector scrub on `project-10k`.
//! Every write to a clip reaches it through `LaneClips`, which has no
//! `DerefMut`; `every_clip_hand_out_drops_the_kept_revision` reaches each of
//! its write methods through the hand-outs that call them, including a whole
//! clip written over one handed out. What these do not catch: a write method
//! added to `LaneClips` that keeps the revision. The property test over every
//! edit command in `aria-ui` (`projection_lanes_tests`) runs the real edits.

use super::*;
use crate::Timeline;
use crate::time::Rational;

/// Lanes hashed on this thread so far.
fn hashed() -> usize {
    super::HASHED.with(std::cell::Cell::get)
}

/// Clips hashed on this thread so far.
fn clips_hashed() -> usize {
    crate::track_clips::CLIPS_HASHED.with(std::cell::Cell::get)
}

/// The revision `track` takes when nothing is kept: the lane as a reload of
/// its save reads it.
fn fresh(track: &Track) -> u64 {
    let json = serde_json::to_string(track).expect("the lane serializes");
    serde_json::from_str::<Track>(&json)
        .expect("the lane loads")
        .revision()
}

/// `lanes` lanes, each named apart and holding `clips` abutting solid-colour
/// clips.
fn timeline(lanes: usize, clips: i64) -> (Timeline, Vec<TrackId>) {
    let rate = Rational::FPS_24;
    let solid = Generator::SolidColor {
        rgba: [200, 40, 40, 255],
    };
    let kind = TrackKind::for_generator(&solid).expect("a generator names its lane");
    let mut timeline = Timeline::new(rate);
    let ids: Vec<TrackId> = (0..lanes)
        .map(|i| timeline.add_track(Track::new(kind, format!("V{i}"))))
        .collect();
    for &id in &ids {
        for c in 0..clips {
            let range = TimeRange::at_rate(c * 10, 10, rate);
            timeline
                .add_clip(id, Clip::generated(solid.clone(), range))
                .expect("abutting clips fit");
        }
    }
    (timeline, ids)
}

fn revisions(timeline: &Timeline, ids: &[TrackId]) -> Vec<u64> {
    ids.iter()
        .map(|&id| timeline.track(id).expect("lane exists").revision())
        .collect()
}

#[test]
fn a_revision_moves_with_the_content_and_only_with_it() {
    let (mut timeline, ids) = timeline(3, 4);
    let before = revisions(&timeline, &ids);
    assert!(
        before[0] != before[1] && before[1] != before[2] && before[0] != before[2],
        "lanes with different content share a revision: {before:?}"
    );

    let _ = timeline.track_mut(ids[1]).expect("lane exists");
    assert_eq!(
        revisions(&timeline, &ids),
        before,
        "a lane handed out and left as it was moved"
    );

    timeline.track_mut(ids[1]).expect("lane exists").muted = true;
    let muted = revisions(&timeline, &ids);
    assert_ne!(muted[1], before[1], "the written lane kept its revision");
    assert_eq!(
        (muted[0], muted[2]),
        (before[0], before[2]),
        "a lane nothing wrote to moved"
    );

    timeline.track_mut(ids[1]).expect("lane exists").muted = false;
    assert_eq!(
        revisions(&timeline, &ids),
        before,
        "the lane written back to its content took another revision"
    );

    assert_eq!(revisions(&timeline.clone(), &ids), before, "a clone moved");
    let json = serde_json::to_string(&timeline).expect("the timeline serializes");
    let loaded: Timeline = serde_json::from_str(&json).expect("the timeline loads");
    assert_eq!(revisions(&loaded, &ids), before, "a reload moved");
}

#[test]
fn a_lane_nothing_wrote_to_is_not_hashed_again() {
    let (mut timeline, ids) = timeline(50, 20);
    let start = hashed();
    let first = revisions(&timeline, &ids);
    assert_eq!(hashed() - start, 50, "each lane hashes on its first read");

    assert_eq!(revisions(&timeline, &ids), first);
    assert_eq!(hashed() - start, 50, "a second read hashed again");

    timeline.track_mut(ids[7]).expect("lane exists").solo = true;
    let written = revisions(&timeline, &ids);
    assert_ne!(written[7], first[7]);
    assert_eq!(
        hashed() - start,
        51,
        "a write to one lane hashed more than that lane"
    );

    let clone = timeline.clone();
    assert_eq!(revisions(&clone, &ids), written);
    assert_eq!(hashed() - start, 51, "a clone hashed its lanes again");
}

#[test]
fn a_write_to_one_clip_hashes_that_clip_and_no_other() {
    let (mut timeline, ids) = timeline(3, 20);
    let start = clips_hashed();
    let first = revisions(&timeline, &ids);
    assert_eq!(
        clips_hashed() - start,
        60,
        "each clip hashes on its first read"
    );

    let lane = timeline.track(ids[1]).expect("lane exists");
    let clip = lane.clips_ordered().nth(5).expect("the lane holds 20").id;
    let clip_revisions = |timeline: &Timeline| -> Vec<u64> {
        let lane = timeline.track(ids[1]).expect("lane exists");
        lane.clips_ordered()
            .map(|c| lane.clip_revision(c.id).expect("the clip is on the lane"))
            .collect()
    };
    let before = clip_revisions(&timeline);

    timeline.clip_mut(clip).expect("clip exists").name = Some("renamed".into());
    let start = clips_hashed();
    let written = revisions(&timeline, &ids);
    assert_eq!(
        clips_hashed() - start,
        1,
        "a write to one clip hashed more than that clip"
    );
    assert_ne!(written[1], first[1], "the written lane kept its revision");
    assert_eq!((written[0], written[2]), (first[0], first[2]));
    let after = clip_revisions(&timeline);
    for (index, (was, now)) in before.iter().zip(&after).enumerate() {
        assert_eq!(
            was == now,
            index != 5,
            "clip {index}: only the written clip takes another revision"
        );
    }
    let lane = timeline.track(ids[1]).expect("lane exists");
    assert_eq!(
        written[1],
        fresh(lane),
        "the kept revision is not the content's"
    );

    timeline.clip_mut(clip).expect("clip exists").name = None;
    assert_eq!(
        revisions(&timeline, &ids),
        first,
        "the clip written back to its content took another lane revision"
    );
    assert_eq!(clip_revisions(&timeline), before);
}

/// Write the name of `clip` on `lane` through one hand-out.
type Write = fn(&mut Timeline, TrackId, ClipId);

#[test]
fn every_clip_hand_out_drops_the_kept_revision() {
    fn named() -> Option<String> {
        Some("written".into())
    }
    let writes: [(&str, Write); 9] = [
        ("Timeline::clip_mut", |t, _, c| {
            t.clip_mut(c).expect("clip").name = named();
        }),
        ("a whole clip written over Timeline::clip_mut", |t, _, c| {
            // The copy is taken after the clip was hashed, so a hash kept with
            // the clip would come back with it.
            let mut clip = t.clip(c).expect("clip").clone();
            clip.name = named();
            *t.clip_mut(c).expect("clip") = clip;
        }),
        ("Track::clip_mut", |t, l, c| {
            t.track_mut(l)
                .expect("lane")
                .clip_mut(c)
                .expect("clip")
                .name = named();
        }),
        ("Timeline::clip_timeline_mut", |t, _, c| {
            t.clip_timeline_mut(c).expect("clip").name = named();
        }),
        ("Track::clip_timeline_mut", |t, l, c| {
            let lane = t.track_mut(l).expect("lane");
            lane.clip_timeline_mut(c).expect("clip").name = named();
        }),
        ("Track::for_each_clip_mut", |t, l, c| {
            let lane = t.track_mut(l).expect("lane");
            lane.for_each_clip_mut(|clip| {
                if clip.id == c {
                    clip.name = named();
                }
            });
        }),
        ("Track::try_for_each_clip_mut", |t, l, c| {
            let lane = t.track_mut(l).expect("lane");
            let wrote: Result<(), ()> = lane.try_for_each_clip_mut(|clip| {
                if clip.id == c {
                    clip.name = named();
                }
                Ok(())
            });
            wrote.expect("the pass completes");
        }),
        ("Timeline::replace_clip", |t, _, c| {
            let mut clip = t.clip(c).expect("clip").clone();
            clip.name = named();
            t.replace_clip(clip).expect("the clip fits where it was");
        }),
        ("Timeline::add_clip of a removed clip", |t, l, c| {
            let mut clip = t.remove_clip(c).expect("clip");
            clip.name = named();
            t.add_clip(l, clip).expect("the clip fits where it was");
        }),
    ];
    for (path, write) in writes {
        let (mut timeline, ids) = timeline(1, 4);
        let lane = ids[0];
        let track = timeline.track(lane).expect("lane exists");
        let clip = track.clips_ordered().nth(1).expect("the lane holds 4").id;
        let before = (
            track.revision(),
            track.clip_revision(clip).expect("the clip is on the lane"),
        );
        write(&mut timeline, lane, clip);
        let track = timeline.track(lane).expect("lane exists");
        let after = (
            track.revision(),
            track.clip_revision(clip).expect("the clip is on the lane"),
        );
        assert!(
            after.0 != before.0 && after.1 != before.1,
            "{path}: the write kept a revision: {before:?} -> {after:?}"
        );
        assert_eq!(after.0, fresh(track), "{path}: the kept revision is stale");
    }
}
