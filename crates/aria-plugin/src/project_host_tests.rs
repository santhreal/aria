//! The project host's contract: a group or a transaction lands as one undo
//! step or not at all, `CommandApplied` follows only what can no longer roll
//! back, a `CommandProposed` handler cannot start an edit, and an edit from
//! another thread is refused within a bounded wait instead of blocking.
//!
//! Not caught here: a shipped handler that locks the engine; the editor's
//! mounted-plugin suite proposes through every shipped handler for that.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use aria_commands::{
    Command, EditCommand, EditOutcome, ProjectCommand, TemplateMeta, every_edit_command,
    every_project_command,
};
use aria_engine::{ApplyOutcome, Engine, EngineConfig, EngineError};
use aria_models::{MediaId, Project, TrackId, TrackKind};
use parking_lot::Mutex;

use super::{CommandApplied, Origin, ProjectHost, TransactError, transactional};
use crate::Registration;
use crate::context::PluginContext;
use crate::editor_events::{
    ExportFinished, ExportStarted, MediaImported, MediaRelinked, ProjectClosed, ProjectOpened,
    ProjectSaved, RenderResult,
};
use crate::events::{Event, EventBus};
use crate::hooks::CommandProposed;

/// Long enough that only a blocked edit misses it.
const BOUND: Duration = Duration::from_secs(10);

const REFUSAL: &str = "lanes named refused are not allowed";

struct Fixture {
    host: Arc<ProjectHost>,
    ctx: PluginContext,
}

/// A host over an empty project, with one handler that refuses any lane
/// named "refused".
fn fixture() -> (Fixture, Registration) {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let policy = ctx.on_waterfall::<CommandProposed, _>(|proposed| {
        if lane_name(&proposed.command).as_deref() == Some("refused") {
            proposed.refuse(REFUSAL);
        }
    });
    (Fixture { host, ctx }, policy)
}

fn add(name: &str) -> Command {
    Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Video,
        name: name.to_string(),
        index: None,
        pinned: true,
    })
}

/// Fails in the engine: the lane does not exist.
fn missing_lane() -> Command {
    Command::Edit(EditCommand::SetTrackLocked {
        track: TrackId::from_raw(999),
        locked: true,
    })
}

