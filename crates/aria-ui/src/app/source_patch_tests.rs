//! Which lane a Source tab's channel lands on, as the lane headers draw it.
//!
//! The lit patch button and the lane an edit lands on are read from one
//! resolution, [`UiSequence::patched_lane`]. These pin that resolution for every
//! patch a channel can hold: `Auto` with and without a targeted lane, a lane, a
//! lane since removed, and off. Each case also asserts that exactly the resolved
//! lanes are lit, so a header lit on a lane the edit would not use goes red.
//!
//! Not covered: the lane header drawing the button. The registry row is driven
//! by `drawn_census`, and the header is pinned to the registry there.

use aria_models::{Patch, Project, Rational, RationalTime, SourceChannel, TrackId, TrackKind};

use crate::projection::project_to_ui_with;
use crate::types::UiSequence;

struct Lanes {
    v1: TrackId,
    v2: TrackId,
    a1: TrackId,
    a2: TrackId,
    t1: TrackId,
}

fn project() -> (Project, Lanes) {
    let mut project = Project::new("patch", Rational::FPS_24);
    let lanes = Lanes {
        v1: project.add_track(TrackKind::Video, "V1"),
        v2: project.add_track(TrackKind::Video, "V2"),
        t1: project.add_track(TrackKind::Text, "T1"),
        a1: project.add_track(TrackKind::Audio, "A1"),
        a2: project.add_track(TrackKind::Audio, "A2"),
    };
    (project, lanes)
}

fn sequence(project: &Project) -> UiSequence {
    project_to_ui_with(
        project,
        RationalTime::new(0, Rational::FPS_24),
        &[],
        false,
        false,
        false,
    )
    .sequence
}

fn target(project: &mut Project, lane: TrackId, targeted: bool) {
    project
        .timeline_mut()
        .track_mut(lane)
        .expect("lane")
        .targeted = targeted;
}

fn patch(project: &mut Project, channel: SourceChannel, patch: Patch) {
    project
        .timeline_mut()
        .set_source_patch(channel, patch)
        .expect("a patch of the channel's kind");
}

/// Resolve both channels and check the header lights those lanes and no other.
fn resolved(project: &Project) -> (Option<u64>, Option<u64>) {
    let sequence = sequence(project);
    let video = sequence.patched_lane(SourceChannel::Video);
    let audio = sequence.patched_lane(SourceChannel::Audio);
    let mut lit: Vec<u64> = sequence
        .tracks
        .iter()
        .filter(|lane| lane.patched)
        .map(|lane| lane.id)
        .collect();
    lit.sort_unstable();
    let mut expected: Vec<u64> = video.into_iter().chain(audio).collect();
    expected.sort_unstable();
    assert_eq!(
        lit, expected,
        "the headers light the lanes the edit lands on"
    );
    (video, audio)
}

#[test]
fn auto_lands_on_the_topmost_targeted_lane_else_the_topmost_lane() {
    let (mut project, lanes) = project();
    // Picture lanes stack upward from the divider, so V2 is drawn above V1;
    // sound lanes stack downward, so A1 is drawn above A2.
    assert_eq!(
        resolved(&project),
        (Some(lanes.v2.raw()), Some(lanes.a1.raw()))
    );

    target(&mut project, lanes.v2, false);
    target(&mut project, lanes.a1, false);
    assert_eq!(
        resolved(&project),
        (Some(lanes.v1.raw()), Some(lanes.a2.raw())),
        "an untargeted lane is passed over"
    );

    for lane in [lanes.v1, lanes.a2] {
        target(&mut project, lane, false);
    }
    assert_eq!(
        resolved(&project),
        (Some(lanes.v2.raw()), Some(lanes.a1.raw())),
        "with nothing targeted, the topmost lane of the kind"
    );
}

#[test]
fn a_lane_patch_overrides_targeting_until_the_lane_is_removed() {
    let (mut project, lanes) = project();
    patch(&mut project, SourceChannel::Video, Patch::Track(lanes.v1));
    patch(&mut project, SourceChannel::Audio, Patch::Track(lanes.a2));
    assert_eq!(
        resolved(&project),
        (Some(lanes.v1.raw()), Some(lanes.a2.raw()))
    );

    project
        .timeline_mut()
        .remove_track(lanes.v1)
        .expect("remove V1");
    assert_eq!(
        resolved(&project),
        (Some(lanes.v2.raw()), Some(lanes.a2.raw())),
        "a patch to a removed lane resolves as Auto"
    );
}

#[test]
fn an_off_channel_lands_nowhere_and_leaves_the_other_alone() {
    let (mut project, lanes) = project();
    patch(&mut project, SourceChannel::Video, Patch::Off);
    assert_eq!(resolved(&project), (None, Some(lanes.a1.raw())));
    patch(&mut project, SourceChannel::Audio, Patch::Off);
    patch(&mut project, SourceChannel::Video, Patch::Auto);
    assert_eq!(resolved(&project), (Some(lanes.v2.raw()), None));
}

#[test]
fn a_lane_that_takes_no_source_is_never_patched() {
    let (mut project, lanes) = project();
    for lane in [lanes.v1, lanes.v2] {
        project.timeline_mut().remove_track(lane).expect("remove");
    }
    let sequence = sequence(&project);
    let title = sequence
        .tracks
        .iter()
        .find(|lane| lane.id == lanes.t1.raw())
        .expect("the title lane is projected");
    assert_eq!(title.channel, None);
    assert!(!title.patched);
    assert_eq!(
        sequence.patched_lane(SourceChannel::Video),
        None,
        "a sequence with no picture lane has nowhere to put a picture"
    );
}
