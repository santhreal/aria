//! WHY: a project file edited by hand, by a script, or cut short reaches the
//! editor through one function, [`project_to_ui`], once the loader accepts
//! it. Every value of a saved project is replaced in turn by each hostile
//! value and every key and element is removed in turn
//! ([`aria_commands::for_each_damaged_file`]); each file the loader accepts
//! must project within the command budget, without a panic, and draw every
//! clip and lane of its open timeline. Values are drawn from the saved file at
//! run time, so a field a new version adds is in the census.
//!
//! Does not catch a wrong value the projection draws without panicking, or a
//! fault in what the editor does with the store afterwards.

use std::time::{Duration, Instant};

use aria_commands::{Command, CommandRng, EditCommand, for_each_damaged_file, random_edit_command};
use aria_engine::{Engine, EngineConfig};
use aria_models::{Generator, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::project_to_ui;

const RATE: Rational = Rational::FPS_24;

/// The most one projection of a damaged file may take: the engine census
/// gives one command the same.
const BUDGET: Duration = Duration::from_secs(2);

/// A project holding every lane kind the editor draws apart, media and
/// generated clips, a link, a transition and a marker, then 60 seeded edits.
fn fuzzed_project() -> String {
    let mut project = Project::new("damaged", RATE);
    let picture = project.add_media(MediaSource::new(
        "/synthetic/clip.mp4",
        1920,
        1080,
        RATE,
        240,
        true,
    ));
    let sound = project.add_media(MediaSource::new(
        "/synthetic/sound.wav",
        0,
        0,
        RATE,
        240,
        true,
    ));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let t1 = project.add_track(TrackKind::Text, "T1");
    for kind in [TrackKind::Adjustment, TrackKind::Filter, TrackKind::Submix] {
        project.add_track(kind, "lane");
    }
    let source = TimeRange::at_rate(0, 48, RATE);
    let first = project
        .add_clip(v1, picture, source, RationalTime::new(0, RATE))
        .expect("a clip at zero");
    project
        .add_clip(v1, picture, source, RationalTime::new(48, RATE))
        .expect("a clip after it");
    let under = project
        .add_clip(a1, sound, source, RationalTime::new(0, RATE))
        .expect("a sound clip");
    let mut engine =
        Engine::with_project(EngineConfig { undo_limit: 64 }, project).expect("an engine");
    for edit in [
        EditCommand::LinkClips {
            clips: vec![first, under],
        },
        EditCommand::AddTransition {
            clip: first,
            transition_id: "dip_to_black".into(),
        },
        EditCommand::AddGenerated {
            track: t1,
            generator: Generator::text("Title"),
            timeline: TimeRange::at_rate(0, 72, RATE),
        },
        EditCommand::AddMarker {
            at: RationalTime::new(24, RATE),
            name: "Beat".into(),
            color: None,
        },
    ] {
        engine.apply(Command::Edit(edit)).expect("a fixture edit");
    }
    let mut rng = CommandRng::new(7);
    for _ in 0..60 {
        let edit = random_edit_command(&mut rng, engine.project());
        let _ = engine.apply(Command::Edit(edit));
    }
    engine
        .project()
        .save_to_string()
        .expect("the fixture saves")
}

/// The rule projecting `project` breaks, or `None`.
fn projection_fault(project: &Project) -> Option<String> {
    let lanes = project.timeline().tracks_ordered().count();
    let clips: usize = project
        .timeline()
        .tracks_ordered()
        .map(|lane| lane.clips_ordered().count())
        .sum();
    let t0 = Instant::now();
    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        project_to_ui(project, RationalTime::new(0, RATE), &[])
    }));
    let took = t0.elapsed();
    let store = match drawn {
        Ok(store) => store,
        Err(payload) => {
            let text = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|t| (*t).to_owned()))
                .unwrap_or_default();
            return Some(format!("projecting panicked: {text}"));
        }
    };
    if took >= BUDGET {
        return Some(format!("projecting took {took:?}"));
    }
    if store.sequence.tracks.len() != lanes || store.sequence.clip_count() != clips {
        return Some(format!(
            "drew {} lanes and {} clips of {lanes} and {clips}",
            store.sequence.tracks.len(),
            store.sequence.clip_count()
        ));
    }
    None
}

#[test]
fn every_damaged_project_file_the_loader_opens_projects_every_lane_and_clip() {
    let text = fuzzed_project();
    let original = Project::load_from_string(&text).expect("the fixture opens");
    assert_eq!(projection_fault(&original), None, "the undamaged fixture");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("a saved project is JSON");
    let mut opened = 0usize;
    let mut faults = Vec::new();
    let damaged = for_each_damaged_file(&doc, |what, edited| {
        let Ok(project) = Project::load_from_string(&edited.to_string()) else {
            return;
        };
        opened += 1;
        if let Some(fault) = projection_fault(&project) {
            faults.push(format!("{what}: {fault}"));
        }
    });
    assert!(damaged > 200, "the fuzzed project holds {damaged} values");
    assert!(opened > 20, "only {opened} damaged files opened");
    assert!(
        faults.is_empty(),
        "{} damaged files break a rule; the first 40:\n{}",
        faults.len(),
        faults[..faults.len().min(40)].join("\n")
    );
}
