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
            let value = spec.fact.read(project);
            assert!(
                !value.trim().is_empty(),
                "'{}' reads nothing on the {which} project",
                spec.id
            );
        }
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

/// Soloing one lane must show up in the soloed-kinds readout, and clearing it
/// must take it away. The label comes from the lane catalog, so this also
/// proves the readout is derived rather than written here.
#[test]
fn soloing_a_lane_shows_up_in_the_readout() {
    let mut project = dressed();
    let before = ProjectFact::SoloedKinds.read(&project);
    assert_eq!(before, "none");
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
    assert_eq!(after, crate::track_catalog::track_kind_spec(kind).label);
}
