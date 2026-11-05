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
fn auto_lands_on_the_targeted_lane_nearest_the_divide_else_the_nearest_lane() {
    let (mut project, lanes) = project();
    // Picture lanes stack upward from the divider, so V1 is nearest the
    // divider (below V2); sound lanes stack downward, so A1 is nearest the
    // divider (above A2).
    assert_eq!(
        resolved(&project),
        (Some(lanes.v1.raw()), Some(lanes.a1.raw()))
    );

    target(&mut project, lanes.v1, false);
    target(&mut project, lanes.a1, false);
    assert_eq!(
        resolved(&project),
        (Some(lanes.v2.raw()), Some(lanes.a2.raw())),
        "an untargeted lane is passed over"
    );

    for lane in [lanes.v2, lanes.a2] {
        target(&mut project, lane, false);
    }
    assert_eq!(
        resolved(&project),
        (Some(lanes.v1.raw()), Some(lanes.a1.raw())),
        "with nothing targeted, the lane of the kind nearest the divide"
    );
}

#[test]
fn a_lane_patch_overrides_targeting_until_the_lane_is_removed() {
    let (mut project, lanes) = project();
    patch(&mut project, SourceChannel::Video, Patch::Track(lanes.v2));
    patch(&mut project, SourceChannel::Audio, Patch::Track(lanes.a2));
    assert_eq!(
        resolved(&project),
        (Some(lanes.v2.raw()), Some(lanes.a2.raw()))
    );

    project
        .timeline_mut()
        .remove_track(lanes.v2)
        .expect("remove V2");
    assert_eq!(
        resolved(&project),
        (Some(lanes.v1.raw()), Some(lanes.a2.raw())),
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
    assert_eq!(resolved(&project), (Some(lanes.v1.raw()), None));
}

#[test]
fn adding_a_video_or_audio_lane_keeps_auto_patch_and_source_edit_on_v1_and_a1() {
    let mut state = crate::app::AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("source-patch-divide"),
    )
    .expect("state");
    state.new_project().expect("project");
    let v1 = state
        .store
        .sequence
        .patched_lane(SourceChannel::Video)
        .expect("V1 is patched initially");
    let a1 = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("add A1")
        .raw();
    assert_eq!(
        state.store.sequence.patched_lane(SourceChannel::Audio),
        Some(a1)
    );

    // Pressing the Timeline's + adds a targeted video lane at the top of the
    // stack; adding another audio lane appends below A1. Both Auto patches and
    // the next source edit must stay on V1 and A1 nearest the divide.
    state
        .add_track(TrackKind::Video, "V4")
        .expect("add top video lane");
    let a2 = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("add A2")
        .raw();
    assert_ne!(a1, a2);
    assert_eq!(
        state.store.sequence.patched_lane(SourceChannel::Video),
        Some(v1),
        "Auto video patch stays on V1 when a new video lane is added above it"
    );
    assert_eq!(
        state.store.sequence.patched_lane(SourceChannel::Audio),
        Some(a1),
        "Auto audio patch stays on A1 when a new audio lane is added below it"
    );

    let media = state
        .import_media(crate::app::fixtures::asset(
            "fixture-640x360-24fps-3s-av.mp4",
        ))
        .expect("import video with sound");
    let lanes = state
        .source_lanes(media, None)
        .expect("resolve source lanes")
        .expect("both channels patched");
    assert_eq!(lanes.picture, Some(TrackId::from_raw(v1)));
    assert_eq!(lanes.sound.lane(), Some(TrackId::from_raw(a1)));
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
