use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_commands::{Command, ProjectCommand, TemplateMeta, every_project_command};
use aria_engine::{ApplyOutcome, Engine, EngineError};
use aria_models::MediaId;
use parking_lot::Mutex;

use super::*;
use crate::Registration;
use crate::context::PluginContext;
use crate::editor_events::{
    ExportFinished, ExportStarted, MediaImported, MediaRelinked, ProjectClosed, ProjectOpened,
    ProjectSaved, RenderResult,
};
use crate::events::{Event, EventBus};

/// One host notification, as these tests compare it.
#[derive(Debug, Clone, PartialEq)]
enum Note {
    Applied(&'static str),
    Imported(MediaId, PathBuf),
    Relinked(MediaId, PathBuf),
    Saved(PathBuf),
    Closed(Option<PathBuf>),
    /// The session's file and its lanes, sorted.
    Opened(Option<PathBuf>, Vec<String>),
    ExportStarted(PathBuf),
    ExportFinished(PathBuf, RenderResult),
}

/// Record each `E` delivered on `ctx`'s bus into `seen` as `to_note` reads it.
fn note<E: Event>(
    ctx: &PluginContext,
    seen: &Arc<Mutex<Vec<Note>>>,
    to_note: fn(&E) -> Note,
) -> Registration {
    let seen = Arc::clone(seen);
    ctx.on_event::<E, _>(move |event| seen.lock().push(to_note(event)))
}

/// Every host notification a handler received, in delivery order.
struct Notes {
    seen: Arc<Mutex<Vec<Note>>>,
    bus: Arc<EventBus>,
    _registrations: Vec<Registration>,
}

impl Notes {
    fn new(ctx: &PluginContext) -> Self {
        let seen: Arc<Mutex<Vec<Note>>> = Arc::default();
        let registrations = vec![
            note::<CommandApplied>(ctx, &seen, |e| Note::Applied(e.command.label())),
            note::<MediaImported>(ctx, &seen, |e| Note::Imported(e.media, e.path.clone())),
            note::<MediaRelinked>(ctx, &seen, |e| Note::Relinked(e.media, e.path.clone())),
            note::<ProjectSaved>(ctx, &seen, |e| Note::Saved(e.path.clone())),
            note::<ProjectClosed>(ctx, &seen, |e| Note::Closed(e.path.clone())),
            note::<ProjectOpened>(ctx, &seen, |e| {
                Note::Opened(e.path.clone(), lane_names(&e.project))
            }),
            note::<ExportStarted>(ctx, &seen, |e| Note::ExportStarted(e.path.clone())),
            note::<ExportFinished>(ctx, &seen, |e| {
                Note::ExportFinished(e.path.clone(), e.result.clone())
            }),
        ];
        Self {
            seen,
            bus: Arc::clone(ctx.event_bus()),
            _registrations: registrations,
        }
    }

    /// Everything delivered since the last call.
    fn take(&self) -> Vec<Note> {
        assert!(
            self.bus.flush(BOUND),
            "delivery must finish within the bound"
        );
        std::mem::take(&mut *self.seen.lock())
    }
}

/// Files for one scenario, in a directory removed when this drops.
struct Files {
    /// Two canonical copies of a still the image probe reads on every
    /// platform.
    a: PathBuf,
    b: PathBuf,
    project: PathBuf,
    template: PathBuf,
    export: PathBuf,
    _dir: tempfile::TempDir,
}

impl Files {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let still = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
        let copy = |name: &str| {
            let path = dir.path().join(name);
            std::fs::copy(&still, &path).expect("copy the still");
            std::fs::canonicalize(&path).expect("canonical path")
        };
        let (a, b) = (copy("a.png"), copy("b.png"));
        let root = std::fs::canonicalize(dir.path()).expect("canonical dir");
        Self {
            a,
            b,
            project: root.join("cut.aria"),
            template: root.join("intro.ariat"),
            export: root.join("out.mp4"),
            _dir: dir,
        }
    }
}

