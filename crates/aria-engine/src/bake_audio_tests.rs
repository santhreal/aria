//! Baking a clip that has no picture.
//!
//! The class these close: a sound intermediate that is not the clip's own
//! sound. It happens two ways, and both are silent. The file holds more than the
//! clip, because the lane's fader, mute, solo, sends or bus route were folded in
//! and are then applied a second time on playback; or a lane kind is baked as
//! the wrong medium, which writes a file with nothing a decoder can find in it.
//!
//! What they do not catch: whether the samples in the file are the right ones.
//! That is the export path's coverage, and it needs the mixer and real media.

use aria_models::{Project, TrackKind};

use super::bake_tests::{R24, fixture, rt, source, survivors, tr};
use super::{BakeMedium, bake_project};

/// Which medium a lane bakes to is read off its own row, so this walks every
/// row there is rather than the two kinds someone remembered. A lane kind added
/// with neither flag set reaches the "neither picture nor sound" refusal and
/// fails here, which is the point: it has to say what it renders to.
#[test]
fn every_lane_kind_bakes_to_the_medium_its_row_describes() {
    let mut seen = 0;
    for spec in aria_models::track_kind_specs() {
        let medium = BakeMedium::for_lane(spec.kind);
        match (spec.is_bus, spec.is_visual, spec.is_audio) {
            (true, _, _) => {
                let refusal = medium.expect_err("a bus holds no clip");
                assert!(
                    refusal.to_string().contains(spec.label),
                    "{:?}'s refusal must name the lane: {refusal}",
                    spec.kind
                );
            }
            (false, true, false) => assert_eq!(
                medium.expect("a picture lane"),
                BakeMedium::Picture,
                "{:?} composites, so it bakes a picture",
                spec.kind
            ),
            (false, false, true) => assert_eq!(
                medium.expect("a sound lane"),
                BakeMedium::Sound,
                "{:?} is sound, so it bakes sound",
                spec.kind
            ),
            (false, visual, audio) => panic!(
                "{:?} says is_visual={visual} is_audio={audio}: its row does not say what a \
                 render of one of its clips would be",
                spec.kind
            ),
        }
        seen += 1;
    }
    assert_eq!(
        seen,
        aria_models::track_kind_specs().len(),
        "every row is answered for"
    );
}

/// A sound pass holds the clip's own sound and nothing else. The lane it sits
/// on is programme: its fader, its mute and where it points are applied again
/// when the intermediate plays back, so the pass takes them off.
#[test]
fn a_sound_pass_takes_every_lane_off_the_mix_desk() {
    let mut project = Project::new("bake", R24);
    let media = project.add_media(source("/media/song.wav"));
    let bus = project.add_track(TrackKind::Submix, "Bus");
    let lane = project.add_track(TrackKind::Audio, "A1");
    let other = project.add_track(TrackKind::Audio, "A2");
    let clip = project
        .add_clip(lane, media, tr(0, 100), rt(48))
        .expect("sound");
    let neighbour = project
        .add_clip(other, media, tr(0, 40), rt(0))
        .expect("another lane's sound");
    project
        .set_clip_denoise(clip, true)
        .expect("something to render");

    project
        .timeline_mut()
        .set_track_output(lane, aria_models::TrackOutput::Bus(bus))
        .expect("route the lane into the bus");
    project
        .timeline_mut()
        .add_send(lane, bus, 0.5, false)
        .expect("a send");
    let desk = project.timeline_mut().track_mut(lane).expect("the lane");
    desk.gain = 0.25;
    desk.muted = true;
    project.timeline_mut().track_mut(other).expect("A2").solo = true;

    let pass = bake_project(&project, clip).expect("a bake pass");
    assert_eq!(pass.medium, BakeMedium::Sound);
    assert_eq!(
        survivors(&pass.project),
        vec![(0, 100)],
        "the pass holds the clip alone at tick zero"
    );
    assert!(
        pass.project.clip(neighbour).is_none(),
        "another lane's sound is not this clip's"
    );
    for track in pass.project.timeline().tracks_ordered() {
        assert_eq!(track.gain, 1.0, "{} kept a fader", track.name);
        assert!(!track.muted, "{} kept its mute", track.name);
        assert!(!track.solo, "{} kept its solo", track.name);
        assert!(track.sends.is_empty(), "{} kept a send", track.name);
        assert_eq!(
            track.output,
            aria_models::TrackOutput::Master,
            "{} still points at a bus",
            track.name
        );
    }
}

/// The picture path is not touched by the sound one: a video clip still bakes a
/// picture, and its lane's mix desk keeps every setting,
/// because the picture file carries the sound that desk shaped.
#[test]
fn a_picture_pass_leaves_the_mix_desk_alone() {
    let (mut project, target, _) = fixture();
    let lane = project.timeline().track_of(target).expect("the lane");
    project
        .timeline_mut()
        .track_mut(lane)
        .expect("the lane")
        .gain = 0.25;

    let pass = bake_project(&project, target).expect("a bake pass");
    assert_eq!(pass.medium, BakeMedium::Picture);
    assert_eq!(
        pass.project.timeline().track(lane).expect("the lane").gain,
        0.25,
        "a picture export writes the mix it is given"
    );
}
