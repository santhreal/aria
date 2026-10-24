//! What the audit catalogs must hold.
//!
//! The class closed here is "a question was added and nothing answers it".
//! Every case enumerates [`clip_facts`] or [`project_facts`] at run time and
//! compares sets, so a row added to either table is exercised the moment it
//! exists and a row that disappears takes its assertion with it. Nothing here
//! lists a variant or a count.
//!
//! What this does not catch, stated plainly: a row whose `test` answers the
//! wrong question. The compile-time gate is [`super::index_of`] and the
//! identity case below proves an arm points at its own row, not that the
//! predicate behind it is the one the label names.

use super::*;
use crate::clip::Generator;
use crate::media::MediaSource;
use crate::project::Project;
use crate::time::{Rational, TimeRange};
use crate::track::TrackKind;

const R24: Rational = Rational::FPS_24;

/// A sequence with two abutting generated clips on a lane that takes
/// transitions, plus one media clip and one pool folder — enough state that at
/// least one row answers `true` and the counts are not all zero.
fn dressed() -> Project {
    let mut project = Project::new("audit", R24);
    let stickers = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            stickers,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            TimeRange::at_rate(0, 24, R24),
        )
        .expect("left");
    project
        .add_generated(
            stickers,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            TimeRange::at_rate(24, 24, R24),
        )
        .expect("right");
    let video = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(MediaSource::new(
        "/tmp/audit.mp4",
        1920,
        1080,
        R24,
        1000,
        true,
    ));
    project
        .add_clip(
            video,
            media,
            TimeRange::at_rate(0, 50, R24),
            crate::time::RationalTime::new(0, R24),
        )
        .expect("media clip");
    project.add_bin("Footage", None).expect("a folder");
    project
}

/// Every arm of the exhaustive match points at its own row, and every row is
/// reachable by its id. Both sides are derived from the table.
#[test]
fn a_row_per_clip_fact() {
    for (index, spec) in clip_facts().iter().enumerate() {
        assert_eq!(
            index_of(spec.fact),
            index,
            "the '{}' row resolves to '{}' instead",
            spec.id,
            clip_facts()[index_of(spec.fact)].id
        );
        assert_eq!(spec.fact.id(), spec.id);
        assert_eq!(
            clip_fact_by_id(spec.id).map(|found| found.fact),
            Some(spec.fact),
            "'{}' is not reachable by id",
            spec.id
        );
    }
}

#[test]
fn a_row_per_project_fact() {
    for (index, spec) in project_facts().iter().enumerate() {
        assert_eq!(
            spec.fact.id(),
            spec.id,
            "the '{}' row is described by '{}'",
            spec.id,
            spec.fact.id()
        );
        assert_eq!(
            project_facts()[index].id,
            spec.id,
            "'{}' moved out of declaration order",
            spec.id
        );
    }
}

/// Ids and labels identify a row on screen and on the wire; two rows
/// sharing either makes one of them unaddressable.
#[test]
fn every_row_is_uniquely_named() {
    let clip_ids: std::collections::BTreeSet<_> = clip_facts().iter().map(|spec| spec.id).collect();
    assert_eq!(
        clip_ids.len(),
        clip_facts().len(),
        "two clip rows share an id"
    );
    let clip_labels: std::collections::BTreeSet<_> =
        clip_facts().iter().map(|spec| spec.label).collect();
    assert_eq!(
        clip_labels.len(),
        clip_facts().len(),
        "two clip rows share a label"
    );
    let project_ids: std::collections::BTreeSet<_> =
        project_facts().iter().map(|spec| spec.id).collect();
    assert_eq!(
        project_ids.len(),
        project_facts().len(),
        "two project rows share an id"
    );
    for spec in clip_facts() {
        assert!(!spec.label.is_empty(), "'{}' has no label", spec.id);
    }
    for spec in project_facts() {
        assert!(!spec.label.is_empty(), "'{}' has no label", spec.id);
    }
}

/// A question must be answerable for every clip of a dressed sequence without
/// panicking, and [`clips_with_fact`] must agree with asking each clip.
#[test]
fn every_clip_fact_agrees_with_asking_each_clip() {
    let project = dressed();
    let timeline = project.timeline();
    for spec in clip_facts() {
        let expected: Vec<_> = timeline
            .tracks_ordered()
            .flat_map(|track| track.clips_ordered())
            .filter(|clip| spec.fact.holds(timeline, clip))
            .map(|clip| clip.id)
            .collect();
        assert_eq!(
            clips_with_fact(timeline, spec.fact),
            expected,
            "'{}' disagrees with itself",
            spec.id
        );
    }
}

