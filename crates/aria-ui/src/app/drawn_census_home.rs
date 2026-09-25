//! Driving the Home screen: the tiles, a recent project's actions, and the
//! recovery offer.
//!
//! # Why
//!
//! `Load project` claimed this screen and nothing pressed it. The recovery
//! banner's Restore is the only control that sends it, and the banner's other
//! button throws the copy away, so the two are one mis-wiring apart with an
//! afternoon of work between them. The registry is
//! [`home_pick`](crate::surfaces::home_pick) and this presses every row of it.
//!
//! Every tile here replaces the session, so the fixture is a *saved* project: an
//! intent asked for over unsaved work is parked behind the unsaved-changes
//! prompt and reaches nothing, which is that prompt's contract rather than this
//! one. `unsaved_tests` owns the parked path.
//!
//! The rows that open a file dialog are driven with the answer supplied, the way
//! the menu census drives Open: the dialog is answered, never opened.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::app::AppState;
use crate::app::picker::Answer;
use crate::app::sent_census::Watch;
use crate::surfaces::home_pick::{self, Lands, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "home_pick.rs";

/// A session sitting on Home, with everything its rows act on.
struct Dressed {
    state: AppState,
    /// The project in the recents list.
    remembered: PathBuf,
    /// Where a relink points that project after it moved.
    moved_to: PathBuf,
    /// A template bundle with no slots, so applying it asks no further question.
    bundle: PathBuf,
}

/// Dress Home: a project remembered, a bundle to apply, a copy to recover.
///
/// Saved rather than dirty, because every tile on this screen replaces the
/// session: over unsaved work each one parks itself behind the prompt and asks
/// for nothing, so a census pressing them there would measure the prompt.
fn at_home() -> Dressed {
    let (mut state, remembered) = crate::app::fixtures::remembered_project();
    let dir = crate::scratch_pool::scratch("home-census");
    let bundle = dir.join("bundle.ariat");
    let moved_to = dir.join("moved.aria");
    std::fs::write(&moved_to, b"{}").expect("a project file at the new path");
    state
        .save_template(bundle.clone(), "Census".into())
        .expect("a template bundle with no slots to fill");
    state
        .save_project()
        .expect("a saved project, so a tile runs its intent instead of parking it");
    assert!(
        !state.store.is_dirty,
        "the fixture is still dirty, so every tile would park behind the prompt"
    );
    state.write_recovery().expect("a recovery copy on disk");
    assert!(
        state.pending_recovery().is_some(),
        "the banner has no offer to draw, so its two buttons are not drawn either"
    );
    Dressed {
        state,
        remembered,
        moved_to,
        bundle,
    }
}

impl Dressed {
    /// Every control the screen draws, in the order it draws them, pointed at
    /// this session's own files.
    ///
    /// Read off the state they are pressed against: the paths are per-fixture,
    /// so a row carrying one from an earlier build would name a file this
    /// session never remembered.
    fn picks(&self) -> Vec<Pick> {
        vec![
            Pick::NewProject,
            Pick::PickProject,
            Pick::OpenRecent(self.remembered.clone()),
            Pick::RelinkRecent(self.remembered.clone()),
            Pick::OpenSample,
            Pick::UseTemplate,
            Pick::RestoreRecovery,
            Pick::DiscardRecovery,
            Pick::Reveal(self.remembered.clone()),
        ]
    }

    /// What the file dialog answers for one row, if it asks.
    ///
    /// A relink answers with the project's new home, which is the whole gesture:
    /// the old path is dropped and the new one opened.
    fn answer(&self, pick: &Pick) -> Option<PathBuf> {
        match pick {
            Pick::PickProject => Some(self.remembered.clone()),
            Pick::RelinkRecent(_) => Some(self.moved_to.clone()),
            Pick::UseTemplate => Some(self.bundle.clone()),
            _ => None,
        }
    }

    /// Press one control the way the screen does, answering the dialog first for
    /// the rows that open one.
    fn press(&mut self, pick: Pick) {
        if let Some(answer) = self.answer(&pick) {
            self.state.supply_pick(Answer::Paths(vec![answer]));
        }
        let what = pick.what();
        home_pick::apply(&mut self.state, pick)
            .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
    }
}

/// Every command the screen's controls asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    let count = at_home().picks().len();
    for index in 0..count {
        let mut home = at_home();
        let pick = home.picks().swap_remove(index);
        if pick.leaves_the_editor() {
            continue;
        }
        let watch = Watch::on(&home.state);
        home.press(pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_control_the_screen_draws_is_pressed() {
    let pressed: BTreeSet<String> = at_home()
        .picks()
        .into_iter()
        .map(|pick| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these Home controls are drawn and nothing presses them: {missing:?}"
    );
}

#[test]
fn only_a_row_that_leaves_the_editor_goes_unpressed() {
    // Fail by default: a new row that opts out turns this red until somebody
    // records why a test must not press it. Nothing else may opt out.
    const DECLARED: [(&str, &str); 1] = [(
        "a project shown in the file manager",
        "hands the path to the desktop's file manager, which would open a window \
         on whoever ran the suite",
    )];
    let home = at_home();
    let outside: Vec<&'static str> = home
        .picks()
        .iter()
        .filter(|pick| pick.leaves_the_editor())
        .map(Pick::what)
        .collect();
    let declared: Vec<&str> = DECLARED.iter().map(|(what, _)| *what).collect();
    assert_eq!(
        outside, declared,
        "a Home row opts out of the census without a recorded reason"
    );
}

#[test]
fn every_row_lands_what_it_says_it_lands() {
    // Not every row is an edit: a new project resets the session and the discard
    // deletes a file beside the app's data, so holding both to "sends a command"
    // would either fail or be weakened until it proved nothing. Each row is
    // asserted against its own column instead, and a row with no column does not
    // compile.
    let count = at_home().picks().len();
    for index in 0..count {
        let mut home = at_home();
        let pick = home.picks().swap_remove(index);
        if pick.leaves_the_editor() {
            continue;
        }
        let what = pick.what();
        let lands = pick.lands();
        let watch = Watch::on(&home.state);
        home.press(pick);
        let sent = watch.take();
        match lands {
            Lands::Command => assert!(
                !sent.is_empty(),
                "{what} asked for nothing, so the tile is wired to nothing"
            ),
            Lands::Session | Lands::Recovery => assert!(
                sent.is_empty(),
                "{what} sent {sent:?}, so it is an edit rather than a session change"
            ),
            Lands::Outside => unreachable!("an outside row is skipped above"),
        }
    }
}

#[test]
fn a_new_project_replaces_the_session_without_an_edit() {
    // The row that sends nothing has to be held to something, or "sends nothing"
    // covers a tile wired to nothing at all. What it lands is the session: the
    // pool the fixture filled is empty and the project has no file behind it.
    let mut home = at_home();
    assert!(
        !home.state.store.media.is_empty() && home.state.project_path.is_some(),
        "the fixture has neither a pool nor a saved path, so this proves nothing"
    );
    home.press(Pick::NewProject);
    assert!(
        home.state.store.media.is_empty(),
        "the new project kept the old project's pool"
    );
    assert!(
        home.state.project_path.is_none(),
        "the new project still points at the file the old one was saved to"
    );
}

#[test]
fn the_banner_restores_the_copy_rather_than_dropping_it() {
    // The class the missing surface hid: two buttons an inch apart, one of which
    // is the only way back to the work. Restore must load the copy and take the
    // offer down; discard must take it down and touch the project not at all.
    let mut home = at_home();
    let watch = Watch::on(&home.state);
    home.press(Pick::RestoreRecovery);
    assert!(
        watch.take().contains(&"Load project"),
        "restoring the recovery copy loaded nothing"
    );
    assert!(
        home.state.pending_recovery().is_none(),
        "the offer survived being taken, so it would be offered again"
    );

    let mut home = at_home();
    let before = home.state.store.sequence.tracks.len();
    let watch = Watch::on(&home.state);
    home.press(Pick::DiscardRecovery);
    assert!(
        watch.take().is_empty(),
        "discarding the recovery copy edited the project"
    );
    assert!(
        home.state.pending_recovery().is_none(),
        "the discarded copy is still on offer"
    );
    assert_eq!(
        home.state.store.sequence.tracks.len(),
        before,
        "discarding the copy changed the open project"
    );
}

#[test]
fn a_relink_forgets_the_path_that_moved() {
    // Relink is Open with a step before it: the card that could not be opened
    // must stop being drawn, or the list keeps offering a file that is not there.
    let mut home = at_home();
    let old = home.remembered.clone();
    let listed = |state: &AppState, path: &PathBuf| -> bool {
        state
            .recents()
            .iter()
            .any(|recent| recent.path.as_ref() == path)
    };
    assert!(
        listed(&home.state, &old),
        "the fixture's project is not in the list"
    );
    home.press(Pick::RelinkRecent(old.clone()));
    assert!(
        !listed(&home.state, &old),
        "the moved project is still listed under the path that moved"
    );
}

#[test]
fn the_census_reaches_every_command_these_controls_claim() {
    let sent = dispatched();
    for label in ["Load project", "Open project", "Apply template"] {
        assert!(
            sent.contains(label),
            "the Home census pressed every control the screen draws and none of \
             them sent `{label}`"
        );
    }
}

#[test]
fn neither_home_file_calls_a_setter_itself() {
    // Home is two files: the route and the cards it draws. Both are held to the
    // registry.
    super::no_setter_escapes(REGISTRY, "home/mod.rs");
    super::no_setter_escapes(REGISTRY, "home/cards.rs");
}
