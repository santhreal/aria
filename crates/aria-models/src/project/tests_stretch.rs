//! The speed that makes a clip occupy a given length.
//!
//! # Why
//!
//! Closes the class where a rate stretch lands on a length other than the one
//! dragged to. A speed composed from the clip's current length inherits that
//! length's truncation, so once a clip has been retimed the stretch misses by
//! a tick or more, and a stretch held at the next clip's start overlaps it and
//! is refused. Each case sets the speed [`Clip::speed_for`] gives and reads
//! the length the model derives from it: flat, reversed, retimed from a length
//! that truncates, and ramped.
//!
//! Not covered: a source long enough that the ratio no longer fits `i32`
//! terms and is approximated.

use super::*;

/// The length `clip` takes at the speed [`Clip::speed_for`] gives for
/// `duration`.
fn stretched(project: &mut Project, clip: ClipId, duration: i64) -> i64 {
    let held = project.clip(clip).unwrap();
    let (speed, reversed) = (held.speed_for(duration, R24).unwrap(), held.reversed);
    project.set_clip_speed(clip, speed, reversed).unwrap();
    project.clip(clip).unwrap().timeline.duration.value
}

#[test]
fn a_flat_stretch_lands_on_the_length_asked_for() {
    for (source, first) in [(48, 48), (100, 37), (11, 3), (1000, 999), (7, 1)] {
        let (mut project, media, track) = project_with_media(2000);
        let clip = project
            .add_clip(track, media, tr(0, source), rt(0))
            .unwrap();
        assert_eq!(
            stretched(&mut project, clip, first),
            first,
            "source {source}"
        );
        // Retimed now, from a length that truncates for most sources.
        for duration in [1, 2, first + 1, 100, 4801] {
            assert_eq!(
                stretched(&mut project, clip, duration),
                duration,
                "source {source}, retimed to {first} first"
            );
        }
    }
}

#[test]
fn a_reversed_stretch_stays_reversed() {
    let (mut project, media, track) = project_with_media(2000);
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_speed(clip, Rational::new(3, 1), true)
        .unwrap();
    assert_eq!(stretched(&mut project, clip, 77), 77);
    assert!(project.clip(clip).unwrap().reversed);
}

#[test]
fn a_ramped_stretch_lands_on_the_length_asked_for() {
    let (mut project, media, track) = project_with_media(2000);
    let clip = project.add_clip(track, media, tr(0, 480), rt(0)).unwrap();
    let ramp = crate::clip::speed_preset("montage").unwrap();
    project.set_clip_speed_curve(clip, Some(ramp)).unwrap();
    for duration in [1, 24, 100, 481, 1000] {
        assert_eq!(stretched(&mut project, clip, duration), duration);
    }
}

#[test]
fn nothing_stretches_a_generated_clip_or_to_no_length() {
    let (project, solid, _, _) = project_with_abutting_pair();
    assert_eq!(
        project.clip(solid).unwrap().speed_for(24, R24),
        Err(ModelError::InvalidParam(
            "speed requires a media-backed clip".into()
        ))
    );
    let (mut project, media, track) = project_with_media(200);
    let clip = project.add_clip(track, media, tr(0, 48), rt(0)).unwrap();
    assert_eq!(
        project.clip(clip).unwrap().speed_for(0, R24),
        Err(ModelError::InvalidRange)
    );
    assert_eq!(stretched(&mut project, clip, 1), 1);
}
