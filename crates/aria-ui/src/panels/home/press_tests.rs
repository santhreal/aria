//! A recent card is one control: a press anywhere on it runs the card's row,
//! a right-click opens its project's menu, and it draws its facts and no
//! buttons.
//!
//! The classes closed here: a card that only its small Open action opened, so
//! a press on the project's name, folder or time did nothing; a card that drew
//! Open and Reveal buttons on every project, a second Open beside the card's
//! own press and a Reveal its menu already offers; and a card whose menu, the
//! only place Reveal is left, could not be opened from it. Both rows a press
//! runs are pressed through the drawn card: Open on a project still where it
//! was remembered, and Relink on one that moved, which asks for the new file
//! instead of opening the old path.
//!
//! Not covered: the file manager Reveal starts, which a test may not start.

use std::path::PathBuf;

use gpui::{
    Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px, size,
};

use crate::app::AppState;
use crate::app::picker::Answer;
use crate::test_support::drawn_shell_at;
use crate::types::{ContextTarget, HomeRail, UiRoute};

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

/// A point inside the first card's top padding, clear of its lines.
fn on_card(visual: &mut VisualTestContext) -> gpui::Point<Pixels> {
    let card = bounds(visual, "recent-card-0");
    point(card.center().x, card.origin.y + px(4.))
}

/// Press the first card inside its top padding.
fn press_card(visual: &mut VisualTestContext) {
    let at = on_card(visual);
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn a_press_on_a_recent_card_opens_its_project(cx: &mut TestAppContext) {
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
    // The press is the card's only action, so the card states what it does.
    let card = bounds(&mut visual, "recent-card-0");
    let states_relink = visual.update(|window, _| {
        window.rendered_text_runs().iter().any(|run| {
            card.contains(&run.bounds.center()) && run.text.as_ref() == "Missing · click to relink"
        })
    });
    assert!(
        states_relink,
        "a moved project's card does not state that a press relinks it"
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
fn a_recent_card_draws_its_name_folder_and_time_and_nothing_else(cx: &mut TestAppContext) {
    let (state, mut visual, _) = home_with_one_recent(cx, "home-card-facts");
    let card = bounds(&mut visual, "recent-card-0");
    let (label, detail, opened) = visual.read(|app| {
        let recent = state.read(app).recents().first().cloned();
        let recent = recent.expect("the fixture lists one project");
        (recent.label, recent.detail, recent.opened)
    });
    let mut lines: Vec<(f32, String)> = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| card.contains(&run.bounds.center()))
            .map(|run| (f32::from(run.bounds.top()), run.text.to_string()))
            .collect()
    });
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    let lines: Vec<String> = lines.into_iter().map(|(_, text)| text).collect();
    assert_eq!(
        lines.len(),
        3,
        "the card draws {lines:?}, not its name, folder and time"
    );
    assert_eq!(lines[0], label.as_ref(), "the first line is not the name");
    // The folder gives up its start to stay on one line.
    let folder = lines[1].trim_start_matches('…');
    assert!(
        !folder.is_empty() && detail.ends_with(folder),
        "the second line `{}` is not the end of the folder `{detail}`",
        lines[1]
    );
    assert_eq!(lines[2], opened.as_ref(), "the last line is not the time");
}

#[gpui::test]
fn a_right_click_on_a_recent_card_opens_its_projects_menu(cx: &mut TestAppContext) {
    let (state, mut visual, path) = home_with_one_recent(cx, "home-card-menu");
    let at = on_card(&mut visual);
    visual.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    visual.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).context_menu.clone()),
        Some(ContextTarget::Recent { path }),
        "a right-click on the card did not open its project's menu"
    );
    assert_eq!(
        session(&mut visual, &state),
        (UiRoute::Home, None),
        "a right-click on the card opened the project"
    );
    let drawn: Vec<String> = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .map(|run| run.text.to_string())
            .collect()
    });
    // Open is also a Start tile, so only the rows nothing else on Home draws
    // prove the menu is up.
    for row in ["Reveal", "Remove from List", "Delete File…"] {
        assert!(
            drawn.iter().any(|text| text == row),
            "the card's menu draws no {row}: {drawn:?}"
        );
    }
}
