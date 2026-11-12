//! Clips shared between a project and its copies.
//!
//! WHY: a copy of the project, taken for a save, an autosave, a filmstrip
//! render or an export, copied every clip, so an edit or an undo made while a
//! copy was held copied the whole timeline first: 10 to 20 ms on a timeline of
//! 8000 clips, on the window's thread. A lane holds each clip behind a shared
//! reference. For every write a lane takes, these tests hold that the copy
//! still reads what it read before the write, that the write lands, and that
//! the clips the write did not reach are still shared; and that a copy costs
//! less than copying the clips.
//!
//! Not caught: a write that reaches a clip without going through `LaneClips`.

use std::time::{Duration, Instant};

use crate::clip::Generator;
use crate::ids::{ClipId, TrackId};
use crate::project::Project;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

/// `lanes` title lanes of `per_lane` one-second titles a second apart.
fn timeline(lanes: usize, per_lane: usize) -> (Project, Vec<TrackId>, Vec<ClipId>) {
    let mut project = Project::new("shared", Rational::FPS_24);
    let mut clips = Vec::new();
    let lanes: Vec<TrackId> = (0..lanes)
        .map(|lane| project.add_track(TrackKind::Text, format!("T{lane}")))
        .collect();
    for &lane in &lanes {
        for index in 0..per_lane {
            let span = TimeRange::at_rate(48 * index as i64, 24, Rational::FPS_24);
            let clip = project
                .add_generated(lane, Generator::text("Title"), span)
                .expect("a title");
            clips.push(clip);
        }
    }
    (project, lanes, clips)
}

fn shared(one: &Project, other: &Project, id: ClipId) -> bool {
    match (one.clip(id), other.clip(id)) {
        (Some(one), Some(other)) => std::ptr::eq(one, other),
        _ => false,
    }
}

fn document(project: &Project) -> String {
    project.save_to_string().expect("serialize")
}

/// A write to the project, given a lane and a clip on it; whether it reaches
/// that clip; and whether the other clips of the lane stay shared.
type Write = fn(&mut Project, TrackId, ClipId);

fn writes() -> [(&'static str, Write, bool, bool); 5] {
    [
        (
            "a field",
            |project, _, clip| {
                project.clip_mut(clip).expect("the clip").name = Some("changed".into());
            },
            true,
            true,
        ),
        (
            "a clip written over",
            |project, lane, clip| {
                let mut copy = project.clip(clip).expect("the clip").clone();
                copy.name = Some("changed".into());
                let track = project.timeline_mut().track_mut(lane).expect("the lane");
                assert!(
                    track.insert_clip(copy).is_some(),
                    "nothing was written over"
                );
            },
            true,
            true,
        ),
        (
            "a removal",
            |project, _, clip| {
                project.remove_clip(clip).expect("removed");
            },
            true,
            true,
        ),
        (
            "an addition",
            |project, lane, _| {
                let span = TimeRange::at_rate(100_000, 24, Rational::FPS_24);
                project
                    .add_generated(lane, Generator::text("New"), span)
                    .expect("a new title");
            },
            false,
            true,
        ),
        (
            "every clip of a lane",
            |project, lane, _| {
                let track = project.timeline_mut().track_mut(lane).expect("the lane");
                track.for_each_clip_mut(|clip| clip.name = Some("changed".into()));
            },
            true,
            false,
        ),
    ]
}

#[test]
fn a_write_after_a_copy_leaves_the_copy_and_shares_what_it_did_not_reach() {
    for (name, write, reaches, keeps_others) in writes() {
        let (mut project, lanes, clips) = timeline(2, 3);
        let held = project.clone();
        let before = document(&held);
        for &clip in &clips {
            assert!(
                shared(&held, &project, clip),
                "{name}: a copy copied a clip"
            );
        }
        let reached = clips[1];
        write(&mut project, lanes[0], reached);
        assert_eq!(
            document(&held),
            before,
            "{name}: the write changed the copy"
        );
        assert_ne!(document(&project), before, "{name}: the write was lost");
        assert_eq!(
            shared(&held, &project, reached),
            !reaches,
            "{name}: the clip the write was given"
        );
        let others = clips.iter().filter(|&&clip| clip != reached);
        let lane_two = &clips[3..];
        for clip in others {
            let expected = keeps_others || lane_two.contains(clip);
            assert_eq!(
                shared(&held, &project, *clip),
                expected,
                "{name}: clip {clip:?} not reached by the write"
            );
        }
    }
}

fn fastest(mut work: impl FnMut()) -> Duration {
    (0..7)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .min()
        .expect("a run")
}

#[test]
fn a_copy_of_a_large_timeline_costs_less_than_copying_its_clips() {
    let (project, _, _) = timeline(8, 1000);
    let copy = fastest(|| {
        std::hint::black_box(project.clone());
    });
    let clips = fastest(|| {
        let every: Vec<_> = project
            .timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips().cloned())
            .collect();
        std::hint::black_box(every);
    });
    println!("copy of 8000 clips {copy:?}; copying the clips {clips:?}");
    assert!(
        copy * 4 < clips,
        "a copy took {copy:?} against {clips:?} to copy the clips: it copies them"
    );
}