fn lane_name(command: &Command) -> Option<String> {
    match command {
        Command::Edit(EditCommand::AddTrack { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

/// Lane names, sorted: stack order is not part of these contracts.
fn lane_names(project: &Project) -> Vec<String> {
    let mut names: Vec<String> = project
        .timeline()
        .tracks_ordered()
        .map(|track| track.name.clone())
        .collect();
    names.sort();
    names
}

fn lanes(host: &ProjectHost) -> Vec<String> {
    lane_names(&host.snapshot())
}

fn history(host: &ProjectHost) -> Vec<(String, bool)> {
    host.engine()
        .lock()
        .history_entries()
        .iter()
        .map(|entry| (entry.label.to_string(), entry.applied))
        .collect()
}

/// Nothing holds the project and no group is open, in the host or the engine.
fn assert_idle(host: &ProjectHost) {
    let idle = {
        let sequence = host.sequence.lock();
        sequence.owner.is_none()
            && sequence.holds == 0
            && sequence.groups == 0
            && sequence.announced.is_empty()
    };
    assert!(idle, "the host must release the project after every edit");
    assert!(
        !host.engine().lock().in_group(),
        "the engine must have no group left open"
    );
}

/// Every `CommandApplied` a handler received, as lane or label, origin, and
/// revision.
struct Announcements {
    entries: Arc<Mutex<Vec<(String, Origin, u64)>>>,
    bus: Arc<EventBus>,
}

impl Announcements {
    /// What was delivered, once everything posted so far has been.
    fn delivered(&self) -> Vec<(String, Origin, u64)> {
        assert!(
            self.bus.flush(BOUND),
            "delivery must finish within the bound"
        );
        self.entries.lock().clone()
    }
}

fn record_applied(ctx: &PluginContext) -> (Announcements, Registration) {
    let entries: Arc<Mutex<Vec<(String, Origin, u64)>>> = Arc::default();
    let sink = Arc::clone(&entries);
    let registration = ctx.on_event::<CommandApplied, _>(move |applied| {
        let name = lane_name(&applied.command).unwrap_or_else(|| applied.command.label().into());
        sink.lock()
            .push((name, applied.origin.clone(), applied.revision));
    });
    let log = Announcements {
        entries,
        bus: Arc::clone(ctx.event_bus()),
    };
    (log, registration)
}

fn announced(log: &Announcements) -> Vec<(String, Origin)> {
    log.delivered()
        .into_iter()
        .map(|(name, origin, _)| (name, origin))
        .collect()
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

/// The History label a transaction named "Make lanes" gets. A new origin does
/// not compile here until its label is decided.
fn step_label(origin: &Origin) -> &'static str {
    match origin {
        Origin::Editor | Origin::Assistant => "Make lanes",
        Origin::Plugin(_) => "acme.lanes: Make lanes",
    }
}

#[test]
fn a_transaction_lands_as_one_undo_step_named_for_its_origin() {
    for origin in [
        Origin::Editor,
        Origin::Assistant,
        Origin::plugin("acme.lanes"),
    ] {
        let (Fixture { host, ctx: _ctx }, _policy) = fixture();
        let receipt = host
            .transact(origin.clone(), "Make lanes", vec![add("a"), add("b")])
            .expect("both lanes apply");

        assert_eq!(lanes(&host), ["a", "b"], "{origin}");
        assert_eq!(
            history(&host),
            [(step_label(&origin).to_string(), true)],
            "{origin}: one step, named for the transaction"
        );
        assert_eq!(receipt.revision, host.revision(), "{origin}");
        let project = host.snapshot();
        let created: Vec<String> = receipt
            .outcomes
            .iter()
            .map(|outcome| match outcome {
                ApplyOutcome::Edited(EditOutcome::CreatedTrack(track)) => project
                    .timeline()
                    .track(*track)
                    .expect("the receipt names a lane that exists")
                    .name
                    .clone(),
                other => panic!("{origin}: expected a created lane, got {other:?}"),
            })
            .collect();
        assert_eq!(
            created,
            ["a", "b"],
            "{origin}: one outcome per command, in order"
        );
        drop(project);

        assert!(host.undo().expect("undo"), "{origin}");
        assert!(
            lanes(&host).is_empty(),
            "{origin}: one undo reverts both lanes"
        );
        assert!(host.redo().expect("redo"), "{origin}");
        assert_eq!(lanes(&host), ["a", "b"], "{origin}");
        assert_idle(&host);
    }
}

#[derive(Debug, Clone, Copy)]
enum Abort {
    Refused,
    Failed,
    Aborted,
    Panicked,
}

#[test]
fn a_transaction_that_does_not_finish_leaves_nothing_behind() {
    for abort in [
        Abort::Refused,
        Abort::Failed,
        Abort::Aborted,
        Abort::Panicked,
    ] {
        let (Fixture { host, ctx }, _policy) = fixture();
        host.apply(&Origin::Editor, add("before"))
            .expect("prior lane");
        let (log, _applied) = record_applied(&ctx);

        let result = host.transact_with(Origin::plugin("acme.atomic"), "Atomic", |tx| {
            tx.apply(add("kept"))?;
            match abort {
                Abort::Refused => tx.apply(add("refused")).map(|_| ()),
                Abort::Failed => tx.apply(missing_lane()).map(|_| ()),
                Abort::Aborted => Err(TransactError::Aborted("the body gave up".into())),
                Abort::Panicked => panic!("the body panicked"),
            }
        });

        let err = result.expect_err("the transaction must not commit");
        match (abort, &err) {
            (Abort::Refused, TransactError::Refused { label, reason }) => {
                assert_eq!((*label, reason.as_str()), ("Add track", REFUSAL));
            }
            (Abort::Failed, TransactError::Failed { label, .. }) => {
                assert_eq!(*label, "Lock track")
            }
            (Abort::Aborted, TransactError::Aborted(reason)) => {
                assert_eq!(reason, "the body gave up")
            }
            (Abort::Panicked, TransactError::Panicked(message)) => {
                assert_eq!(message, "the body panicked");
            }
            _ => panic!("{abort:?}: wrong error {err:?}"),
        }
        assert_eq!(
            lanes(&host),
            ["before"],
            "{abort:?}: no lane from the transaction remains"
        );
        assert_eq!(
            history(&host),
            [("Add track".to_string(), true)],
            "{abort:?}: history holds only the prior step"
        );
        assert!(
            announced(&log).is_empty(),
            "{abort:?}: nothing rolled back is announced"
        );
        assert_idle(&host);
        host.apply(&Origin::Editor, add("after"))
            .unwrap_or_else(|err| panic!("{abort:?}: the next edit must apply, got {err}"));
    }
}

#[test]
fn a_lone_refused_command_reports_the_handlers_reason() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    match host.apply(&Origin::plugin("acme.lone"), add("refused")) {
        Err(EngineError::Refused(reason)) => assert_eq!(reason, REFUSAL),
        other => panic!("expected the handler's refusal, got {other:?}"),
    }
    assert!(lanes(&host).is_empty());
    assert!(history(&host).is_empty());
    assert_idle(&host);
}

#[test]
fn a_failed_transaction_inside_a_group_rolls_back_only_its_own_commands() {
    let (Fixture { host, ctx }, _policy) = fixture();
    let (log, _applied) = record_applied(&ctx);
    let group = host.group(&Origin::Assistant).expect("open");
    host.apply(&Origin::Assistant, add("outer"))
        .expect("outer lane");

    let inner = host.transact(
        Origin::plugin("acme.inner"),
        "Inner",
        vec![add("inner"), missing_lane()],
    );
    assert!(
        matches!(
            inner,
            Err(TransactError::Failed {
                label: "Lock track",
                ..
            })
        ),
        "got {inner:?}"
    );
    assert_eq!(
        lanes(&host),
        ["outer"],
        "the savepoint keeps what came before it"
    );

    group.commit();
    assert_eq!(history(&host), [("Add track".to_string(), true)]);
    assert_eq!(announced(&log), [("outer".to_string(), Origin::Assistant)]);
    assert_idle(&host);
}

#[test]
fn applied_commands_are_announced_once_nothing_can_roll_them_back() {
    let (Fixture { host, ctx }, _policy) = fixture();
    let (log, _applied) = record_applied(&ctx);

    host.apply(&Origin::Editor, add("lone")).expect("lone lane");
    assert_eq!(
        announced(&log),
        [("lone".to_string(), Origin::Editor)],
        "a command outside a group is final as soon as it lands"
    );

    let group = host.group(&Origin::Assistant).expect("open");
    host.apply(&Origin::Assistant, add("a")).expect("a");
    let dropped = host.group(&Origin::Assistant).expect("savepoint");
    host.apply(&Origin::Assistant, add("dropped"))
        .expect("dropped");
    dropped.rollback();
    let kept = host.group(&Origin::Assistant).expect("savepoint");
    host.apply(&Origin::Assistant, add("b")).expect("b");
    kept.commit();
    assert_eq!(
        announced(&log).len(),
        1,
        "nothing in a group is announced before the outermost commit"
    );

    let committed = group.commit();
    assert_eq!(
        announced(&log),
        [
            ("lone".to_string(), Origin::Editor),
            ("a".to_string(), Origin::Assistant),
            ("b".to_string(), Origin::Assistant),
        ],
        "the outermost commit announces its commands in order, without the rolled-back one"
    );
    let revisions: Vec<u64> = log
        .delivered()
        .iter()
        .map(|(_, _, revision)| *revision)
        .collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "each announcement carries the revision right after its command: {revisions:?}"
    );
    assert!(revisions.iter().all(|revision| *revision <= committed));

    let rolled_back = host.group(&Origin::Editor).expect("open");
    host.apply(&Origin::Editor, add("gone")).expect("gone");
    rolled_back.rollback();
    assert_eq!(
        announced(&log).len(),
        3,
        "a rolled-back group announces nothing"
    );

    assert_eq!(lanes(&host), ["a", "b", "lone"]);
    assert_eq!(
        history(&host),
        [
            ("Add track".to_string(), true),
            ("Add track +1".to_string(), true),
        ]
    );
    assert_idle(&host);
}

#[test]
fn dropping_a_group_rolls_it_back() {
    let (Fixture { host, ctx }, _policy) = fixture();
    let (log, _applied) = record_applied(&ctx);
    {
        let origin = Origin::plugin("acme.drop");
        let _group = host.group(&origin).expect("open");
        host.apply(&origin, add("dropped")).expect("dropped");
    }
    assert!(lanes(&host).is_empty());
    assert!(history(&host).is_empty());
    assert!(announced(&log).is_empty());
    assert_idle(&host);
}

#[derive(Debug)]
struct Seen {
    snapshot_is_proposal: bool,
    revision: u64,
    transact: Option<TransactError>,
    panicked: Vec<(&'static str, bool)>,
}

#[test]
fn a_handler_reads_the_proposed_project_and_cannot_start_an_edit() {
    let (Fixture { host, ctx }, _policy) = fixture();
    host.apply(&Origin::Editor, add("first"))
        .expect("first lane");
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let _probe = {
        let host = Arc::clone(&host);
        let seen = Arc::clone(&seen);
        ctx.on_waterfall::<CommandProposed, _>(move |proposed| {
            let entry = Seen {
                snapshot_is_proposal: Arc::ptr_eq(&host.snapshot(), &proposed.project),
                revision: host.revision(),
                transact: host
                    .transact(Origin::plugin("acme.nested"), "Nested", vec![add("nested")])
                    .err(),
                panicked: vec![
                    (
                        "apply",
                        panics(|| drop(host.apply(&Origin::Editor, add("nested")))),
                    ),
                    ("group", panics(|| drop(host.group(&Origin::Editor)))),
                    ("undo", panics(|| drop(host.undo()))),
                    ("redo", panics(|| drop(host.redo()))),
                    ("history_jump", panics(|| drop(host.history_jump(0)))),
                    (
                        "replace_session",
                        panics(|| drop(host.replace_session(&Origin::Editor, |_| ()))),
                    ),
                ],
            };
            seen.lock().push(entry);
        })
    };
    let revision = host.revision();

    let (sent, received) = mpsc::channel();
    let editor = Arc::clone(&host);
    std::thread::spawn(move || {
        let _ = sent.send(editor.apply(&Origin::Editor, add("second")).is_ok());
    });
    let Ok(applied) = received.recv_timeout(BOUND) else {
        // The engine is locked by the stuck thread; dropping the host would
        // wait on it forever.
        std::mem::forget(host);
        panic!("a host call from a handler blocked on the engine it runs under");
    };
    assert!(applied, "the proposal itself still applies");

    let seen = seen.lock();
    let [entry] = seen.as_slice() else {
        panic!("one proposal, one handler run: {seen:?}");
    };
    assert!(
        entry.snapshot_is_proposal,
        "a handler's snapshot is the proposed project"
    );
    assert_eq!(
        entry.revision, revision,
        "a handler reads the revision being edited"
    );
    assert_eq!(entry.transact, Some(TransactError::Reentrant));
    for (call, panicked) in &entry.panicked {
        assert!(
            panicked,
            "{call} from a handler must refuse, not lock the engine"
        );
    }
    assert_eq!(
        lanes(&host),
        ["first", "second"],
        "nothing a handler started landed"
    );
}

#[test]
fn a_snapshot_does_not_change_when_the_project_does() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    let before = host.snapshot();
    host.apply(&Origin::Editor, add("after")).expect("lane");
    assert_eq!(before.timeline().track_count(), 0);
    assert_eq!(host.snapshot().timeline().track_count(), 1);
    assert!(!Arc::ptr_eq(&before, &host.snapshot()));
}

#[test]
fn another_thread_is_refused_by_name_within_the_contended_wait() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    let group = host.group(&Origin::Assistant).expect("open");
    host.apply(&Origin::Assistant, add("turn"))
        .expect("turn lane");

    let contender = Arc::clone(&host);
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let applied = contender.apply(&Origin::Editor, add("late"));
        let waited = started.elapsed();
        let transacted = contender.transact(Origin::plugin("acme.late"), "Late", vec![add("late")]);
        let grouped = contender
            .group(&Origin::Editor)
            .map(|group| group.rollback());
        let undone = contender.undo();
        let replaced = contender.replace_session(&Origin::Editor, Engine::new_session);
        let _ = sent.send((
            applied,
            waited,
            transacted,
            grouped,
            undone,
            replaced,
            contender.holder(),
        ));
    });
    let (applied, waited, transacted, grouped, undone, replaced, holder) = received
        .recv_timeout(BOUND)
        .expect("a contended edit must be refused, not block");

    let busy = "the assistant is editing the project; try again when it finishes";
    for (call, result) in [
        ("apply", applied.map(|_| ())),
        ("group", grouped),
        ("undo", undone.map(|_| ())),
        ("replace_session", replaced),
    ] {
        match result {
            Err(EngineError::Refused(reason)) => assert_eq!(reason, busy, "{call}"),
            other => panic!("{call}: expected a busy refusal, got {other:?}"),
        }
    }
    assert_eq!(
        transacted,
        Err(TransactError::Busy {
            holder: Origin::Assistant
        })
    );
    assert!(
        waited >= ProjectHost::CONTENDED_WAIT && waited < Duration::from_secs(2),
        "refused after waiting {waited:?}"
    );
    assert_eq!(
        holder,
        Some(Origin::Assistant),
        "another thread sees the holder"
    );
    assert_eq!(host.holder(), None, "the holder is not busy to itself");

    group.commit();
    let contender = Arc::clone(&host);
    let applied = std::thread::spawn(move || contender.apply(&Origin::Editor, add("late")).is_ok())
        .join()
        .expect("join");
    assert!(applied, "the project is free once the group commits");
    assert_eq!(lanes(&host), ["late", "turn"]);
    assert_idle(&host);
}

