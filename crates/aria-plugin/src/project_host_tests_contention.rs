use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use aria_commands::{Command, ProjectCommand, every_edit_command, every_project_command};
use aria_engine::{Engine, EngineError};
use parking_lot::Mutex;

use super::super::{TransactError, transactional};
use super::*;
use crate::hooks::CommandProposed;

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
