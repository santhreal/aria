//! Which camera a multicam clip shows, and where in that camera's own file the
//! frame is read from.
//!
//! # Why
//!
//! The resolver answered `Ok(None)` for every multicam clip, so a group with
//! angles, cuts and a sidebar to switch them reached no frame at all: the whole
//! feature ended at the model. These drive the resolved layer, which names the
//! media the decoder will be asked for and the tick it will be asked at, across a
//! cut and per angle.
//!
//! The convention these pin down is the sync offset's sign. An angle's offset is
//! where that camera's first frame sits on the group's common clock, so a camera
//! that started rolling late has a positive offset and is read earlier in its own
//! file than the group's tick. Nothing consumed the field before this, so it is
//! stated here and in the resolver, not inferred from a caller.

use super::tests::*;
use aria_models::{MulticamAngle, MulticamCut, Project};

/// A project with three cameras of distinct shapes, so the angle a cut makes
/// live is visible in the layer's own dimensions as well as in its media id.
struct Rig {
    project: Project,
    cameras: Vec<aria_models::MediaId>,
}

/// `offsets` is one sync offset per camera, in angle order.
fn rig(offsets: [i64; 3]) -> Rig {
    let mut project = Project::new("multicam", FPS_24);
    let cameras: Vec<aria_models::MediaId> = [(1920, 1080), (1080, 1920), (720, 720)]
        .into_iter()
        .map(|(w, h)| project.add_media(video(w, h)))
        .collect();
    let angles = cameras
        .iter()
        .zip(offsets)
        .map(|(media, offset)| MulticamAngle::new(*media, offset))
        .collect();
    let group = project
        .create_multicam_group("Concert", angles)
        .expect("three cameras is a group");
    let track = project.add_track(TrackKind::Video, "V1");
    // Cuts at 24 and 48: three windows, one per camera, so every angle is live
    // somewhere and none of them is live everywhere.
    for cut in [MulticamCut::new(24, 1), MulticamCut::new(48, 2)] {
        project
            .switch_multicam_angle(group, cut.at, cut.angle)
            .expect("a cut lands");
    }
    project
        .add_multicam_clip(track, group, tr(0, 72), rt(0))
        .expect("place the group on the lane");
    Rig { project, cameras }
}

/// The media and source tick the layer at `t` reads.
fn read(rig: &Rig, t: i64) -> (aria_models::MediaId, i64) {
    let scene = resolve(&rig.project, rt(t)).expect("the scene resolves");
    assert_eq!(
        scene.layers.len(),
        1,
        "a multicam clip is one layer, and at tick {t} it resolved to \
         {} of them",
        scene.layers.len()
    );
    match &scene.layers[0].source {
        LayerSource::Media { media, source_time } => (*media, source_time.value),
        other => panic!("a multicam angle is decoded media, and tick {t} read {other:?}"),
    }
}

#[test]
fn the_camera_a_cut_makes_live_is_the_one_decoded() {
    // The defect: every tick resolved to nothing, so the cut list was a list of
    // switches between pictures that were never asked for.
    let rig = rig([0, 0, 0]);
    for (tick, angle) in [(0, 0), (23, 0), (24, 1), (47, 1), (48, 2), (71, 2)] {
        let (media, _) = read(&rig, tick);
        assert_eq!(
            media, rig.cameras[angle],
            "tick {tick} decoded the wrong camera"
        );
    }
}

#[test]
fn each_camera_fits_the_canvas_at_its_own_shape() {
    // A group of mixed formats is the ordinary case: one body camera, one phone
    // held upright, one action cam. A resolver that fitted every angle at the
    // first angle's dimensions would pass the test above and stretch two thirds
    // of the cut. The assertion is on shape rather than pixels, because the
    // canvas is itself chosen from the media: what has to survive the fit is the
    // shape the camera was shot at.
    let rig = rig([0, 0, 0]);
    for (tick, w, h) in [
        (0, 1920.0, 1080.0),
        (24, 1080.0, 1920.0),
        (48, 720.0, 720.0),
    ] {
        let scene = resolve(&rig.project, rt(tick)).expect("the scene resolves");
        let SizeSpec::Fixed([lw, lh]) = scene.layers[0].size else {
            panic!(
                "a picture layer has a fixed size, got {:?}",
                scene.layers[0].size
            )
        };
        approx(lw / lh, w / h);
    }
}

#[test]
fn a_camera_that_started_late_is_read_earlier_in_its_own_file() {
    // The offset is where this camera's first frame sits on the group's clock,
    // so the read runs the other way. A sign error here is invisible in a group
    // whose cameras are all synchronised, which is exactly the group a test
    // would reach for first.
    let rig = rig([0, 12, -6]);
    let (_, at_group_start) = read(&rig, 0);
    assert_eq!(
        at_group_start, 0,
        "an angle at offset zero reads its own tick"
    );

    let (media, late) = read(&rig, 30);
    assert_eq!(
        media, rig.cameras[1],
        "tick 30 is the second camera's window"
    );
    assert_eq!(
        late, 18,
        "a camera 12 ticks late reads 12 ticks earlier than the group's tick"
    );

    let (media, early) = read(&rig, 50);
    assert_eq!(
        media, rig.cameras[2],
        "tick 50 is the third camera's window"
    );
    assert_eq!(
        early, 56,
        "a camera 6 ticks early reads 6 ticks later than the group's tick"
    );
}

#[test]
fn a_read_before_a_camera_started_stalls_on_its_first_frame() {
    // A decoder cannot answer for a negative tick. The alternative to clamping
    // is resolving nothing, which would draw a hole in the middle of a cut
    // rather than a held frame.
    let rig = rig([0, 40, 0]);
    let (media, at) = read(&rig, 30);
    assert_eq!(media, rig.cameras[1]);
    assert_eq!(
        at, 0,
        "a read before the camera's first frame has to stall on it"
    );
}

#[test]
fn the_cut_list_is_read_in_the_groups_time_and_not_the_timelines() {
    // The clip is a window onto the group: cuts are recorded against the group's
    // own clock, so moving the clip along the lane must not move which camera is
    // live under it.
    let mut project = Project::new("multicam", FPS_24);
    let cameras: Vec<aria_models::MediaId> = (0..2)
        .map(|_| project.add_media(video(1920, 1080)))
        .collect();
    let angles = cameras
        .iter()
        .map(|media| MulticamAngle::new(*media, 0))
        .collect();
    let group = project
        .create_multicam_group("Interview", angles)
        .expect("two cameras is a group");
    project
        .switch_multicam_angle(group, 24, 1)
        .expect("a cut lands");
    let track = project.add_track(TrackKind::Video, "V1");
    // The clip starts at timeline tick 100 and opens on group tick 30, which is
    // past the cut: the second camera is live from the clip's first frame.
    project
        .add_multicam_clip(track, group, tr(30, 40), rt(100))
        .expect("place the group late on the lane");
    let scene = resolve(&project, rt(100)).expect("the scene resolves");
    match &scene.layers[0].source {
        LayerSource::Media { media, source_time } => {
            assert_eq!(
                *media, cameras[1],
                "the clip's own start reset the cut list"
            );
            assert_eq!(source_time.value, 30, "the read follows the source window");
        }
        other => panic!("a multicam angle is decoded media, got {other:?}"),
    }
}
