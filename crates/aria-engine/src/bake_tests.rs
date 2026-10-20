//! What a bake pass may contain, and what it may not.
//!
//! The class these close: a rendered intermediate that is not what the clip
//! looked like. Two ways that happens, and both are silent — the file shows
//! more than the clip (a neighbour, a half transition, a lane bar's grade that
//! belongs to the programme) or less than it (the clip's own effects dropped on
//! the way into the pass). Either way the swap looks right until something
//! moves, so every assertion here is "the pass holds this clip, whole, alone".
//!
//! Placement is the other half: frame `0` of the render is the tick the clip
//! started on, which is the mapping the restore inverts, so the pass must put
//! the clip at `0` and keep its span and rate.
//!
//! What they do not catch: whether the encoder writes those frames. That is the
//! export path's coverage and it needs a GPU.

use aria_models::{
    Clip, ClipId, Generator, MediaSource, Project, Rational, RationalTime, TimeRange, Track,
    TrackKind,
};

use super::bake_project;

pub(super) const R24: Rational = Rational::FPS_24;

pub(super) fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, R24)
}

pub(super) fn tr(start: i64, duration: i64) -> TimeRange {
    TimeRange::at_rate(start, duration, R24)
}

pub(super) fn source(name: &str) -> MediaSource {
    MediaSource::new(name, 1920, 1080, R24, 600, true)
}

/// A project with two abutting clips on a video lane, and a third above them.
/// The target carries an effect, because a media clip with nothing on it is not
/// worth rendering and the model refuses one.
pub(super) fn fixture() -> (Project, ClipId, ClipId) {
    let mut project = Project::new("bake", R24);
    let media = project.add_media(source("/media/a.mp4"));
    let lower = project.add_track(TrackKind::Video, "V1");
    let upper = project.add_track(TrackKind::Video, "V2");
    let target = project
        .add_clip(lower, media, tr(0, 100), rt(48))
        .expect("target");
    let neighbour = project
        .add_clip(lower, media, tr(100, 60), rt(148))
        .expect("neighbour");
    project
        .add_clip(upper, media, tr(0, 40), rt(60))
        .expect("above");
    project.add_effect(target, "gaussian_blur").expect("effect");
    (project, target, neighbour)
}

/// The clips of the pass's open timeline, start-ordered.
pub(super) fn survivors(project: &Project) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = project
        .timeline()
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .map(|clip| (clip.timeline.start.value, clip.timeline.duration.value))
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn the_pass_holds_the_clip_alone_at_tick_zero() {
    let (project, target, _) = fixture();
    let pass = bake_project(&project, target).expect("a bake pass").project;

    assert_eq!(survivors(&pass), vec![(0, 100)]);
    assert_eq!(pass.timeline().frame_rate, R24);
    assert_eq!(pass.timeline().canvas(), project.timeline().canvas());
    // The source window is untouched: the render decodes the same frames the
    // clip decoded, which is what makes the file frame-for-frame comparable.
    let clip = pass.clip(target).expect("the clip");
    assert_eq!(clip.source_range(), Some(tr(0, 100)));
}

#[test]
fn the_pass_keeps_the_look_the_render_is_supposed_to_fold_in() {
    let (mut project, target, _) = fixture();
    project
        .set_clip_speed(target, Rational::new(2, 1), false)
        .expect("speed");

    let pass = bake_project(&project, target).expect("a bake pass").project;
    let clip = pass.clip(target).expect("the clip");
    assert_eq!(clip.effects.len(), 1, "the pass dropped the clip's effects");
    assert!(
        clip.has_speed_curve() || clip.speed != Rational::new(1, 1),
        "the pass dropped the clip's retiming"
    );
    // A retimed clip's span is its retimed span, so the file still covers the
    // programme the clip occupies.
    assert_eq!(survivors(&pass), vec![(0, clip.timeline.duration.value)]);
}

#[test]
fn a_lane_bar_above_the_clip_is_not_folded_in() {
    let (mut project, target, _) = fixture();
    let lane = project.add_track(TrackKind::Adjustment, "A1");
    project
        .add_generated(lane, Generator::Adjustment, tr(0, 300))
        .expect("bar");

    let pass = bake_project(&project, target).expect("a bake pass").project;
    assert_eq!(
        survivors(&pass),
        vec![(0, 100)],
        "a lane bar's grade belongs to the programme, not to one clip"
    );
}

