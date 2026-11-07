//! Every way a project read from a file becomes the session corrects the size a
//! save from before the probe applied the container's display rotation stored.
//!
//! WHY: such a save states a phone video's sideways size, and once the decoder
//! draws the picture upright it draws it squashed into that frame. The repair
//! itself is `Project::orient_media`'s test; this one closes "a load path that
//! does not run it" for each command that replaces the session from a file:
//! Open, Load, and ApplyTemplate, and for RelinkMedia, which re-probes an entry
//! whose file moved. The stale entry is a landscape file stored transposed,
//! which is what a portrait phone video's entry looks like to the repair.
//! Skipped where no encoder can write the file. Not caught: a load path outside
//! the engine; the command-line loader has its own test, and the Python loader
//! has none.

mod common;

use std::path::{Path, PathBuf};

use aria_commands::{Command, ProjectCommand};
use aria_models::{
    CanvasSettings, Generator, MediaSource, Project, Rational, Template, TemplateMeta, TrackKind,
};
use common::{export_to, load_project, open_project, temp_engine, tr};

/// The size of the file [`landscape_mp4`] writes.
const SIZE: (u32, u32) = (64, 32);

/// A 6-frame `SIZE` video, or `None` where this platform cannot encode one.
fn landscape_mp4(dir: &Path) -> Option<PathBuf> {
    if !aria_encoder::capability().available {
        return None;
    }
    let mut source = Project::new("landscape", Rational::FPS_24);
    source.timeline_mut().set_canvas(CanvasSettings {
        resolution: Some([SIZE.0, SIZE.1]),
        ..CanvasSettings::default()
    });
    let generator = Generator::SolidColor {
        rgba: [10, 120, 200, 255],
    };
    let kind = TrackKind::for_generator(&generator).expect("a solid has a lane kind");
    let track = source.add_track(kind, "Solid");
    source
        .add_generated(track, generator, tr(0, 6))
        .expect("place the solid");
    let project = dir.join("landscape.aria");
    source
        .save_to_file(&project)
        .expect("save the source project");

    let (_scratch, mut engine) = temp_engine();
    open_project(&mut engine, &project);
    let out = dir.join("landscape.mp4");
    export_to(&mut engine, &out);
    Some(out)
}

#[test]
fn every_command_that_reads_a_project_corrects_a_sideways_size() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(file) = landscape_mp4(dir.path()) else {
        return;
    };
    let mut stale = Project::new("stale", Rational::FPS_24);
    let mut media = MediaSource::new(&file, SIZE.1, SIZE.0, Rational::FPS_24, 6, false);
    media.upright = false;
    stale.add_media(media);
    let project = dir.path().join("stale.aria");
    stale
        .save_to_file(&project)
        .expect("save the stale project");
    let template = dir.path().join("stale.ariat");
    Template::from_project(stale, TemplateMeta::new("stale"))
        .save_to_file(&template)
        .expect("save the stale template");

    let commands = [
        (
            "Open",
            ProjectCommand::Open {
                path: project.clone(),
            },
        ),
        ("Load", ProjectCommand::Load { path: project }),
        (
            "ApplyTemplate",
            ProjectCommand::ApplyTemplate {
                path: template,
                picks: Vec::new(),
            },
        ),
    ];
    for (what, command) in commands {
        let (_scratch, mut engine) = temp_engine();
        engine
            .apply(Command::Project(command))
            .unwrap_or_else(|err| panic!("{what}: {err}"));
        let entries: Vec<_> = engine
            .project()
            .media_iter()
            .map(|media| (media.width, media.height, media.upright))
            .collect();
        assert_eq!(entries, [(SIZE.0, SIZE.1, true)], "{what}");
    }
}

/// A stale entry whose file moved is left as it was by the load, and relinking
/// it adopts the probe's upright size and marks it upright, so the next load
/// does not probe it again.
#[test]
fn relinking_a_stale_entry_records_its_upright_size() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(file) = landscape_mp4(dir.path()) else {
        return;
    };
    let mut stale = Project::new("stale", Rational::FPS_24);
    let gone = dir.path().join("gone.mp4");
    let mut media = MediaSource::new(&gone, SIZE.1, SIZE.0, Rational::FPS_24, 6, false);
    media.upright = false;
    let media = stale.add_media(media);
    let project = dir.path().join("moved.aria");
    stale
        .save_to_file(&project)
        .expect("save the stale project");

    let (_scratch, mut engine) = temp_engine();
    load_project(&mut engine, &project);
    let entry = |engine: &aria_engine::Engine| {
        let media = engine.project().media(media).expect("the entry");
        (media.width, media.height, media.upright)
    };
    assert_eq!(entry(&engine), (SIZE.1, SIZE.0, false), "a missing file");
    engine
        .apply(Command::Project(ProjectCommand::RelinkMedia {
            media,
            path: file,
        }))
        .expect("relink");
    assert_eq!(entry(&engine), (SIZE.0, SIZE.1, true));
}
