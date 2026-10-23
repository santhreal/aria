//! A press on a recent card runs the card's row, and a press on an action on
//! the card runs that action and stops there.
//!
//! The class closed here: a card that only its small Open action opened, so a
//! press on the project's name, folder or time did nothing, and an action whose
//! press also reached the card under it, which would have Reveal open the
//! project as well. Both rows a card runs are pressed through the drawn card:
//! Open on a project still where it was remembered, and Relink on one that
//! moved, which asks for the new file instead of opening the old path.
//!
//! Not covered: the file manager Reveal starts, which a test may not start, so
//! Reveal is measured by what it leaves alone.

use std::path::PathBuf;

use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::app::picker::Answer;
use crate::test_support::drawn_shell_at;
use crate::types::{HomeRail, UiRoute};

/// Home on its Recents rail with no project open, listing one saved project,
/// and that project's path.
///
/// Saved, then replaced by a new session, so the open project is clean and has
/// no path: an open over unsaved work parks behind the unsaved-changes prompt,
/// which is that prompt's contract, and a project already open at the card's
/// path would make an open indistinguishable from no open.
fn home_with_one_recent(
    cx: &mut TestAppContext,
    tag: &str,
) -> (gpui::Entity<AppState>, VisualTestContext, PathBuf) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
    let (state, mut visual) = drawn_shell_at(cx, tag, at);
    // The route and the cards glide in; reduced motion lands them at once.
    visual.update(|_, app| app.set_reduce_motion(true));
    let path = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let dir = state.data_dir.join("projects");
            std::fs::create_dir_all(&dir)
                .unwrap_or_else(|err| panic!("a folder for the project: {err}"));
            let path = dir.join("card.aria");
            state.project_path = Some(path.clone());
            state
                .save_project()
                .expect("the project saved, so its card points at a real file");
            state
                .new_project()
                .expect("a new session, so the saved project is not the open one");
            assert!(
                !state.store.is_dirty,
                "the new session is dirty, so an open would park behind the prompt"
            );
            state.go_home();
            state.set_home_rail(HomeRail::Recents);
            cx.notify();
            path
        })
    });
    assert_eq!(
        first_recent(&mut visual, &state),
        Some((path.clone(), false)),
        "Home lists the saved project first, where it was saved"
    );
    (state, visual, path)
}

/// The first card's project and whether it is missing.
fn first_recent(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
) -> Option<(PathBuf, bool)> {
    visual.read(|app| {
        state
            .read(app)
            .recents()
            .first()
            .map(|recent| ((*recent.path).clone(), recent.missing))
    })
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual.run_until_parked();
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("Home drew no {selector}"))
}

/// Where the session is: the route and the open project's path.
fn session(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
) -> (UiRoute, Option<PathBuf>) {
    visual.read(|app| {
        let state = state.read(app);
        (state.route, state.project_path.clone())
    })
}

/// Press the first card inside its top padding, clear of its name and of the
/// actions along its bottom.
fn press_card(visual: &mut VisualTestContext) {
    let card = bounds(visual, "recent-card-0");
    visual.simulate_click(
        point(card.center().x, card.origin.y + px(4.)),
        Modifiers::none(),
    );
    visual.run_until_parked();
}

#[gpui::test]
fn a_press_on_a_recent_card_away_from_its_actions_opens_its_project(cx: &mut TestAppContext) {
    let (state, mut visual, path) = home_with_one_recent(cx, "home-card-open");
    press_card(&mut visual);
    assert_eq!(
        session(&mut visual, &state),
        (UiRoute::Editor, Some(path)),
        "a press on the card did not open the project it lists"
    );
    assert_eq!(
        visual.read(|app| state.read(app).picks_asked()),
        0,
        "a card whose project is where it was remembered asked for a file"
    );
}

#[gpui::test]
fn a_press_on_a_moved_projects_card_relinks_it_to_the_file_chosen(cx: &mut TestAppContext) {
    let (state, mut visual, path) = home_with_one_recent(cx, "home-card-relink");
    let moved = path.with_file_name("moved.aria");
    std::fs::rename(&path, &moved).unwrap_or_else(|err| panic!("move {path:?}: {err}"));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            // Home reads the disk when it is entered, which is when a card
            // learns its file is gone.
            state.go_home();
            state.supply_pick(Answer::Paths(vec![moved.clone()]));
            cx.notify();
        })
    });
    assert_eq!(
        first_recent(&mut visual, &state),
        Some((path.clone(), true)),
        "the card of a project moved away does not read as missing"
    );
    press_card(&mut visual);
    assert_eq!(
        visual.read(|app| state.read(app).picks_asked()),
        1,
        "a press on a missing project's card did not ask for its new file"
    );
    assert_eq!(
        session(&mut visual, &state),
        (UiRoute::Editor, Some(moved)),
        "the relink did not open the file chosen"
    );
    let listed: Vec<PathBuf> = visual.read(|app| {
        state
            .read(app)
            .recents()
            .iter()
            .map(|recent| (*recent.path).clone())
            .collect()
    });
    assert!(
        !listed.contains(&path),
        "the old path is still listed after the relink: {listed:?}"
    );
}

#[gpui::test]
fn a_press_on_a_cards_action_runs_that_action_and_not_the_card(cx: &mut TestAppContext) {
    let (state, mut visual, _) = home_with_one_recent(cx, "home-card-reveal");
    let card = bounds(&mut visual, "recent-card-0");
    let reveal = bounds(&mut visual, "recent-reveal-0");
    assert!(
        card.contains(&reveal.center()),
        "Reveal at {reveal:?} is not drawn on its card at {card:?}"
    );
    visual.simulate_click(reveal.center(), Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        session(&mut visual, &state),
        (UiRoute::Home, None),
        "a press on Reveal reached the card under it and opened the project"
    );
    assert!(
        visual.read(|app| state.read(app).pending_intent().is_none()),
        "a press on Reveal asked to replace the session"
    );
}
