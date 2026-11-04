//! `SlipClip` on a linked picture and sound whose takes run at different
//! rates.
//!
//! # Why
//!
//! A slip moves a clip's take inside it, and each clip linked to it moves its
//! own take with it. The engine slipped every member by the tick count the
//! command carried, whatever the rate of the member's take, so a picture at
//! 24 fps slipped half a second moved its sound, counted in milliseconds, by
//! twelve milliseconds. The class closed here is a linked slip that moves the
//! takes by different times, lets one take run past its end while the other
//! stops, or undoes in more than one step.
//!
//! Not caught here: the drag that sends the command, which the UI census
//! `drawn_census_timeline` drives, and the rounding of a share between two
//! ticks, which `aria-models` covers.

mod common;

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineConfig};
use aria_models::{ClipId, MediaSource, Project, Rational, TimeRange, TrackKind};
use common::{rt, tr};

/// A sound counted in milliseconds.
const MS: Rational = Rational::new(1_000, 1);

/// Frames 48 to 96 of a 300-frame shot, linked to 2 s to 4 s of a 10 s sound:
/// the shot has 2 s behind its window and 8.5 s after it, the sound 2 s behind
/// and 6 s after.
fn pair() -> (Engine, ClipId, ClipId) {
    let mut project = Project::new("linked-slip", Rational::FPS_24);
    let shot = project.add_media(MediaSource::new(
        "/srv/shot.mp4",
        1920,
        1080,
        Rational::FPS_24,
        300,
        false,
    ));
    let tone = project.add_media(MediaSource::new("/srv/tone.wav", 0, 0, MS, 10_000, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let picture = project
        .add_clip(v1, shot, tr(48, 48), rt(0))
        .expect("place the picture");
    let sound = project
        .add_clip(a1, tone, TimeRange::at_rate(2_000, 2_000, MS), rt(0))
        .expect("place the sound");
    project
        .link_clips(&[picture, sound])
        .expect("link the pair");
    let engine = Engine::with_project(EngineConfig { undo_limit: 32 }, project).expect("engine");
    (engine, picture, sound)
}

/// Where each take of the pair starts, in its own ticks.
fn starts(engine: &Engine, picture: ClipId, sound: ClipId) -> (i64, i64) {
    let start = |clip| {
        engine
            .project()
            .clip(clip)
            .and_then(|clip| clip.source_range())
            .expect("a placed take")
            .start
            .value
    };
    (start(picture), start(sound))
}

fn slip(engine: &mut Engine, clip: ClipId, by: i64) {
    engine
        .apply(Command::Edit(EditCommand::SlipClip { clip, by }))
        .expect("slip the pair");
}

#[test]
fn a_linked_slip_moves_each_take_by_the_same_time_and_undoes_in_one_step() {
    let (mut engine, picture, sound) = pair();

    // Twelve frames are half a second of either take.
    slip(&mut engine, picture, 12);
    assert_eq!(starts(&engine, picture, sound), (60, 2_500));
    assert!(engine.undo(), "the slip is in the history");
    assert_eq!(starts(&engine, picture, sound), (48, 2_000));
    assert!(!engine.undo(), "the slip was one step");

    // Sent from the sound, in its own ticks.
    slip(&mut engine, sound, -1_000);
    assert_eq!(starts(&engine, picture, sound), (24, 1_000));
}

#[test]
fn a_linked_slip_stops_both_takes_where_the_first_runs_out() {
    let (mut engine, picture, sound) = pair();

    // The sound runs out 6 s on, before the shot's 8.5 s.
    slip(&mut engine, picture, 1_000);
    assert_eq!(starts(&engine, picture, sound), (192, 8_000));

    // Both have 2 s behind them at the start, so back past the head both stop
    // at the first tick of the take.
    let (mut engine, picture, sound) = pair();
    slip(&mut engine, sound, -100_000);
    assert_eq!(starts(&engine, picture, sound), (0, 0));
}
