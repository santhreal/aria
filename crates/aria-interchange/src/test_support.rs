//! Builders the format and placement suites share, and an engine session to
//! place cuts in and read them back from.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_commands::ProjectCommand;
use aria_engine::{Engine, EngineConfig};
use aria_models::{MarkerColor, MediaId, Project, Rational, RationalTime, Track};
use aria_plugin::{ApplyOutcome, Command, Origin, PluginContext, ProjectHost};
use parking_lot::Mutex;

use crate::model::{Cut, Event, Lane, LaneKind, Mark, Source, file_stem};

pub(crate) const R24: Rational = Rational::FPS_24;
/// Normal speed.
pub(crate) const UNIT: Rational = Rational::new(1, 1);

/// `value` ticks at `rate`.
pub(crate) fn at(value: i64, rate: Rational) -> RationalTime {
    RationalTime::new(value, rate)
}

/// An event of `duration` frames at `start` reading `path` from `source`,
/// filed under the file's stem, at normal speed, without its sound and
/// with no dissolve.
pub(crate) fn event(
    name: &str,
    start: i64,
    duration: i64,
    path: &str,
    source: RationalTime,
) -> Event {
    let path = PathBuf::from(path);
    Event {
        name: name.to_owned(),
        start,
        duration,
        source: Source {
            reel: file_stem(&path, ""),
            path: Some(path),
            start: source,
        },
        speed: UNIT,
        reversed: false,
        dissolve: None,
        sound: false,
    }
}

pub(crate) fn lane(kind: LaneKind, name: &str, events: Vec<Event>) -> Lane {
    Lane {
        kind,
        name: name.to_owned(),
        events,
    }
}

pub(crate) fn mark(at: i64, name: &str, comment: &str, color: Option<MarkerColor>) -> Mark {
    Mark {
        at,
        name: name.to_owned(),
        comment: comment.to_owned(),
        color,
    }
}

/// A cut named `name` at `rate` with no frame size.
pub(crate) fn cut(name: &str, rate: Rational, lanes: Vec<Lane>, marks: Vec<Mark>) -> Cut {
    Cut {
        name: name.to_owned(),
        rate,
        size: None,
        lanes,
        marks,
    }
}

/// What `write` produced from `cut`, as text, with what it reported.
pub(crate) fn written(
    write: fn(&Cut, &mut dyn std::io::Write) -> std::io::Result<Vec<String>>,
    cut: &Cut,
) -> (String, Vec<String>) {
    let mut out = Vec::new();
    let skipped = write(cut, &mut out).expect("writes to memory");
    (String::from_utf8(out).expect("UTF-8 output"), skipped)
}

/// 3 s of 24 fps picture, no sound.
pub(crate) const PICTURE: &str = "fixture-640x360-24fps-3s.mp4";
/// 2 s of 24 fps picture with sound (the file name predates the cut).
pub(crate) const TALKING: &str = "fixture-640x360-24fps-3s-av.mp4";
/// 3 s of 30 fps picture.
pub(crate) const PICTURE_30: &str = "fixture-640x360-30fps-3s.mp4";
/// 2 s of a sine tone.
pub(crate) const TONE: &str = "fixture-sine-48k-2s.wav";

/// An engine behind a project host, with a directory for the files a cut
/// reads.
pub(crate) struct Session {
    ctx: PluginContext,
    host: ProjectHost,
    pub(crate) dir: tempfile::TempDir,
}

pub(crate) fn session() -> Session {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx);
    let dir = tempfile::tempdir().expect("temp dir");
    Session { ctx, host, dir }
}

impl Session {
    /// The context the session's host posts its events on.
    pub(crate) fn context(&self) -> &PluginContext {
        &self.ctx
    }

    /// The host that edits the session's project.
    pub(crate) fn host(&self) -> &ProjectHost {
        &self.host
    }

    /// Place `cut` as one undo step, reading relative paths against the
    /// session directory, and return the report.
    pub(crate) fn place(&self, cut: &Cut) -> Vec<String> {
        self.host
            .transact_with(Origin::Editor, "Import", |tx| {
                crate::import::place(cut, self.dir.path(), tx)
            })
            .expect("places the cut")
            .0
    }

    /// Fixture `name` copied into the session directory as `file`.
    pub(crate) fn copy(&self, name: &str, file: &str) -> PathBuf {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name)
            .canonicalize()
            .expect("the fixture exists");
        let path = self.dir.path().join(file);
        std::fs::copy(&fixture, &path).expect("copy the fixture");
        path
    }

    pub(crate) fn apply(&self, command: Command) -> ApplyOutcome {
        let label = command.label();
        self.host
            .transact_with(Origin::Editor, "Edit", |tx| tx.apply(command))
            .unwrap_or_else(|err| panic!("{label}: {err}"))
            .0
    }

    pub(crate) fn import(&self, path: &Path) -> MediaId {
        let command = Command::Project(ProjectCommand::Import {
            path: path.to_path_buf(),
        });
        match self.apply(command) {
            ApplyOutcome::Imported { media } => media,
            other => panic!("importing {} produced {other:?}", path.display()),
        }
    }

    pub(crate) fn project(&self) -> Arc<Project> {
        self.host.snapshot()
    }
}

/// The one track named `name`.
pub(crate) fn track<'p>(project: &'p Project, name: &str) -> &'p Track {
    let mut named = project
        .timeline()
        .tracks_ordered()
        .filter(|track| track.name == name);
    let track = named
        .next()
        .unwrap_or_else(|| panic!("no track is named {name}"));
    assert!(named.next().is_none(), "two tracks are named {name}");
    track
}