/// Every project row must read something for a dressed project and for an empty
/// one: a blank readout reports nothing.
#[test]
fn every_project_fact_reads_on_a_dressed_and_an_empty_project() {
    let empty = Project::new("empty", R24);
    let dressed = dressed();
    for spec in project_facts() {
        for (which, project) in [("empty", &empty), ("dressed", &dressed)] {
            let value = spec.fact.read(project).text(|tick| tick.to_string());
            assert!(
                !value.trim().is_empty(),
                "'{}' reads nothing on the {which} project",
                spec.id
            );
        }
    }
}

/// A readout value starts the way every other drawn line does: with a digit or
/// a capital. Every answer a row can give is reached here: zero, no and none
/// on an empty project, counts on a dressed one, and yes plus a soloed lane
/// list once a junction carries a transition and a lane is soloed. The empty
/// answers, and only those, read as [`FactValue::is_empty`], which is what
/// draws a row dim.
///
/// Does not catch a value that is correctly cased but worded wrong.
#[test]
fn every_project_fact_reads_in_sentence_case() {
    let empty = Project::new("empty", R24);
    let dressed = dressed();
    let mut joined = dressed.clone();
    let (lane, left) = joined
        .timeline()
        .tracks_ordered()
        .find(|track| track.kind == TrackKind::Sticker)
        .map(|track| (track.id, track.clips_ordered().next().expect("a clip").id))
        .expect("the sticker lane");
    joined
        .add_transition(left, "crossfade")
        .expect("a crossfade");
    joined
        .timeline_mut()
        .track_mut(lane)
        .expect("the lane")
        .solo = true;
    for (which, project) in [
        ("empty", &empty),
        ("dressed", &dressed),
        ("joined", &joined),
    ] {
        for spec in project_facts() {
            let value = spec.fact.read(project).text(|tick| tick.to_string());
            let first = value.chars().next().expect("a value");
            assert!(
                first.is_ascii_digit() || first.is_uppercase(),
                "'{}' reads {value:?} on the {which} project",
                spec.id
            );
        }
    }
    for fact in [
        ProjectFact::ContentEnd,
        ProjectFact::Transitions,
        ProjectFact::SoloedKinds,
    ] {
        assert!(
            fact.read(&empty).is_empty(),
            "'{}' on an empty project is not the empty answer",
            fact.id()
        );
        assert!(
            !fact.read(&joined).is_empty(),
            "'{}' on a joined project reads as empty",
            fact.id()
        );
    }
}

/// The abutting pair is the one state the table is dressed for, so the cut row
/// must find both of them: a fact table whose rows all answer `false` would
/// pass every case above.
#[test]
fn the_cut_row_finds_the_abutting_pair() {
    let project = dressed();
    let found = clips_with_fact(project.timeline(), ClipFact::AtACut);
    assert_eq!(
        found.len(),
        1,
        "exactly the left clip of the one junction abuts its neighbour"
    );
}

/// What each question answers for one clip, read the way the model reads it
/// for a single clip: the clip's own predicate, the timeline's per-clip scan,
/// or the lane's abutment rule asked about every neighbour. Exhaustive, so a
/// question added to [`ClipFact`] does not compile here until it has one.
fn read_alone(fact: ClipFact, timeline: &Timeline, lane: &Track, clip: &Clip) -> bool {
    match fact {
        ClipFact::CustomCrop => clip.has_custom_crop(),
        ClipFact::CustomAudio => clip.has_custom_audio(),
        ClipFact::VolumeEnvelope => clip.has_volume_envelope(),
        ClipFact::PanEnvelope => clip.has_pan_envelope(),
        ClipFact::SpeedCurve => clip.has_speed_curve(),
        ClipFact::Beats => clip.has_beats(),
        ClipFact::PitchRidesSpeed => clip.audio_pitch_factor() != 1.0,
        ClipFact::OwnAudio => timeline.carries_own_audio(clip.id),
        ClipFact::DetachedAudio => timeline.detached_to_audio_lane(clip.id),
        ClipFact::AtACut => lane
            .clips()
            .any(|other| other.id != clip.id && lane.clips_abut(clip.id, other.id)),
    }
}

