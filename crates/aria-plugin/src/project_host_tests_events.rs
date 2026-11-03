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

/// WHY: closes the defect class where plugin-initiated `ProjectHost::{open,
/// save, save_as}`, `host_wire`, or `AriaHostV1` session/transport/render calls
/// bypass dirty-project protection, undo-group refusal, or package permission
/// grants (`project` and `fs`).
#[test]
fn project_host_and_host_wire_enforce_session_and_permission_contracts() {
    use crate::editor_events::{EditorView, TransportRequest};
    use crate::host_wire::{self, SessionChange};
    use crate::package::Package;
    use crate::render_host::{FrameRequest, FrameSize, FrameSource, MAX_RENDER_FRAME_DIMENSION};
    use aria_core::{Rational, RationalTime};

    let (Fixture { host, ctx }, _policy) = fixture();
    std::mem::forget(ctx.register_arc(Arc::clone(&host)));
    let view = Arc::new(EditorView::new());
    std::mem::forget(ctx.register_arc(Arc::clone(&view)));
    let files = Files::new();
    let notes = Notes::new(&ctx);

    // Saving an unsaved project with no path fails with EngineError::Refused.
    let plugin_origin = Origin::plugin("com.example.session");
    assert!(matches!(
        host.save(&plugin_origin),
        Err(EngineError::Refused(_))
    ));

    // save_as writes the file, clears dirty state, posts ProjectSaved, and records SessionChange::Saved.
    host.apply(&plugin_origin, add("tracked"))
        .expect("add lane");
    assert!(host.is_dirty());
    let saved_path = host
        .save_as(&plugin_origin, &files.project)
        .expect("save_as");
    assert_eq!(saved_path, files.project);
    assert!(!host.is_dirty());
    assert_eq!(host.project_path(), Some(files.project.clone()));
    assert_eq!(
        notes.take(),
        [
            Note::Applied("Add track"),
            Note::Applied("Save project"),
            Note::Saved(files.project.clone()),
        ]
    );

    // A dirty project refuses `open` until saved.
    host.apply(&plugin_origin, add("unsaved"))
        .expect("dirty edit");
    notes.take();
    let err = host
        .open(&plugin_origin, &files.project)
        .expect_err("dirty open must be refused");
    assert!(err.to_string().contains("unsaved changes"), "got: {err}");
    assert!(notes.take().is_empty());

    // Saving via `save()` clears dirty state, after which `open()` succeeds and records SessionChange::Opened.
    host.save(&plugin_origin).expect("save existing");
    host.open(&plugin_origin, &files.project)
        .expect("open clean");
    assert_eq!(
        notes.take(),
        [
            Note::Applied("Save project"),
            Note::Saved(files.project.clone()),
            Note::Applied("Open project"),
            Note::Closed(Some(files.project.clone())),
            Note::Opened(
                Some(files.project.clone()),
                vec!["tracked".into(), "unsaved".into()],
            ),
        ]
    );
    assert_eq!(
        host_wire::take_session_changes(&host),
        [
            SessionChange::Saved {
                path: files.project.clone()
            },
            SessionChange::Saved {
                path: files.project.clone()
            },
            SessionChange::Opened {
                path: Some(files.project.clone())
            },
        ]
    );

    // Inside an open EditGroup, open/save/save_as are refused.
    let group = host.group(&plugin_origin).expect("group");
    assert!(host.save(&plugin_origin).is_err());
    assert!(host.save_as(&plugin_origin, &files.project).is_err());
    assert!(host.open(&plugin_origin, &files.project).is_err());
    group.rollback();

    // Bounds validation on host_wire::render_frame and check_project_fs_path.
    for bad_size in [
        FrameSize::Fit {
            max_width: 0,
            max_height: 180,
        },
        FrameSize::Fit {
            max_width: MAX_RENDER_FRAME_DIMENSION + 1,
            max_height: 180,
        },
        FrameSize::Exact {
            width: 320,
            height: 0,
        },
        FrameSize::Exact {
            width: 320,
            height: MAX_RENDER_FRAME_DIMENSION + 1,
        },
    ] {
        let req = FrameRequest {
            source: FrameSource::Open,
            size: bad_size,
            motion_blur: false,
        };
        assert!(
            host_wire::render_frame(&ctx, &host, RationalTime::new(0, Rational::FPS_24), &req)
                .is_err(),
            "bad size {bad_size:?} must fail"
        );
    }

    let pkg_dir = files.project.parent().expect("parent").join("pkg");
    std::fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    std::fs::write(
        pkg_dir.join("plugin.toml"),
        r#"[plugin]
id = "com.example.rw"
name = "RW"
version = "0.1.0"
api = 1
publisher = "Test"
description = "Test"
license = "MIT"

[permissions]
project = "read-write"
fs = ["project"]
"#,
    )
    .expect("write manifest");
    let pkg = Package::open(&pkg_dir).expect("open package");
    assert!(
        host_wire::check_project_fs_path(&pkg, &host, Path::new("/etc/passwd"), "read").is_err()
    );
    assert!(host_wire::check_project_fs_path(&pkg, &host, &files.project, "write").is_ok());

    let landed =
        host_wire::editor_seek(&ctx, &host, RationalTime::new(6, Rational::FPS_24)).expect("seek");
    assert_eq!(landed, RationalTime::new(6, Rational::FPS_24));
    host_wire::editor_play(&ctx, 2).expect("play");
    host_wire::editor_stop(&ctx).expect("stop");
    assert_eq!(
        view.take_transport_requests(),
        [
            TransportRequest::Seek {
                at: RationalTime::new(6, Rational::FPS_24)
            },
            TransportRequest::Play { speed: 2 },
            TransportRequest::Stop,
        ]
    );
}
