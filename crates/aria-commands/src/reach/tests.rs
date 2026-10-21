//! Contracts for the reach declaration.
//!
//! The class closed here is "a command drifts out of every UI surface". The
//! registry's exhaustive variant-to-row match already refuses to compile when a
//! variant is added, so what is left to assert is that a declaration is
//! meaningful: an unreachable command carries a real reason, and the set of
//! unreachable commands does not grow quietly.
//!
//! What this does not catch: whether a named surface is really there. A row
//! states which control sends the command, and only the editor can drive one,
//! so the census that dispatches every palette row, menu row and chord lives in
//! `aria-ui`. This crate cannot see a menu.

use super::Reach;
use crate::command::ProjectCommand;
use crate::edit_command::EditCommand;
use crate::samples::{every_edit_command, every_project_command};

/// Every command's label paired with its declared reach.
fn declared() -> Vec<(&'static str, Reach)> {
    every_edit_command()
        .into_iter()
        .map(|cmd| (cmd.label(), cmd.reach()))
        .chain(
            every_project_command()
                .into_iter()
                .map(|cmd| (cmd.label(), cmd.reach())),
        )
        .collect()
}

#[test]
fn an_unreachable_command_states_why() {
    for (label, reach) in declared() {
        if let Some(reason) = reach.reason() {
            assert!(
                reason.len() > 20,
                "{label} declares no operator path but gives no usable reason"
            );
        }
    }
}

/// The commands no editor control sends. `Export` blocks, so the desktop
/// queues background work instead; the three placements put one clip on one
/// lane for the assistant, scripts, and the mobile app, while every desktop
/// placement is a patched `Source edit`.
const UNREACHABLE: [&str; 4] = ["Export", "Fit to fill", "Overwrite", "Ripple insert"];

#[test]
fn the_unreachable_set_is_locked() {
    // Promoting a command to a UI surface, or adding one that has no
    // surface, is a decision. Both show up here rather than in nobody's diff.
    let mut unreachable: Vec<&'static str> = declared()
        .into_iter()
        .filter(|(_, reach)| !reach.is_operator())
        .map(|(label, _)| label)
        .collect();
    unreachable.sort_unstable();
    assert_eq!(unreachable, UNREACHABLE);
}

#[test]
fn a_command_with_no_surface_yet_names_the_work_that_blocks_it() {
    // `Planned` and `Internal` are different statements: one is waiting for
    // something, the other never gets a control. A row that only says "not yet"
    // is how a gap becomes invisible, so the text has to name the ledger row
    // that owns it.
    for (label, reach) in declared() {
        if let Reach::Planned(missing) = reach {
            assert!(
                missing.contains("ledger row"),
                "{label} is planned but names no ledger row: {missing}"
            );
        }
    }
}

#[test]
fn every_operator_command_names_its_surface() {
    // The whole point of the payload: a command nobody built a control for
    // cannot be declared reachable without someone writing down where the
    // control is, and finding that there is nowhere to write.
    for (label, reach) in declared() {
        if let Some(surface) = reach.surface() {
            assert!(
                surface.len() > 20 && surface.contains(' '),
                "{label} claims a UI surface without naming it: {surface:?}"
            );
        }
    }
}

#[test]
fn every_other_command_claims_an_operator_surface() {
    let all = declared();
    let operator = all.iter().filter(|(_, r)| r.is_operator()).count();
    assert_eq!(operator, all.len() - UNREACHABLE.len());
}

#[test]
fn a_reason_is_only_carried_by_an_unreachable_command() {
    for (label, reach) in declared() {
        match reach {
            Reach::Operator(surface) => {
                assert_eq!(reach.surface(), Some(surface), "{label}");
                assert_eq!(reach.reason(), None, "{label}");
            }
            Reach::Planned(reason) | Reach::Internal(reason) => {
                assert_eq!(reach.surface(), None, "{label}");
                assert_eq!(reach.reason(), Some(reason), "{label}");
                assert!(
                    !reason.trim().is_empty(),
                    "{label} is unreachable without a reason"
                );
            }
        }
    }
}

/// Reach reads no field, so it must not depend on one: two samples of the same
/// variant with different payloads answer the same.
#[test]
fn reach_is_a_property_of_the_variant_not_the_payload() {
    let a = EditCommand::SetProjectName { name: "a".into() };
    let b = EditCommand::SetProjectName {
        name: "bbbb".into(),
    };
    assert_eq!(a.reach(), b.reach());
    let c = ProjectCommand::Import { path: "/x".into() };
    let d = ProjectCommand::Import {
        path: "/y/z.mp4".into(),
    };
    assert_eq!(c.reach(), d.reach());
}
