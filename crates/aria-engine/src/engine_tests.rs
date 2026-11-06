//! The session history: a failed undo or redo, history labels, grouped
//! rollback, and a jump that cannot reach its depth.

use super::*;

/// An inverse that always fails, simulating a corrupted/mismatched history
/// entry.
struct FailAction;

impl EditAction for FailAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Err(aria_models::ModelError::InvalidRange.into())
    }
}

struct NoopAction;

impl EditAction for NoopAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Ok(Box::new(NoopAction))
    }
}

fn step(action: Box<dyn EditAction>) -> HistoryStep {
    HistoryStep {
        id: 0,
        opened: 0,
        label: std::borrow::Cow::Borrowed("Test step"),
        action,
    }
}

#[test]
fn failed_undo_clears_both_stacks() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    // A redo entry that WOULD apply, plus a failing undo entry on top.
    engine.history.push_redo(step(Box::new(NoopAction)));
    engine.history.push_undo(step(Box::new(FailAction)));
    assert!(engine.can_undo() && engine.can_redo());

    assert!(!engine.undo(), "a failing inverse reports no step");
    assert!(
        !engine.can_undo() && !engine.can_redo(),
        "a diverged history is dropped wholesale rather than trusted"
    );
}

#[test]
fn failed_redo_clears_both_stacks() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    engine.history.push_redo(step(Box::new(FailAction)));
    assert!(engine.can_undo() && engine.can_redo());

    assert!(!engine.redo());
    assert!(!engine.can_undo() && !engine.can_redo());
}

#[test]
fn history_entries_borrow_static_and_grouped_labels() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    engine.history.begin_group();
    engine
        .history
        .record_do(0, "Grouped step", Box::new(NoopAction));
    engine
        .history
        .record_do(0, "Second step", Box::new(NoopAction));
    engine.history.commit_group();

    let stored: Vec<(*const u8, usize)> = engine
        .history
        .undo_labels()
        .map(|label| (label.as_ptr(), label.len()))
        .collect();
    let entries = engine.history_entries();
    assert_eq!(entries.len(), stored.len());
    for (entry, (ptr, len)) in entries.iter().zip(stored) {
        assert!(
            matches!(entry.label, Cow::Borrowed(_)),
            "history inspection allocated an owned label"
        );
        assert_eq!((entry.label.as_ptr(), entry.label.len()), (ptr, len));
    }
}

#[test]
fn grouped_rollback_bumps_revision_once_for_every_group_size() {
    for action_count in [1, 2, 8] {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.begin_group();
        for _ in 0..action_count {
            engine
                .history
                .record_do(0, "Test step", Box::new(NoopAction));
        }
        let before = engine.revision();

        engine.rollback_group();

        assert_eq!(
            engine.revision(),
            before + 1,
            "{action_count} reverted actions must publish one restored state"
        );
    }
}

#[test]
fn a_jump_that_cannot_reach_its_depth_reports_failure() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    // Nothing to redo, so a depth past the end is unreachable and the walk
    // stops instead of spinning.
    assert!(!engine.history_jump(4));
    assert_eq!(engine.history_depth(), 1);
    assert!(engine.history_jump(0), "walking back is reachable");
    assert_eq!(engine.history_depth(), 0);
    assert_eq!(
        engine
            .history_entries()
            .into_iter()
            .map(|entry| (entry.label, entry.applied))
            .collect::<Vec<_>>(),
        vec![(Cow::Borrowed("Test step"), false)],
        "an undone step stays in the list, marked unapplied"
    );
}

/// What the model compares: the parts of a project these sequences edit.
fn project_content(
    project: &Project,
) -> (
    String,
    Vec<(String, TrackKind)>,
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<u8>>>,
    String,
) {
    (
        project.name.clone(),
        project
            .timeline()
            .tracks_ordered()
            .map(|t| (t.name.clone(), t.kind))
            .collect(),
        project.plugin_data.clone(),
        project.metadata().agent_rules.clone(),
    )
}

