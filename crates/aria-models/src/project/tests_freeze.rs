//! [`Project::freeze_check`], each refusal read from a state that holds it.
//!
//! Closes the class "a freeze rule the check misses": the state for each
//! refusal comes from a match over [`FreezeRefusal::ALL`] with no catch-all,
//! so a new refusal stops compiling here until a state reaches it, and every
//! state but the one dressed differs from a clip the check passes. Not
//! covered: whether the engine's freeze edit applies the check, which the
//! engine's `freeze_frame` tests read at every tick around a clip.

use super::*;
use crate::{
    AnimationRef, MediaSource, MulticamGroupId, Rational, RationalTime, SplitRefusal, TimeRange,
};

const R: Rational = Rational::FPS_24;

fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R)
}

/// A video clip over ticks 10..60 on V1, and the clip after it at tick 80.
fn fixture() -> (Project, ClipId, ClipId) {
    let mut project = Project::new("freeze", R);
    let media = project.add_media(MediaSource::new("shot.mp4", 1920, 1080, R, 1_000, true));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(track, media, tr(100, 50), RationalTime::new(10, R))
        .expect("the clip");
    let next = project
        .add_clip(track, media, tr(300, 20), RationalTime::new(80, R))
        .expect("the next clip");
    (project, clip, next)
}

/// A project, a clip and a tick the check refuses with `refusal`.
fn refused(refusal: FreezeRefusal) -> (Project, ClipId, i64) {
    let (mut project, clip, next) = fixture();
    let mut at = 30;
    match refusal {
        FreezeRefusal::NotOnVideoTrack => {
            let media = project.add_media(MediaSource::new("shot.mp4", 1920, 1080, R, 240, true));
            let lane = project.add_track(TrackKind::Audio, "A1");
            let sound = project
                .add_clip(lane, media, tr(0, 40), RationalTime::new(0, R))
                .expect("a video file on a sound lane");
            return (project, sound, 10);
        }
        FreezeRefusal::TrackLocked => {
            let track = project.timeline().track_of(clip).expect("the clip's track");
            project.timeline_mut().track_mut(track).expect("V1").locked = true;
        }
        FreezeRefusal::NotMedia => {
            let held = project.timeline_mut().clip_mut(clip).expect("the clip");
            held.content = ClipSource::Multicam {
                group: MulticamGroupId::next(),
                source: tr(0, 50),
            };
        }
        FreezeRefusal::AlreadyFrozen => {
            project
                .timeline_mut()
                .clip_mut(clip)
                .expect("the clip")
                .freeze_frame = true;
        }
        FreezeRefusal::NoVideo => {
            let still = project.add_media(MediaSource::image("still.png", 1920, 1080));
            let lane = project.add_track(TrackKind::Video, "V2");
            let range = project.media(still).expect("the still").full_range();
            let placed = project
                .add_clip(lane, still, range, RationalTime::new(0, R))
                .expect("the still clip");
            return (project, placed, 0);
        }
        FreezeRefusal::OffClip => at = 61,
        FreezeRefusal::Entrance => {
            let held = project.timeline_mut().clip_mut(clip).expect("the clip");
            held.animation_in = Some(AnimationRef::new("fade_in"));
            at = 10;
        }
        FreezeRefusal::Exit => {
            let held = project.timeline_mut().clip_mut(clip).expect("the clip");
            held.animation_out = Some(AnimationRef::new("fade_out"));
            at = 60;
        }
        FreezeRefusal::Linked => {
            let link = LinkId::next();
            for member in [clip, next] {
                project
                    .timeline_mut()
                    .clip_mut(member)
                    .expect("a clip")
                    .link = Some(link);
            }
        }
        FreezeRefusal::SplitsLink => {
            let media = project.add_media(MediaSource::new("take.wav", 0, 0, R, 240, true));
            let lane = project.add_track(TrackKind::Audio, "A1");
            let sound = project
                .add_clip(lane, media, tr(0, 20), RationalTime::new(80, R))
                .expect("the sound");
            let link = LinkId::next();
            for member in [next, sound] {
                project
                    .timeline_mut()
                    .clip_mut(member)
                    .expect("a clip")
                    .link = Some(link);
            }
        }
    }
    (project, clip, at)
}

#[test]
fn every_refusal_is_read_from_the_state_that_holds_it() {
    let (project, clip, _) = fixture();
    for at in [10, 30, 60] {
        assert_eq!(
            project.freeze_check(clip, at),
            Ok(()),
            "the bare clip at {at}"
        );
    }
    for refusal in FreezeRefusal::ALL {
        let (project, clip, at) = refused(refusal);
        assert_eq!(
            project.freeze_check(clip, at),
            Err(ModelError::Freeze(refusal)),
            "{refusal:?}"
        );
    }
}

/// A tick inside the clip is refused with the refusal of the cut the freeze
/// makes there, and a tick on the clip's edge makes no cut.
#[test]
fn a_tick_inside_the_clip_reads_the_refusal_of_its_cut() {
    let (mut project, clip, _) = fixture();
    project
        .timeline_mut()
        .clip_mut(clip)
        .expect("the clip")
        .fade_in = 20;
    assert_eq!(
        project.freeze_check(clip, 20),
        Err(ModelError::Split(SplitRefusal::InsideFadeIn))
    );
    assert_eq!(project.freeze_check(clip, 10), Ok(()));
    assert_eq!(project.freeze_check(clip, 30), Ok(()));
}