/// Every row answers as the model reads the same question for one clip, for
/// every clip, whether asked through [`clips_with_fact`] or
/// [`ClipFact::holds`].
///
/// The class: a row answered from a read a walk takes once (the audio-lane
/// link set, the lane's start order) that disagrees with the per-clip read it
/// replaced. The shapes: a picture linked to an audio-lane partner, a linked
/// freeze frame, two picture clips linked only to each other, a junction, a
/// gap, a clip starting on another lane where one ends, a zero-length clip at
/// a junction, and a lone zero-length clip whose start is its own end. What
/// this does not catch: a per-clip read and a row that are wrong the same way.
#[test]
fn every_row_answers_as_the_clip_alone_reads() {
    let mut timeline = Timeline::new(R24);
    let v1 = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let v2 = timeline.add_track(Track::new(TrackKind::Video, "V2"));
    let a1 = timeline.add_track(Track::new(TrackKind::Audio, "A1"));
    let media = crate::ids::MediaId::from_raw(1);
    let place = |timeline: &mut Timeline, lane, start, len, frozen| {
        let mut clip = Clip::from_media(
            media,
            TimeRange::at_rate(0, len, R24),
            TimeRange::at_rate(start, len, R24),
        );
        clip.freeze_frame = frozen;
        timeline.add_clip(lane, clip).expect("placed")
    };
    let link = |timeline: &mut Timeline, clips: [ClipId; 2]| {
        let group = crate::ids::LinkId::next();
        for clip in clips {
            timeline.clip_mut(clip).expect("placed").link = Some(group);
        }
    };
    let picture = place(&mut timeline, v1, 0, 24, false);
    let sound = place(&mut timeline, a1, 0, 24, false);
    link(&mut timeline, [picture, sound]);
    place(&mut timeline, v1, 24, 24, false);
    let gap = place(&mut timeline, v1, 60, 12, false);
    let across = place(&mut timeline, v2, 48, 24, false);
    link(&mut timeline, [gap, across]);
    let frozen = place(&mut timeline, v2, 96, 24, true);
    let held = place(&mut timeline, a1, 96, 24, false);
    link(&mut timeline, [frozen, held]);
    let before = place(&mut timeline, v1, 200, 10, false);
    let between = place(&mut timeline, v1, 210, 0, false);
    place(&mut timeline, v1, 210, 10, false);
    place(&mut timeline, v2, 150, 0, false);

    for spec in clip_facts() {
        let found = clips_with_fact(&timeline, spec.fact);
        let mut expected = Vec::new();
        for lane in timeline.tracks_ordered() {
            for clip in lane.clips_ordered() {
                let alone = read_alone(spec.fact, &timeline, lane, clip);
                let case = format!("'{}' for {:?} on {}", spec.id, clip.id, lane.name);
                assert_eq!(spec.fact.holds(&timeline, clip), alone, "{case}");
                if alone {
                    expected.push(clip.id);
                }
            }
        }
        assert_eq!(found, expected, "'{}' over the sequence", spec.id);
    }
    let sorted = |mut ids: Vec<ClipId>| {
        ids.sort_unstable_by_key(|id| id.raw());
        ids
    };
    assert_eq!(
        sorted(clips_with_fact(&timeline, ClipFact::AtACut)),
        sorted(vec![picture, before, between]),
        "the two junctions on V1, and nothing across lanes"
    );
    assert_eq!(
        sorted(clips_with_fact(&timeline, ClipFact::DetachedAudio)),
        sorted(vec![picture, sound, frozen, held]),
        "every member of a link reaching an audio lane"
    );
}

/// Soloing one lane must show up in the soloed-kinds readout, and clearing it
/// must take it away. The label comes from the lane catalog, so this also
/// proves the readout is derived rather than written here.
#[test]
fn soloing_a_lane_shows_up_in_the_readout() {
    let mut project = dressed();
    let before = ProjectFact::SoloedKinds.read(&project);
    assert_eq!(before, FactValue::Names(Vec::new()));
    assert_eq!(before.text(|tick| tick.to_string()), "None");
    assert!(before.is_empty(), "an empty list reads as the empty answer");
    let lane = project
        .timeline()
        .tracks_ordered()
        .next()
        .expect("a lane")
        .id;
    project
        .timeline_mut()
        .track_mut(lane)
        .expect("the lane")
        .solo = true;
    let kind = project.timeline().track(lane).expect("the lane").kind;
    let after = ProjectFact::SoloedKinds.read(&project);
    assert_eq!(
        after,
        FactValue::Names(vec![crate::track_catalog::track_kind_spec(kind).label])
    );
}
