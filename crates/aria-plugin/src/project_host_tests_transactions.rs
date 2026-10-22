use aria_commands::EditOutcome;
use aria_engine::{ApplyOutcome, EngineError};

use super::super::TransactError;
use super::*;

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
