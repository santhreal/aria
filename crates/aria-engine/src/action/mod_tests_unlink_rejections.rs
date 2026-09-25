//! `unlink_rejections` cases.

use super::*;

#[test]
fn unlink_rejections_leave_all_links_unchanged() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let a = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(0, 10)))
        .unwrap();
    let b = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(20, 10)))
        .unwrap();
    let unlinked = project
        .timeline_mut()
        .add_clip(track, Clip::generated(Generator::Adjustment, tr(40, 10)))
        .unwrap();
    let group = LinkId::next();
    for clip in [a, b] {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(group);
    }

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    assert!(matches!(
        link_clips::unlink(&mut ctx, &[]),
        Err(EngineError::Model(ModelError::InvalidRange))
    ));
    let missing = aria_models::ClipId::from_raw(999);
    assert!(matches!(
        link_clips::unlink(&mut ctx, &[a, missing]),
        Err(EngineError::Model(ModelError::UnknownClip(id))) if id == missing
    ));
    assert!(matches!(
        link_clips::unlink(&mut ctx, &[unlinked, unlinked]),
        Err(EngineError::Model(ModelError::InvalidRange))
    ));
    assert_eq!(ctx.project.clip(a).unwrap().link, Some(group));
    assert_eq!(ctx.project.clip(b).unwrap().link, Some(group));
    assert_eq!(ctx.project.clip(unlinked).unwrap().link, None);
}

#[test]
fn unlink_inverse_restores_groups_and_oscillates() {
    let mut project = setup();
    let track = project.add_track(TrackKind::Adjustment, "FX");
    let ids: Vec<_> = (0..4)
        .map(|index| {
            project
                .timeline_mut()
                .add_clip(
                    track,
                    Clip::generated(Generator::Adjustment, tr(index * 20, 10)),
                )
                .unwrap()
        })
        .collect();
    let first = LinkId::next();
    let second = LinkId::next();
    for (clip, link) in [
        (ids[0], first),
        (ids[1], first),
        (ids[2], second),
        (ids[3], second),
    ] {
        project.timeline_mut().clip_mut(clip).unwrap().link = Some(link);
    }

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let undo = link_clips::unlink(&mut ctx, &[ids[0], ids[2]]).unwrap();
    assert!(
        ids.iter()
            .all(|id| ctx.project.clip(*id).unwrap().link.is_none())
    );

    let redo = undo.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.clip(ids[0]).unwrap().link, Some(first));
    assert_eq!(ctx.project.clip(ids[1]).unwrap().link, Some(first));
    assert_eq!(ctx.project.clip(ids[2]).unwrap().link, Some(second));
    assert_eq!(ctx.project.clip(ids[3]).unwrap().link, Some(second));

    let _undo = redo.apply(&mut ctx).unwrap();
    assert!(
        ids.iter()
            .all(|id| ctx.project.clip(*id).unwrap().link.is_none())
    );
}

#[test]
fn set_project_name_inverse_oscillates() {
    let mut project = setup();
    assert_eq!(project.name, "test");

    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    let inv1 = set_project_name::set_project_name(&mut ctx, "renamed".to_string()).unwrap();
    assert_eq!(ctx.project.name, "renamed");

    let inv2 = inv1.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.name, "test", "undo restores the prior name");

    let _ = inv2.apply(&mut ctx).unwrap();
    assert_eq!(ctx.project.name, "renamed", "redo re-applies");
}

#[test]
fn set_track_flags_unknown_track_errors() {
    let mut project = setup();
    let mut project_path = None;
    let mut history = History::new(32);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);
    let missing = aria_models::TrackId::from_raw(999);
    assert!(
        set_track_flags::execute(
            &mut ctx,
            missing,
            set_track_flags::FlagPatch::enabled(false)
        )
        .is_err()
    );
}

#[test]
fn compound_rolls_back_when_an_inner_action_fails() {
    let mut project = setup();
    let mut project_path = None;
    let mut history = History::new(8);
    let mut ctx = test_ctx(&mut project, &mut project_path, &mut history);

    // Inverses run in reverse of the vec: AddTrackTest applies first (adds
    // a track), then FailAction fails. The already-applied AddTrackTest
    // must be rolled back, leaving the project untouched.
    let compound: Box<dyn EditAction> = Box::new(CompoundAction {
        actions: vec![Box::new(FailAction), Box::new(AddTrackTest)],
    });

    let before = ctx.project.timeline().track_count();
    assert!(
        compound.apply(&mut ctx).is_err(),
        "a failing inner action fails the compound"
    );
    assert_eq!(
        ctx.project.timeline().track_count(),
        before,
        "the partial mutation was rolled back"
    );
}

#[test]
fn group_commits_as_single_entry_named_for_its_first_command() {
    let mut history = History::new(8);
    history.begin_group();
    history.record_do("Move clip", Box::new(NoopAction));
    history.record_do("Trim clip", Box::new(NoopAction));
    assert!(!history.can_undo(), "pending group is not undoable yet");
    history.commit_group();
    assert_eq!(
        history.undo_labels().collect::<Vec<_>>(),
        vec!["Move clip +1"],
        "one gesture is one named entry"
    );
    assert!(history.pop_undo().is_some());
    assert!(history.pop_undo().is_none(), "group collapsed to one entry");
}

#[test]
fn a_single_command_group_keeps_the_command_name() {
    let mut history = History::new(8);
    history.begin_group();
    history.record_do("Split clip", Box::new(NoopAction));
    history.commit_group();
    assert_eq!(
        history.undo_labels().collect::<Vec<_>>(),
        vec!["Split clip"]
    );
}

#[test]
fn labels_read_oldest_first_on_both_stacks() {
    let mut history = History::new(8);
    history.record_do("Add clip", Box::new(NoopAction));
    history.record_do("Move clip", Box::new(NoopAction));
    assert_eq!(
        history.undo_labels().collect::<Vec<_>>(),
        vec!["Add clip", "Move clip"]
    );
    let newest = history.pop_undo().expect("a step to undo");
    history.push_redo(newest);
    assert_eq!(history.undo_labels().collect::<Vec<_>>(), vec!["Add clip"]);
    assert_eq!(history.redo_labels().collect::<Vec<_>>(), vec!["Move clip"]);
}

#[test]
fn empty_group_records_nothing() {
    let mut history = History::new(8);
    history.begin_group();
    history.commit_group();
    assert!(!history.can_undo());
}

#[test]
fn group_defers_redo_clear_until_commit() {
    let mut history = History::new(8);
    history.push_redo(step(Box::new(NoopAction)));

    history.begin_group();
    history.record_do("Move clip", Box::new(NoopAction));
    let taken = history.take_group();
    assert_eq!(taken.len(), 1);
    assert!(history.can_redo(), "rolled-back group must not clear redo");
    assert!(!history.can_undo());

    history.begin_group();
    history.record_do("Move clip", Box::new(NoopAction));
    history.commit_group();
    assert!(!history.can_redo(), "commit clears redo like any new edit");
    assert!(history.can_undo());
}