fn run(host: &ProjectHost, command: ProjectCommand) -> Result<ApplyOutcome, EngineError> {
    host.apply(&Origin::Editor, Command::Project(command))
}

fn imported(host: &ProjectHost, path: &Path) -> MediaId {
    match run(
        host,
        ProjectCommand::Import {
            path: path.to_path_buf(),
        },
    ) {
        Ok(ApplyOutcome::Imported { media }) => media,
        other => panic!("importing {} gave {other:?}", path.display()),
    }
}

/// Save a one-lane project, start a new one, then reopen the file with
/// `reopen`: the new session closes and the saved one opens.
fn reopened(
    host: &ProjectHost,
    notes: &Notes,
    files: &Files,
    label: &'static str,
    reopen: fn(PathBuf) -> ProjectCommand,
) -> Vec<Note> {
    host.apply(&Origin::Editor, add("saved")).expect("lane");
    run(
        host,
        ProjectCommand::Save {
            path: files.project.clone(),
        },
    )
    .expect("save");
    host.replace_session(&Origin::Editor, Engine::new_session)
        .expect("new session");
    notes.take();
    run(host, reopen(files.project.clone())).expect(label);
    vec![
        Note::Applied(label),
        Note::Closed(None),
        Note::Opened(Some(files.project.clone()), vec!["saved".to_string()]),
    ]
}

/// Each project command announces `CommandApplied` and then the editor event
/// its outcome implies, with paths and ids read from the session after it.
/// Exhaustive over project commands, so a new one does not compile until what
/// it announces is decided here.
#[test]
fn every_project_command_announces_what_its_outcome_implies() {
    for sample in every_project_command() {
        let label = Command::Project(sample.clone()).label();
        let (Fixture { host, ctx }, _policy) = fixture();
        let files = Files::new();
        let notes = Notes::new(&ctx);
        let expected = match sample {
            ProjectCommand::Import { .. } => {
                let media = imported(&host, &files.a);
                vec![Note::Applied(label), Note::Imported(media, files.a.clone())]
            }
            ProjectCommand::RemoveMedia { .. } => {
                let media = imported(&host, &files.a);
                notes.take();
                run(&host, ProjectCommand::RemoveMedia { media }).expect(label);
                vec![Note::Applied(label)]
            }
            ProjectCommand::Save { .. } => {
                run(
                    &host,
                    ProjectCommand::Save {
                        path: files.project.clone(),
                    },
                )
                .expect(label);
                vec![Note::Applied(label), Note::Saved(files.project.clone())]
            }
            ProjectCommand::Open { .. } => reopened(&host, &notes, &files, label, |path| {
                ProjectCommand::Open { path }
            }),
            ProjectCommand::Load { .. } => reopened(&host, &notes, &files, label, |path| {
                ProjectCommand::Load { path }
            }),
            ProjectCommand::RelinkMedia { .. } => {
                let media = imported(&host, &files.a);
                notes.take();
                let path = files.b.clone();
                run(&host, ProjectCommand::RelinkMedia { media, path }).expect(label);
                vec![Note::Applied(label), Note::Relinked(media, files.b.clone())]
            }
            ProjectCommand::SetMediaLabel { label: color, .. } => {
                let media = imported(&host, &files.a);
                notes.take();
                run(
                    &host,
                    ProjectCommand::SetMediaLabel {
                        media,
                        label: color,
                    },
                )
                .expect(label);
                vec![Note::Applied(label)]
            }
            ProjectCommand::Export { format, .. } => {
                let path = files.export.clone();
                let result = run(
                    &host,
                    ProjectCommand::Export {
                        path: path.clone(),
                        format,
                    },
                );
                let finished = match &result {
                    Ok(_) => RenderResult::Written,
                    Err(err) => RenderResult::Failed(err.to_string()),
                };
                let mut expected = vec![
                    Note::ExportStarted(path.clone()),
                    Note::ExportFinished(path, finished),
                ];
                if result.is_ok() {
                    expected.push(Note::Applied(label));
                }
                expected
            }
            ProjectCommand::SaveTemplate { meta, .. } => {
                let path = files.template.clone();
                run(&host, ProjectCommand::SaveTemplate { path, meta }).expect(label);
                vec![Note::Applied(label)]
            }
            ProjectCommand::ApplyTemplate { .. } => {
                host.apply(&Origin::Editor, add("templated")).expect("lane");
                let template = files.template.clone();
                let meta = TemplateMeta::new("Intro");
                run(
                    &host,
                    ProjectCommand::SaveTemplate {
                        path: template.clone(),
                        meta,
                    },
                )
                .expect("template");
                run(
                    &host,
                    ProjectCommand::Save {
                        path: files.project.clone(),
                    },
                )
                .expect("save");
                notes.take();
                let picks = Vec::new();
                run(
                    &host,
                    ProjectCommand::ApplyTemplate {
                        path: template,
                        picks,
                    },
                )
                .expect(label);
                vec![
                    Note::Applied(label),
                    Note::Closed(Some(files.project.clone())),
                    Note::Opened(None, lanes(&host)),
                ]
            }
        };
        assert_eq!(notes.take(), expected, "{label}");
        assert_idle(&host);
    }
}