/// WHY: closes the class where `Engine::is_dirty` compared the monotonic
/// `revision` against a saved revision, so undoing back to saved content left
/// the session dirty, and where changes that record no history step
/// (`set_plugin_storage`, eviction at `undo_limit`, rolled-back groups) could
/// put the saved step out of step with the undo stack. Drives generated
/// sequences of edits, undos, redos, grouped commits and rollbacks, saves and
/// plugin storage writes across seeds and `undo_limit` bounds against a model
/// of saved content and step-less writes.
///
/// Does not catch: two different steps that leave identical content read
/// dirty against each other, which the model never generates (every name is
/// unique); the window title and recovery copy on `AppState` (see
/// `crates/aria-ui/src/app/recovery_tests.rs`).
#[test]
fn generated_edit_undo_redo_save_and_storage_sequences_match_saved_content_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    for (seed_idx, undo_limit) in [(1_u64, 3_usize), (2, 4), (3, 6), (4, 16), (5, 32)] {
        let mut rng = seed_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next_u64 = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        let mut engine = Engine::new(EngineConfig { undo_limit }).expect("engine");
        // A save adopts the file stem as the project name outside history;
        // `untitled` is the stem it leaves alone, so the name stays an edit.
        let seed_dir = dir.path().join(format!("seed{seed_idx}"));
        std::fs::create_dir_all(&seed_dir).expect("seed dir");
        let save_path = seed_dir.join("untitled.aria");
        let mut saved_content = project_content(engine.project());
        let mut unhistoried_write = false;
        let mut unique_seq = 0_u64;

        for step_idx in 0..100 {
            let prev_rev = engine.revision();
            let op = next_u64() % 12;
            match op {
                0..=3 => {
                    unique_seq += 1;
                    engine
                        .apply(Command::Edit(EditCommand::SetProjectName {
                            name: format!("proj-{seed_idx}-{unique_seq}"),
                        }))
                        .expect("set project name");
                    assert!(
                        engine.revision() > prev_rev,
                        "edit must advance monotonic revision"
                    );
                }
                4 => {
                    unique_seq += 1;
                    engine
                        .apply(Command::Edit(EditCommand::AddTrack {
                            kind: TrackKind::Video,
                            name: format!("track-{seed_idx}-{unique_seq}"),
                            index: None,
                            pinned: false,
                        }))
                        .expect("add track");
                    assert!(engine.revision() > prev_rev);
                }
                5 | 6 => {
                    if engine.undo() {
                        assert!(
                            engine.revision() > prev_rev,
                            "undo must advance monotonic revision"
                        );
                    }
                }
                7 | 8 => {
                    if engine.redo() {
                        assert!(
                            engine.revision() > prev_rev,
                            "redo must advance monotonic revision"
                        );
                    }
                }
                9 => {
                    engine
                        .apply(Command::Project(ProjectCommand::Save {
                            path: save_path.clone(),
                        }))
                        .expect("save project");
                    saved_content = project_content(engine.project());
                    unhistoried_write = false;
                }
                10 => {
                    unique_seq += 1;
                    let changed = engine.set_plugin_storage(
                        "com.example.model",
                        "cursor",
                        format!("v-{seed_idx}-{unique_seq}").into_bytes(),
                    );
                    assert!(changed);
                    assert!(engine.revision() > prev_rev);
                    unhistoried_write = true;
                }
                _ => {
                    engine.begin_group();
                    unique_seq += 1;
                    engine
                        .apply(Command::Edit(EditCommand::SetProjectName {
                            name: format!("grp-{seed_idx}-{unique_seq}"),
                        }))
                        .expect("group edit");
                    assert!(
                        engine.is_dirty(),
                        "mid-group edit must read dirty before commit or rollback"
                    );
                    if next_u64() % 2 == 0 {
                        engine.commit_group();
                    } else {
                        engine.rollback_group();
                    }
                    assert!(engine.revision() > prev_rev);
                }
            }

            let content_differs = project_content(engine.project()) != saved_content;
            let expected_dirty = content_differs || unhistoried_write;
            assert_eq!(
                engine.is_dirty(),
                expected_dirty,
                "seed={seed_idx} (undo_limit={undo_limit}) step={step_idx} op={op}: \
                 is_dirty()={} but content_differs={} unhistoried_write={}",
                engine.is_dirty(),
                content_differs,
                unhistoried_write,
            );
        }
    }
}

