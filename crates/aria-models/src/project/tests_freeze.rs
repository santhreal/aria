//! [`Project::freeze_check`], each refusal read from a state that holds it,
//! and the [`FreezeRule`] it applies, against the project at every tick.
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

/// The freeze rules as the project reads them at the tick asked: the
/// definition [`FreezeRule`] answers from what it read once.
fn per_tick(project: &Project, clip_id: ClipId, at: i64) -> Result<(), ModelError> {
    let refuse = |why| Err(ModelError::Freeze(why));
    let timeline = project.timeline();
    let track_id = timeline.track_of(clip_id).expect("the clip's track");
    let track = timeline.require_track(track_id)?;
    let clip = timeline.clip(clip_id).expect("the clip");
    let start = clip.timeline.start.value;
    let end = clip.timeline.end()?.value;
    if at < start || at > end {
        return refuse(FreezeRefusal::OffClip);
    }
    if at == start && (clip.animation_in.is_some() || clip.animation_combo.is_some()) {
        return refuse(FreezeRefusal::Entrance);
    }
    if at == end && (clip.animation_out.is_some() || clip.animation_combo.is_some()) {
        return refuse(FreezeRefusal::Exit);
    }
    let members = |link: LinkId| {
        timeline
            .tracks_ordered()
            .flat_map(|lane| lane.clips())
            .filter(move |member| member.link == Some(link))
    };
    if clip
        .link
        .is_some_and(|link| members(link).any(|member| member.id != clip_id))
    {
        return refuse(FreezeRefusal::Linked);
    }
    let splits = track
        .clips()
        .filter(|shifted| shifted.timeline.start.value >= at)
        .filter_map(|shifted| shifted.link)
        .any(|link| {
            members(link).any(|member| {
                timeline.track_of(member.id) != Some(track_id) || member.timeline.start.value < at
            })
        });
    if splits {
        return refuse(FreezeRefusal::SplitsLink);
    }
    if start < at && at < end {
        project.split_check(clip_id, at)?;
    }
    Ok(())
}

/// A seeded splitmix64 stream, so a failing layout is named by its seed.
struct Draw(u64);

impl Draw {
    fn below(&mut self, bound: u64) -> i64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % bound) as i64
    }
}

/// A [`FreezeRule`] read once from a clip answers what the project answers at
/// every tick from two before the clip to two after it, over drawn layouts:
/// three lanes of abutting and spaced clips, each in one of three links or
/// none, the frozen clip with or without a fade in and edge animations.
///
/// Closes the class "a rule read once disagrees with the project at a tick it
/// was not read at": the link refusals depend on the tick, and the rule holds
/// them as tick intervals, so an interval off by one tick, a partner on
/// another lane, or a link of one clip shows up as a seed and a tick. Not
/// covered: the refusals before the extent, which hold at every tick and
/// which `every_refusal_is_read_from_the_state_that_holds_it` reads.
#[test]
fn a_rule_read_once_answers_the_project_at_every_tick() {
    for seed in 0..300 {
        let mut draw = Draw(seed);
        let mut project = Project::new("drawn", R);
        let picture = project.add_media(MediaSource::new("shot.mp4", 1920, 1080, R, 10_000, true));
        let sound = project.add_media(MediaSource::new("take.wav", 0, 0, R, 10_000, true));
        let lanes = [
            (project.add_track(TrackKind::Video, "V1"), picture),
            (project.add_track(TrackKind::Video, "V2"), picture),
            (project.add_track(TrackKind::Audio, "A1"), sound),
        ];
        let links = [LinkId::next(), LinkId::next(), LinkId::next()];
        let mut first_lane = Vec::new();
        for (index, (lane, media)) in lanes.into_iter().enumerate() {
            let mut at = draw.below(20);
            for _ in 0..1 + draw.below(4) {
                let length = 4 + draw.below(30);
                let placed = project
                    .add_clip(lane, media, tr(0, length), RationalTime::new(at, R))
                    .expect("a drawn clip");
                let link = links.get(draw.below(5) as usize).copied();
                project
                    .timeline_mut()
                    .clip_mut(placed)
                    .expect("the clip")
                    .link = link;
                if index == 0 {
                    first_lane.push(placed);
                }
                at += length + draw.below(10);
            }
        }
        let clip = first_lane[draw.below(first_lane.len() as u64) as usize];
        let fade = draw.below(8);
        let (entrance, exit) = (draw.below(4) == 0, draw.below(4) == 0);
        let held = project.timeline_mut().clip_mut(clip).expect("the clip");
        held.fade_in = fade;
        held.animation_in = entrance.then(|| AnimationRef::new("fade_in"));
        held.animation_out = exit.then(|| AnimationRef::new("fade_out"));
        let rule = project.freeze_rule(clip).expect("the rule");
        let span = project.timeline().clip(clip).expect("the clip").timeline;
        let end = span.end().expect("the clip's end").value;
        for at in span.start.value - 2..=end + 2 {
            assert_eq!(
                rule.check(at),
                per_tick(&project, clip, at),
                "seed {seed}, at {at} of {}..={end}",
                span.start.value
            );
        }
    }
}
