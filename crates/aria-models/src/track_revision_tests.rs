//! [`Track::revision`] follows the lane's content, and a lane nothing wrote to
//! is not hashed again.
//!
//! WHY: the timeline view rebuilds a lane only when its revision moved. A
//! revision that misses a write leaves a stale lane on screen; one that moves
//! with no change (a refused edit, a pass over every lane) rebuilds lanes for
//! nothing; and one that hashes every lane on every read serializes the whole
//! sequence per projection, a quarter of an edit's projection on
//! `project-10k`. What these do not catch: a write that reaches a lane without
//! the timeline's `&mut Track` hand-out. The property test over every edit
//! command in `aria-ui` (`projection_lanes_tests`) runs the real edits for
//! that.

use super::*;
use crate::Timeline;
use crate::time::Rational;

/// Lanes hashed on this thread so far.
fn hashed() -> usize {
    super::HASHED.with(std::cell::Cell::get)
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