#[test]
fn a_contended_edit_proceeds_when_the_holder_finishes_within_the_wait() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    let group = host.group(&Origin::Assistant).expect("open");
    let contender = Arc::clone(&host);
    let (ready, started) = mpsc::channel();
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = ready.send(());
        let applied = contender.apply(&Origin::Editor, add("waited"));
        let _ = sent.send(applied.map(|_| ()).map_err(|err| err.to_string()));
    });
    started.recv_timeout(BOUND).expect("contender started");
    std::thread::sleep(Duration::from_millis(5));
    group.commit();
    let applied = received
        .recv_timeout(BOUND)
        .expect("the waiting edit must finish");
    assert_eq!(applied, Ok(()));
    assert_eq!(lanes(&host), ["waited"]);
    assert_idle(&host);
}

#[test]
fn every_edit_command_can_be_part_of_an_undo_step() {
    for edit in every_edit_command() {
        let command = Command::Edit(edit);
        assert_eq!(transactional(&command), Ok(()), "{}", command.label());
    }
}

#[test]
fn a_session_replacing_command_is_refused_inside_a_group_or_transaction() {
    let mut refused = Vec::new();
    for project in every_project_command() {
        let command = Command::Project(project);
        let Err(refusal) = transactional(&command) else {
            continue;
        };
        let label = command.label();
        refused.push(label);
        assert_eq!(refusal, TransactError::NotUndoable { label });

        let (Fixture { host, ctx: _ctx }, _policy) = fixture();
        let group = host.group(&Origin::Editor).expect("open");
        host.apply(&Origin::Editor, add("kept")).expect("kept");
        match host.apply(&Origin::Editor, command.clone()) {
            Err(EngineError::Refused(reason)) => assert_eq!(reason, refusal.to_string(), "{label}"),
            other => panic!("{label}: expected a refusal inside a group, got {other:?}"),
        }
        let transacted = host.transact(Origin::plugin("acme.session"), "Session", vec![command]);
        assert_eq!(transacted, Err(refusal), "{label}");
        group.commit();
        assert_eq!(lanes(&host), ["kept"], "{label}: the open group survived");
        assert_eq!(history(&host), [("Add track".to_string(), true)], "{label}");
        assert_idle(&host);
    }
    // Open, Load, and ApplyTemplate clear history, which would take the open
    // group with it.
    for command in every_project_command() {
        if matches!(
            command,
            ProjectCommand::Open { .. }
                | ProjectCommand::Load { .. }
                | ProjectCommand::ApplyTemplate { .. }
        ) {
            let label = Command::Project(command).label();
            assert!(
                refused.contains(&label),
                "{label} must be refused: {refused:?}"
            );
        }
    }
}

