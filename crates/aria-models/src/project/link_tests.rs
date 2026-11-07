//! Link, group and enable plans, and the setters that apply them.
//!
//! WHY: a plan naming a clip twice captured its inverse in plan order, so undo
//! wrote the clip's old value and then the value the plan had just set, and
//! the clip stayed in the new link. A link of one clip, or of one clip named
//! twice, succeeded and moved nothing with it. An unlink or ungroup of clips in
//! no link or group was refused as "invalid time range". Each setter is driven
//! with a plan that names a clip twice; each refusal is read by its variant and
//! its text. Does not cover which clips an unlink or a regroup sweeps in.

use super::*;
use crate::{Clip, ClipSetRefusal, Generator, GroupId, Rational, TimeRange, TrackKind};

/// Three clips on one lane, the first two linked and grouped, all enabled.
fn three_clips() -> (Project, [ClipId; 3]) {
    let mut project = Project::new("links", Rational::FPS_24);
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let clips = [0, 20, 40].map(|start| {
        project
            .timeline_mut()
            .add_clip(
                track,
                Clip::generated(
                    Generator::Adjustment,
                    TimeRange::at_rate(start, 10, Rational::FPS_24),
                ),
            )
            .expect("a clip on an empty span")
    });
    let [a, b, _] = clips;
    project.link_clips(&[a, b]).expect("two clips link");
    let plan = project.group_plan(&[a, b]).expect("two clips group");
    project
        .set_clip_groups(&plan)
        .expect("a group plan applies");
    (project, clips)
}

/// What the three setters write, per clip.
fn state(project: &Project, clips: &[ClipId]) -> Vec<(Option<LinkId>, Option<GroupId>, bool)> {
    clips
        .iter()
        .map(|&id| {
            let clip = project.clip(id).expect("a clip of the fixture");
            (clip.link, clip.group, clip.enabled)
        })
        .collect()
}

#[test]
fn a_plan_naming_a_clip_twice_undoes_to_the_value_before_it() {
    let (mut project, clips) = three_clips();
    let [a, _, c] = clips;
    let before = state(&project, &clips);

    let link = LinkId::next();
    let undo = project
        .set_clip_links(&[(a, Some(link)), (a, None), (c, Some(link))])
        .expect("a link plan applies");
    project.set_clip_links(&undo).expect("its inverse applies");
    assert_eq!(state(&project, &clips), before, "links after undo");

    let group = GroupId::next();
    let undo = project
        .set_clip_groups(&[(a, Some(group)), (a, None), (c, Some(group))])
        .expect("a group plan applies");
    project.set_clip_groups(&undo).expect("its inverse applies");
    assert_eq!(state(&project, &clips), before, "groups after undo");

    let undo = project
        .set_clips_enabled(&[(a, false), (a, false), (c, false)])
        .expect("an enable plan applies");
    project
        .set_clips_enabled(&undo)
        .expect("its inverse applies");
    assert_eq!(state(&project, &clips), before, "enabled after undo");
}

#[test]
fn a_link_or_group_of_fewer_than_two_different_clips_is_refused() {
    let (project, [a, b, _]) = three_clips();
    for clips in [&[][..], &[a], &[a, a]] {
        assert_eq!(
            project.link_plan(clips),
            Err(ModelError::ClipSet(ClipSetRefusal::LinkOfOne)),
            "link of {clips:?}"
        );
        assert_eq!(
            project.group_plan(clips),
            Err(ModelError::ClipSet(ClipSetRefusal::GroupOfOne)),
            "group of {clips:?}"
        );
    }
    let plan = project
        .link_plan(&[b, a, b])
        .expect("two different clips link");
    let named: Vec<ClipId> = plan.iter().map(|&(clip, _)| clip).collect();
    assert_eq!(named, [b, a], "each clip once, in the order named");
    assert!(plan[0].1.is_some() && plan[0].1 == plan[1].1);
}

#[test]
fn a_request_with_nothing_to_act_on_states_what_is_missing() {
    let (project, [_, _, c]) = three_clips();
    let cases = [
        (project.unlink_plan(&[]).err(), ClipSetRefusal::NoClips),
        (
            project.unlink_plan(&[c, c]).err(),
            ClipSetRefusal::NoneLinked,
        ),
        (project.ungroup_plan(&[]).err(), ClipSetRefusal::NoClips),
        (
            project.ungroup_plan(&[c]).err(),
            ClipSetRefusal::NoneGrouped,
        ),
        (
            project.enable_plan(&[], true).err(),
            ClipSetRefusal::NoClips,
        ),
    ];
    for (got, refusal) in cases {
        assert_eq!(got, Some(ModelError::ClipSet(refusal)));
    }
    let texts: Vec<String> = ClipSetRefusal::ALL
        .map(|refusal| ModelError::ClipSet(refusal).to_string())
        .into();
    assert_eq!(
        texts,
        [
            "no clips were named",
            "a link needs at least two different clips",
            "a group needs at least two different clips",
            "none of these clips is linked",
            "none of these clips is in a group",
        ]
    );
}
