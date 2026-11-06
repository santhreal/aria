//! `Engine::history_return` names the revision whose project a history walk
//! restores.
//!
//! WHY: a cache keyed by revision (the timeline filmstrips) follows an undo,
//! a redo or a history jump by dropping only what changed between that
//! revision and the one the walk started from. A step that named the wrong
//! revision would keep cells of a picture the walk changed back. Generated
//! sequences of edits, groups (named and unnamed), merged pairs, rolled-back
//! groups, undos, redos and jumps of any depth check, after every walk, that
//! the project equals the one recorded at the named revision.
//!
//! Does not catch: a walk whose restored project equals another revision's by
//! coincidence; every edit here writes a unique name, so none does.

use std::collections::BTreeMap;

use super::*;

/// What a walk restores: the project name and every track, in order.
fn content(project: &Project) -> (String, Vec<String>) {
    (
        project.name.clone(),
        project
            .timeline()
            .tracks_ordered()
            .map(|track| track.name.clone())
            .collect(),
    )
}

struct Session {
    engine: Engine,
    seen: BTreeMap<u64, (String, Vec<String>)>,
    unique: u64,
}

impl Session {
    fn new(undo_limit: usize) -> Self {
        let engine = Engine::new(EngineConfig { undo_limit }).expect("engine");
        let mut seen = BTreeMap::new();
        seen.insert(engine.revision(), content(engine.project()));
        Self {
            engine,
            seen,
            unique: 0,
        }
    }

    /// Record the project at the engine's revision.
    fn see(&mut self) {
        let now = content(self.engine.project());
        let revision = self.engine.revision();
        if let Some(earlier) = self.seen.insert(revision, now.clone()) {
            assert_eq!(earlier, now, "revision {revision} names two projects");
        }
    }

    fn edit(&mut self, track: bool) {
        self.unique += 1;
        let command = if track {
            EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: format!("track-{}", self.unique),
                index: None,
                pinned: false,
            }
        } else {
            EditCommand::SetProjectName {
                name: format!("name-{}", self.unique),
            }
        };
        self.engine.apply(Command::Edit(command)).expect("edit");
        self.see();
    }

    /// Walk to `depth` and check the project the walk restored. The walk
    /// takes one step at a time, as `history_jump` does, so every revision
    /// it passes is recorded for a later walk that names it.
    fn walk(&mut self, depth: usize, case: &str) {
        let applied = self.engine.history_depth();
        let listed = self.engine.history_entries().len();
        let opened = self.engine.history_return(depth);
        while self.engine.history_depth() > depth && self.engine.undo() {
            self.see();
        }
        while self.engine.history_depth() < depth && self.engine.redo() {
            self.see();
        }
        let reached = self.engine.history_depth() == depth;
        self.see();
        assert_eq!(
            reached,
            depth <= listed,
            "{case}: a walk from {applied} to {depth} of {listed}"
        );
        let Some(opened) = opened.filter(|_| reached) else {
            assert!(
                opened.is_none(),
                "{case}: depth {depth} past {listed} named revision {opened:?}"
            );
            return;
        };
        assert_eq!(
            content(self.engine.project()),
            self.seen[&opened],
            "{case}: a walk from {applied} to {depth} did not restore revision {opened}"
        );
    }
}

#[test]
fn every_history_walk_restores_the_project_at_the_revision_it_names() {
    for (seed, undo_limit) in [(1_u64, 3_usize), (2, 5), (3, 8), (4, 100), (5, 100)] {
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let mut session = Session::new(undo_limit);
        for step in 0..160 {
            let case = format!("seed {seed} limit {undo_limit} step {step}");
            match next() % 11 {
                0..=2 => session.edit(next() % 2 == 0),
                3 => {
                    let depth = session.engine.history_depth();
                    session.walk(depth.saturating_sub(1), &case);
                }
                4 => {
                    let depth = session.engine.history_depth();
                    session.walk(depth + 1, &case);
                }
                5 | 6 => {
                    let listed = session.engine.history_entries().len();
                    let depth = usize::try_from(next() % (listed as u64 + 2)).expect("depth");
                    session.walk(depth, &case);
                }
                7 => {
                    session.engine.begin_group();
                    for _ in 0..=next() % 3 {
                        session.edit(next() % 2 == 0);
                    }
                    if next() % 2 == 0 {
                        session.engine.commit_group();
                    } else {
                        session.engine.commit_group_as("Grouped");
                    }
                }
                8 => {
                    session.engine.begin_group();
                    session.edit(true);
                    session.engine.rollback_group();
                    session.see();
                }
                9 => {
                    session.edit(false);
                    session.edit(true);
                    session.engine.coalesce_last_two_undo_steps();
                }
                _ => {
                    let depth = session.engine.history_depth();
                    assert_eq!(
                        session.engine.history_return(depth),
                        Some(session.engine.revision()),
                        "{case}: a walk that moves nothing restores the current project"
                    );
                }
            }
        }
    }
}