#[test]
fn history_does_not_move_while_a_group_is_open() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    // A committed step, so a stray undo would have something to take.
    host.apply(&Origin::Editor, add("before"))
        .expect("prior step");
    let group = host.group(&Origin::Editor).expect("open");
    host.apply(&Origin::Editor, add("grouped")).expect("lane");
    assert!(!host.undo().expect("undo"));
    assert!(!host.redo().expect("redo"));
    assert!(!host.history_jump(0).expect("jump"));
    assert_eq!(lanes(&host), ["before", "grouped"]);

    group.commit();
    assert!(host.undo().expect("undo"));
    assert_eq!(lanes(&host), ["before"]);
    assert!(host.redo().expect("redo"));
    assert_eq!(lanes(&host), ["before", "grouped"]);
    assert!(host.history_jump(0).expect("jump"));
    assert!(lanes(&host).is_empty());
    assert_idle(&host);
}

#[test]
fn the_session_cannot_be_replaced_while_a_group_is_open() {
    let (Fixture { host, ctx: _ctx }, _policy) = fixture();
    let group = host.group(&Origin::Editor).expect("open");
    host.apply(&Origin::Editor, add("grouped")).expect("lane");
    match host.replace_session(&Origin::Editor, Engine::new_session) {
        Err(EngineError::Refused(reason)) => assert_eq!(
            reason,
            "an edit group is still open; close it before replacing the session"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(lanes(&host), ["grouped"]);

    group.commit();
    host.replace_session(&Origin::Editor, Engine::new_session)
        .expect("no group open");
    assert_eq!(lanes(&host), ["Main"]);
    assert!(history(&host).is_empty());
    assert_idle(&host);
}

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