/// An import inside a group is announced when the outermost group commits and
/// never when it rolls back, whether or not anything reads `CommandApplied`.
#[test]
fn media_imported_in_a_group_is_announced_only_by_the_outermost_commit() {
    let (Fixture { host, ctx }, _policy) = fixture();
    let files = Files::new();
    let seen: Arc<Mutex<Vec<Note>>> = Arc::default();
    let _imported = note::<MediaImported>(&ctx, &seen, |e| Note::Imported(e.media, e.path.clone()));
    let delivered = || {
        assert!(
            ctx.events().flush(BOUND),
            "delivery must finish within the bound"
        );
        std::mem::take(&mut *seen.lock())
    };

    let dropped = host.group(&Origin::Editor).expect("open");
    imported(&host, &files.a);
    assert!(
        delivered().is_empty(),
        "nothing in an open group is announced"
    );
    dropped.rollback();
    assert!(
        delivered().is_empty(),
        "a rolled-back import is never announced"
    );

    let outer = host.group(&Origin::Editor).expect("open");
    let inner = host.group(&Origin::Editor).expect("savepoint");
    let media = imported(&host, &files.b);
    inner.commit();
    assert!(
        delivered().is_empty(),
        "a savepoint commit announces nothing"
    );
    outer.commit();
    assert_eq!(delivered(), [Note::Imported(media, files.b)]);
    assert_idle(&host);
}

/// Replacing the session outside a command announces the session it closed and
/// the one it opened, read after the replacement. A replacement that fails
/// announces neither.
#[test]
fn replacing_the_session_announces_the_closed_and_the_opened_project() {
    let (Fixture { host, ctx }, _policy) = fixture();
    let files = Files::new();
    host.apply(&Origin::Editor, add("old")).expect("lane");
    run(
        &host,
        ProjectCommand::Save {
            path: files.project.clone(),
        },
    )
    .expect("save");
    let notes = Notes::new(&ctx);

    host.replace_session(&Origin::Editor, Engine::new_session)
        .expect("replace");
    assert_eq!(
        notes.take(),
        [
            Note::Closed(Some(files.project.clone())),
            Note::Opened(None, vec!["Main".to_string()]),
        ]
    );

    let missing = files.project.with_file_name("missing.aria");
    run(&host, ProjectCommand::Open { path: missing }).expect_err("no such file");
    assert!(notes.take().is_empty(), "a failed open closes nothing");
    assert_eq!(lanes(&host), ["Main"]);
    assert_idle(&host);
}