#[test]
fn dirty_flag_tracks_eviction_unhistoried_writes_failed_inverses_and_coalescing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let save_path = dir.path().join("boundaries.aria");

    // 1. Eviction past `undo_limit` from an initial save (depth 0) and from a step save (depth 1).
    let mut engine = Engine::new(EngineConfig { undo_limit: 2 }).expect("engine");
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save at depth 0");
    assert!(!engine.is_dirty());
    for name in ["e1", "e2", "e3"] {
        engine
            .apply(Command::Edit(EditCommand::SetProjectName {
                name: name.into(),
            }))
            .expect("edit");
    }
    while engine.undo() {}
    assert_eq!(engine.project().name, "e1");
    assert!(
        engine.is_dirty(),
        "evicting e1 past undo_limit must leave session dirty when undoing back to bottom"
    );

    // Save at the current state ("e1"), then push 3 more edits so "e1" and "e2_b" are evicted.
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save at e1");
    assert!(!engine.is_dirty());
    for name in ["e2_b", "e3_b", "e4_b"] {
        engine
            .apply(Command::Edit(EditCommand::SetProjectName {
                name: name.into(),
            }))
            .expect("edit");
    }
    while engine.undo() {}
    assert_eq!(engine.project().name, "e2_b");
    assert!(
        engine.is_dirty(),
        "evicting saved step past undo_limit must keep session dirty"
    );

    // 2. `mark_unsaved`, `set_agent_rules`, `set_plugin_storage`, `delete_plugin_storage`
    // and a rolled-back group containing an unhistoried write.
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save");
    assert!(!engine.is_dirty());
    engine.mark_unsaved();
    assert!(engine.is_dirty(), "mark_unsaved must dirty a saved session");

    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save");
    engine.set_agent_rules("rule-1".into());
    assert!(engine.is_dirty(), "set_agent_rules must dirty session");

    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save");
    engine.begin_group();
    engine
        .apply(Command::Edit(EditCommand::SetProjectName {
            name: "in-group".into(),
        }))
        .expect("edit in group");
    engine.set_plugin_storage("p", "k", b"v".to_vec());
    engine.rollback_group();
    assert!(
        engine.is_dirty(),
        "rolling back a group must not clear dirty state from an unhistoried storage write"
    );

    // 3. Coalescing across a saved step vs coalescing before save.
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save before c1");
    engine
        .apply(Command::Edit(EditCommand::SetProjectName {
            name: "c1".into(),
        }))
        .expect("c1");
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: save_path.clone(),
        }))
        .expect("save at c1");
    assert!(!engine.is_dirty());
    engine
        .apply(Command::Edit(EditCommand::SetProjectName {
            name: "c2".into(),
        }))
        .expect("c2");
    engine.coalesce_last_two_undo_steps();
    assert!(engine.undo());
    assert!(
        engine.is_dirty(),
        "undoing a coalesced c1+c2 step steps past saved state c1 and must read dirty"
    );
    assert!(engine.redo());
    assert!(
        engine.is_dirty(),
        "redoing coalesced c1+c2 lands at c2, which differs from saved state c1"
    );

    // 4. A failed undo or redo clears history; saved at the empty stack, the
    // session must read dirty afterwards.
    let mut fail_engine = Engine::new(EngineConfig::default()).expect("engine");
    assert!(!fail_engine.is_dirty());
    fail_engine.history.push_undo(step(Box::new(FailAction)));
    assert!(!fail_engine.undo());
    assert!(
        fail_engine.is_dirty(),
        "failed undo clearing history must leave session dirty"
    );
    let mut fail_redo = Engine::new(EngineConfig::default()).expect("engine");
    fail_redo.history.push_redo(step(Box::new(FailAction)));
    assert!(!fail_redo.is_dirty(), "a redo step alone changes nothing");
    assert!(!fail_redo.redo());
    assert!(
        fail_redo.is_dirty(),
        "failed redo clearing history must leave session dirty"
    );
}

/// Attaching a sequence marker to a clip joins the top undo step without a new
/// step id, so a save taken at that step no longer matches the project.
#[test]
fn attaching_a_marker_after_a_save_reads_unsaved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let rate = Rational::FPS_24;
    let mut project = Project::new("attach", rate);
    let media = project.add_media(aria_models::MediaSource::new(
        "/w.mp4", 64, 36, rate, 240, false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            aria_models::TimeRange::at_rate(0, 48, rate),
            RationalTime::new(0, rate),
        )
        .expect("clip");
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.reset_project(project);
    let outcome = engine
        .apply(Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(12, rate),
            name: "m".into(),
            color: None,
        }))
        .expect("add marker");
    let ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedMarker(marker)) = outcome else {
        panic!("expected a created marker, got {outcome:?}");
    };
    engine
        .apply(Command::Project(ProjectCommand::Save {
            path: dir.path().join("attach.aria"),
        }))
        .expect("save");
    assert!(!engine.is_dirty());

    engine.attach_marker_to_clip(marker, clip).expect("attach");
    assert!(
        engine.is_dirty(),
        "a marker attached after the save is not in the file"
    );
}
