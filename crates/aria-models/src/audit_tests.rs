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
use crate::clip::{ClipSource, Generator};
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
    let project_labels: std::collections::BTreeSet<_> =
        project_facts().iter().map(|spec| spec.label).collect();
    assert_eq!(
        project_labels.len(),
        project_facts().len(),
        "two project facts share a label"
    );
    for spec in clip_facts() {
        assert!(!spec.id.is_empty() && !spec.label.is_empty());
        let lower = spec.label.to_ascii_lowercase();
        assert!(
            !lower.contains("lane") && !spec.label.starts_with("Sounds on "),
            "clip fact '{}' label {:?} uses stale wording",
            spec.id,
            spec.label
        );
    }
    for spec in project_facts() {
        assert!(!spec.id.is_empty() && !spec.label.is_empty());
        assert!(
            !spec.label.to_ascii_lowercase().contains("lane"),
            "project fact '{}' label {:?} uses 'lane' instead of 'track'",
            spec.id,
            spec.label
        );
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
            .filter(|clip| spec.fact.holds(&project, clip))
            .map(|clip| clip.id)
            .collect();
        assert_eq!(
            clips_with_fact(&project, spec.fact),
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
    let found = clips_with_fact(&project, ClipFact::AtACut);
    assert_eq!(
        found.len(),
        1,
        "exactly the left clip of the one junction abuts its neighbor"
    );
}

/// What each question answers for one clip, read the way the model reads it
/// for a single clip: the clip's own predicate, the timeline's per-clip scan,
/// or the lane's abutment rule asked about every neighbor. Exhaustive, so a
/// question added to [`ClipFact`] does not compile here until it has one.
fn read_alone(fact: ClipFact, project: &Project, lane: &Track, clip: &Clip) -> bool {
    let timeline = project.timeline();
    match fact {
        ClipFact::CustomCrop => clip.has_custom_crop(),
        ClipFact::CustomAudio => clip.has_custom_audio(),
        ClipFact::VolumeEnvelope => clip.has_volume_envelope(),
        ClipFact::PanEnvelope => clip.has_pan_envelope(),
        ClipFact::SpeedCurve => clip.has_speed_curve(),
        ClipFact::Beats => clip.has_beats(),
        ClipFact::PitchRidesSpeed => {
            project.clip_insert_refusal(clip, lane).is_ok() && clip.audio_pitch_factor() != 1.0
        }
        ClipFact::OwnAudio => {
            project.clip_sounds_on_own_track(clip, lane, &timeline.audio_lane_links())
        }
        ClipFact::DetachedAudio => {
            project.clip_insert_refusal(clip, lane).is_ok()
                && timeline.detached_to_audio_lane(clip.id)
        }
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
    let mut project = Project::new("alone", R24);
    let v1 = project.add_track(TrackKind::Video, "V1");
    let v2 = project.add_track(TrackKind::Video, "V2");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let media = project.add_media(MediaSource::new("alone.mp4", 1920, 1080, R24, 1000, true));
    let place = |project: &mut Project, lane, start, len, frozen| {
        let mut clip = Clip::from_media(
            media,
            TimeRange::at_rate(0, len, R24),
            TimeRange::at_rate(start, len, R24),
        );
        clip.freeze_frame = frozen;
        project.timeline_mut().add_clip(lane, clip).expect("placed")
    };
    let link = |project: &mut Project, clips: [ClipId; 2]| {
        let group = crate::ids::LinkId::next();
        for clip in clips {
            project.timeline_mut().clip_mut(clip).expect("placed").link = Some(group);
        }
    };
    let picture = place(&mut project, v1, 0, 24, false);
    let sound = place(&mut project, a1, 0, 24, false);
    link(&mut project, [picture, sound]);
    place(&mut project, v1, 24, 24, false);
    let gap = place(&mut project, v1, 60, 12, false);
    let across = place(&mut project, v2, 48, 24, false);
    link(&mut project, [gap, across]);
    let frozen = place(&mut project, v2, 96, 24, true);
    let held = place(&mut project, a1, 96, 24, false);
    link(&mut project, [frozen, held]);
    let before = place(&mut project, v1, 200, 10, false);
    let between = place(&mut project, v1, 210, 0, false);
    place(&mut project, v1, 210, 10, false);
    place(&mut project, v2, 150, 0, false);

    for spec in clip_facts() {
        let found = clips_with_fact(&project, spec.fact);
        let mut expected = Vec::new();
        for lane in project.timeline().tracks_ordered() {
            for clip in lane.clips_ordered() {
                let alone = read_alone(spec.fact, &project, lane, clip);
                let case = format!("'{}' for {:?} on {}", spec.id, clip.id, lane.name);
                assert_eq!(spec.fact.holds(&project, clip), alone, "{case}");
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
        sorted(clips_with_fact(&project, ClipFact::AtACut)),
        sorted(vec![picture, before, between]),
        "the two junctions on V1, and nothing across lanes"
    );
    assert_eq!(
        sorted(clips_with_fact(&project, ClipFact::DetachedAudio)),
        sorted(vec![picture, sound, held]),
        "every sounding member of a link reaching an audio lane"
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

/// Every source kind in [`ClipSource`], mapped without a wildcard so a new
/// variant fails to compile here until this suite covers both its sounding and
/// silent placements.
fn source_kind(source: &ClipSource) -> &'static str {
    match source {
        ClipSource::Media { .. } => "media",
        ClipSource::Generated(_) => "generated",
        ClipSource::Multicam { .. } => "multicam",
        ClipSource::Sequence { .. } => "sequence",
    }
}

/// Sound-bearing clip questions ([`ClipFact::OwnAudio`],
/// [`ClipFact::PitchRidesSpeed`], and [`ClipFact::DetachedAudio`]) hold only
/// when the clip's source and track carry sound.
///
/// The class closed here is a clip question checking track kind, link state, or
/// retiming fields without checking [`Project::clip_insert_refusal`] /
/// [`Project::clip_sounds_on_own_track`]. The fixture places every
/// [`ClipSource`] variant in both silent and sounding forms (including the
/// Sample Project shape of a 1x1 still on V1 and a title on T1, a silent video,
/// a freeze frame, a solid generator, a silent multicam group, and a nested
/// sequence containing only silent clips), plus a cyclic nested sequence. What
/// this does not catch: a media probe misreporting [`MediaSource::has_audio`]
/// at import time.
#[test]
fn sound_questions_hold_only_for_sources_with_sound() {
    let mut project = Project::new("sources", R24);
    let v1 = project.add_track(TrackKind::Video, "V1");
    let s1 = project.add_track(TrackKind::Sticker, "S1");
    let t1 = project.add_track(TrackKind::Text, "T1");
    let a1 = project.add_track(TrackKind::Audio, "A1");

    let video_sound_media = project.add_media(MediaSource::new(
        "video_sound.mp4",
        1920,
        1080,
        R24,
        240,
        true,
    ));
    let video_silent_media = project.add_media(MediaSource::new(
        "video_silent.mp4",
        1920,
        1080,
        R24,
        240,
        false,
    ));
    let still_media = project.add_media(MediaSource::image("still.png", 1, 1));
    let audio_media = project.add_media(MediaSource::new("music.wav", 0, 0, R24, 240, true));

    let mc_silent = project
        .create_multicam_group(
            "silent_cam",
            vec![crate::multicam::MulticamAngle::new(video_silent_media, 0)],
        )
        .expect("silent multicam");
    let mc_sound = project
        .create_multicam_group(
            "sound_cam",
            vec![
                crate::multicam::MulticamAngle::new(video_silent_media, 0),
                crate::multicam::MulticamAngle::new(video_sound_media, 0),
            ],
        )
        .expect("sounding multicam");

    let nested_silent = project.create_sequence("silent_nest", R24);
    {
        let seq = project.sequence_mut(nested_silent).expect("silent_nest");
        let nv1 = seq.timeline.add_track(Track::new(TrackKind::Video, "V1"));
        let nt1 = seq.timeline.add_track(Track::new(TrackKind::Text, "T1"));
        seq.timeline
            .add_clip(
                nv1,
                Clip::from_media(
                    still_media,
                    TimeRange::at_rate(0, 24, R24),
                    TimeRange::at_rate(0, 24, R24),
                ),
            )
            .expect("nested still");
        seq.timeline
            .add_clip(
                nv1,
                Clip::from_media(
                    video_silent_media,
                    TimeRange::at_rate(0, 24, R24),
                    TimeRange::at_rate(24, 24, R24),
                ),
            )
            .expect("nested silent video");
        seq.timeline
            .add_clip(
                nt1,
                Clip::generated(
                    Generator::text("Nested title"),
                    TimeRange::at_rate(0, 24, R24),
                ),
            )
            .expect("nested title");
    }

    let nested_sound = project.create_sequence("sound_nest", R24);
    {
        let seq = project.sequence_mut(nested_sound).expect("sound_nest");
        let nv1 = seq.timeline.add_track(Track::new(TrackKind::Video, "V1"));
        seq.timeline
            .add_clip(
                nv1,
                Clip::from_media(
                    video_sound_media,
                    TimeRange::at_rate(0, 24, R24),
                    TimeRange::at_rate(0, 24, R24),
                ),
            )
            .expect("nested sounding video");
    }

    let place_on = |project: &mut Project, lane, mut clip: Clip| -> ClipId {
        clip.speed = Rational::new(2, 1);
        clip.preserve_pitch = false;
        project.timeline_mut().add_clip(lane, clip).expect("placed")
    };
    let span = |slot: i64| TimeRange::at_rate(slot * 24, 24, R24);
    let src = TimeRange::at_rate(0, 24, R24);

    let video_with_audio = place_on(
        &mut project,
        v1,
        Clip::from_media(video_sound_media, src, span(0)),
    );
    let video_without_audio = place_on(
        &mut project,
        v1,
        Clip::from_media(video_silent_media, src, span(1)),
    );
    let still_clip = place_on(
        &mut project,
        v1,
        Clip::from_media(still_media, src, span(2)),
    );
    let mut frozen = Clip::from_media(video_sound_media, src, span(3));
    frozen.freeze_frame = true;
    let frozen_clip = place_on(&mut project, v1, frozen);
    let solid_on_s1 = place_on(
        &mut project,
        s1,
        Clip::generated(
            Generator::SolidColor {
                rgba: [0, 0, 0, 255],
            },
            span(0),
        ),
    );
    let title_on_t1 = place_on(
        &mut project,
        t1,
        Clip::generated(Generator::text("Title"), span(0)),
    );
    let multicam_silent_clip = place_on(
        &mut project,
        v1,
        Clip::from_multicam(mc_silent, src, span(4)),
    );
    let multicam_sound_clip = place_on(
        &mut project,
        v1,
        Clip::from_multicam(mc_sound, src, span(5)),
    );
    let seq_silent_clip = place_on(
        &mut project,
        v1,
        Clip::from_sequence(nested_silent, src, span(6)),
    );
    let seq_sound_clip = place_on(
        &mut project,
        v1,
        Clip::from_sequence(nested_sound, src, span(7)),
    );
    let audio_file_clip = place_on(
        &mut project,
        a1,
        Clip::from_media(audio_media, src, span(0)),
    );

    let cases = [
        ("video_with_audio", video_with_audio, true),
        ("video_without_audio", video_without_audio, false),
        ("still", still_clip, false),
        ("freeze_frame", frozen_clip, false),
        ("solid_generator", solid_on_s1, false),
        ("title_generator", title_on_t1, false),
        ("multicam_silent", multicam_silent_clip, false),
        ("multicam_with_sound", multicam_sound_clip, true),
        ("nested_silent_sequence", seq_silent_clip, false),
        ("nested_sequence_with_sound", seq_sound_clip, true),
        ("audio_file", audio_file_clip, true),
    ];

    let links = project.timeline().audio_lane_links();
    let v1_lane = project.timeline().track(v1).expect("V1");
    let solid_clip_ref = project.timeline().clip(solid_on_s1).expect("solid");
    assert!(
        !project.clip_sounds_on_own_track(solid_clip_ref, v1_lane, &links),
        "generated clip must report false even against a mixer track"
    );

    let mut covered_kinds = std::collections::BTreeSet::new();
    for (name, id, expected_sound) in cases {
        let clip = project.timeline().clip(id).expect(name);
        let lane = project
            .timeline()
            .track(project.timeline().track_of(id).expect(name))
            .expect(name);
        covered_kinds.insert(source_kind(&clip.content));
        assert_eq!(
            project.clip_sounds_on_own_track(clip, lane, &links),
            expected_sound,
            "clip_sounds_on_own_track for {name}"
        );
        assert_eq!(
            ClipFact::OwnAudio.holds(&project, clip),
            expected_sound,
            "OwnAudio::holds for {name}"
        );
        assert_eq!(
            ClipFact::PitchRidesSpeed.holds(&project, clip),
            expected_sound,
            "PitchRidesSpeed::holds for {name}"
        );
    }
    assert_eq!(
        covered_kinds,
        std::collections::BTreeSet::from(["media", "generated", "multicam", "sequence"]),
        "every ClipSource variant must be exercised"
    );

    let expected_sounding = vec![
        audio_file_clip,
        video_with_audio,
        multicam_sound_clip,
        seq_sound_clip,
    ];
    assert_eq!(
        clips_with_fact(&project, ClipFact::OwnAudio),
        expected_sounding,
        "OwnAudio must include only sounding clips"
    );
    assert_eq!(
        clips_with_fact(&project, ClipFact::PitchRidesSpeed),
        expected_sounding,
        "PitchRidesSpeed must include only sounding clips"
    );

    let mut sample = Project::new("sample", R24);
    let sample_v1 = sample.add_track(TrackKind::Video, "V1");
    let sample_t1 = sample.add_track(TrackKind::Text, "T1");
    let sample_still = sample.add_media(MediaSource::image("still.png", 1, 1));
    sample
        .add_clip(
            sample_v1,
            sample_still,
            TimeRange::at_rate(0, 1000, crate::media::STILL_TICK_RATE),
            crate::time::RationalTime::new(0, R24),
        )
        .expect("still on V1");
    sample
        .add_generated(
            sample_t1,
            Generator::text("Title"),
            TimeRange::at_rate(0, 24, R24),
        )
        .expect("title on T1");
    assert!(
        clips_with_fact(&sample, ClipFact::OwnAudio).is_empty(),
        "Sample Project (1x1 still on V1 and title on T1) must report 0 for own_audio"
    );

    let group = crate::ids::LinkId::next();
    for (_, id, _) in cases {
        project.timeline_mut().clip_mut(id).expect("placed").link = Some(group);
    }
    for (name, id, expected_sound) in cases {
        let clip = project.timeline().clip(id).expect(name);
        assert_eq!(
            ClipFact::DetachedAudio.holds(&project, clip),
            expected_sound,
            "DetachedAudio::holds for {name}"
        );
    }
    assert_eq!(
        clips_with_fact(&project, ClipFact::DetachedAudio),
        expected_sounding,
        "DetachedAudio must include only sounding clips in the link group"
    );

    let cycle_a = project.create_sequence("cycle_a", R24);
    let cycle_b = project.create_sequence("cycle_b", R24);
    for (holder, target) in [(cycle_a, cycle_b), (cycle_b, cycle_a)] {
        let seq = project.sequence_mut(holder).expect("cycle seq");
        let nv1 = seq.timeline.add_track(Track::new(TrackKind::Video, "V1"));
        seq.timeline
            .add_clip(nv1, Clip::from_sequence(target, src, span(0)))
            .expect("cycle clip");
    }
    let cyclic_clip = place_on(&mut project, v1, Clip::from_sequence(cycle_a, src, span(8)));
    let cyclic = project.timeline().clip(cyclic_clip).expect("cyclic clip");
    let v1_lane = project.timeline().track(v1).expect("V1");
    assert!(!project.clip_sounds_on_own_track(
        cyclic,
        v1_lane,
        &project.timeline().audio_lane_links()
    ));
    assert!(!ClipFact::OwnAudio.holds(&project, cyclic));
}