#[test]
fn a_half_transition_does_not_survive_its_junction() {
    let (mut project, target, _) = fixture();
    project
        .add_transition(target, "crossfade")
        .expect("a transition");
    assert!(project.has_transitions(), "the fixture needs a transition");

    let pass = bake_project(&project, target).expect("a bake pass").project;
    assert!(
        !pass.has_transitions(),
        "the pass kept a transition with nothing on the other side of it"
    );
}

#[test]
fn a_nest_still_shows_the_timeline_it_shows() {
    let mut project = Project::new("bake", R24);
    let media = project.add_media(source("/media/a.mp4"));
    let inner = project.create_sequence("inner", R24);
    let inner_track = {
        let sequence = project.sequence_mut(inner).expect("the nest");
        sequence
            .timeline
            .add_track(Track::new(TrackKind::Video, "V1"))
    };
    project
        .sequence_mut(inner)
        .expect("the nest")
        .timeline
        .add_clip(inner_track, Clip::from_media(media, tr(0, 80), tr(0, 80)))
        .expect("inner clip");
    let track = project.add_track(TrackKind::Video, "V1");
    let nest = project
        .add_sequence_clip(track, inner, tr(0, 80), rt(24))
        .expect("a nest clip");

    let pass = bake_project(&project, nest).expect("a bake pass").project;
    assert_eq!(
        pass.sequence(inner)
            .expect("the nest survived")
            .timeline
            .tracks_ordered()
            .flat_map(|track| track.clips())
            .count(),
        1,
        "the pass emptied the timeline the nest clip shows"
    );
    assert_eq!(survivors(&pass), vec![(0, 80)]);
}

#[test]
fn a_clip_inside_a_nest_bakes_from_that_nests_timeline() {
    let mut project = Project::new("bake", R24);
    let media = project.add_media(source("/media/a.mp4"));
    let inner = project.create_sequence("inner", R24);
    let inner_track = {
        let sequence = project.sequence_mut(inner).expect("the nest");
        sequence
            .timeline
            .add_track(Track::new(TrackKind::Video, "V1"))
    };
    let deep = {
        let sequence = project.sequence_mut(inner).expect("the nest");
        let mut clip = Clip::from_media(media, tr(0, 50), tr(30, 50));
        // Something on the clip, so it is worth rendering.
        clip.flip_h = true;
        let id = clip.id;
        sequence.timeline.add_clip(inner_track, clip).expect("deep");
        id
    };
    let track = project.add_track(TrackKind::Video, "V1");
    project
        .add_sequence_clip(track, inner, tr(0, 80), rt(0))
        .expect("a nest clip");

    let pass = bake_project(&project, deep).expect("a bake pass").project;
    assert_eq!(
        pass.active_sequence(),
        Some(inner),
        "the pass has to open the timeline the clip lives on"
    );
    assert_eq!(survivors(&pass), vec![(0, 50)]);
}

#[test]
fn every_refusal_names_what_is_wrong() {
    let (mut project, target, _) = fixture();

    let audio_media = project.add_media(source("/media/song.wav"));

    let lane = project.add_track(TrackKind::Adjustment, "A1");
    let bar = project
        .add_generated(lane, Generator::Adjustment, tr(0, 60))
        .expect("bar");
    let refusal = bake_project(&project, bar).expect_err("a lane bar");
    assert!(
        refusal.to_string().contains("lane bar"),
        "the refusal must name the reason: {refusal}"
    );

    let rendered = project.add_media(source("/media/rendered.mp4"));
    project.bake_clip(target, rendered).expect("a bake");
    let refusal = bake_project(&project, target).expect_err("already rendered");
    assert!(
        refusal.to_string().contains("already rendered"),
        "the refusal must name the reason: {refusal}"
    );

    let bare_track = project.add_track(TrackKind::Video, "V3");
    let bare = project
        .add_clip(bare_track, audio_media, tr(0, 40), rt(0))
        .expect("a clip with nothing on it");
    let refusal = bake_project(&project, bare).expect_err("nothing on it");
    assert!(
        refusal.to_string().contains("only re-encode"),
        "the refusal must name the reason: {refusal}"
    );

    let unknown = ClipId::from_raw(9999);
    bake_project(&project, unknown).expect_err("no such clip");
}

/// A pass is built from a clone, so the project being edited is not
/// disturbed by asking what a bake would render.
#[test]
fn building_a_pass_leaves_the_project_alone() {
    let (project, target, neighbour) = fixture();
    let before = survivors(&project);
    let _ = bake_project(&project, target).expect("a bake pass").project;
    assert_eq!(survivors(&project), before);
    assert!(project.clip(neighbour).is_some());
}
