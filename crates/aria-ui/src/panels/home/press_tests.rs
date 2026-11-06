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
//! The rail's New and Open rows and Ctrl+O on Home run their action on the
//! first press, as a card does.
//!
//! Not covered: the file manager Reveal starts, which a test may not start.

use std::path::{Path, PathBuf};

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

/// Press one of the rail's rows at its center.
fn press_rail(visual: &mut VisualTestContext, selector: &'static str) {
    let row = bounds(visual, selector);
    visual.simulate_click(row.center(), Modifiers::none());
    visual.run_until_parked();
}

/// Back on Home with a clean session, a file chosen for the next pick.
fn home_again(visual: &mut VisualTestContext, state: &gpui::Entity<AppState>, pick: &Path) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.new_project().expect("a clean session");
            state.go_home();
            state.supply_pick(Answer::Paths(vec![pick.to_path_buf()]));
            cx.notify();
        })
    });
}

/// WHY: New and Open on the rail were panes holding one tile that repeated
/// them, and Ctrl+O on Home switched to that pane, so starting or opening a
/// project from Home took two presses. Closes "a Home entry point that only
/// moves to another one" for the rail's two rows and the shortcut: each runs
/// its action on the first press. Does not cover Ctrl+Alt+N, which already
/// ran `new_project`.
#[gpui::test]
fn the_rails_new_and_open_and_ctrl_o_run_at_the_first_press(cx: &mut TestAppContext) {
    let (state, mut visual, path) = home_with_one_recent(cx, "home-rail-actions");
    let visual = &mut visual;
    home_again(visual, &state, &path);
    press_rail(visual, "home-rail-Open");
    let asked = visual.read(|app| state.read(app).picks_asked());
    assert_eq!(asked, 1, "the rail's Open asked for no file");
    assert_eq!(
        session(visual, &state),
        (UiRoute::Editor, Some(path.clone())),
        "the rail's Open did not open the file chosen"
    );

    home_again(visual, &state, &path);
    press_rail(visual, "home-rail-New");
    assert_eq!(
        session(visual, &state),
        (UiRoute::Editor, None),
        "the rail's New did not start an empty project"
    );

    home_again(visual, &state, &path);
    visual.simulate_keystrokes("ctrl-o");
    visual.run_until_parked();
    let asked = visual.read(|app| state.read(app).picks_asked());
    assert_eq!(asked, 2, "Ctrl+O on Home asked for no file");
    assert_eq!(
        session(visual, &state),
        (UiRoute::Editor, Some(path)),
        "Ctrl+O on Home did not open the file chosen"
    );
}

fn pick_variant_names_from_source() -> Vec<&'static str> {
    let src = include_str!("../../surfaces/home_pick.rs");
    let body = src
        .split("pub enum Pick {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("pub enum Pick body in surfaces/home_pick.rs");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(|line| {
            line.trim_end_matches(',')
                .split('(')
                .next()
                .expect("variant identifier")
                .trim()
        })
        .collect()
}
fn press_selector(visual: &mut VisualTestContext, selector: &'static str) {
    let target = bounds(visual, selector);
    visual.simulate_click(target.center(), Modifiers::none());
    visual.run_until_parked();
}
fn press_text(visual: &mut VisualTestContext, label: &str) {
    let at = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .find(|run| run.text.as_ref() == label)
            .unwrap_or_else(|| panic!("Home drew no text `{label}`"))
            .bounds
            .center()
    });
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
}
fn strip_draws(visual: &mut VisualTestContext, expected: &str) -> bool {
    visual.run_until_parked();
    visual.update(|window, _| {
        window.rendered_text_runs().iter().any(|run| {
            let text = run.text.as_ref();
            f32::from(run.bounds.origin.y) >= 750.0
                && (text == expected
                    || text
                        .strip_suffix('…')
                        .is_some_and(|prefix| !prefix.is_empty() && expected.starts_with(prefix)))
        })
    })
}
fn assert_home_error_drawn(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    label: &str,
) -> String {
    assert_eq!(
        session(visual, state),
        (UiRoute::Home, None),
        "{label} left Home"
    );
    let err = visual
        .read(|app| state.read(app).last_error.clone())
        .unwrap_or_else(|| panic!("{label} did not set last_error"));
    assert!(
        strip_draws(visual, &err),
        "Home status-strip did not draw {label} error `{err}`"
    );
    err
}
fn assert_error_cleared(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    prior_err: &str,
    label: &str,
) {
    assert_eq!(
        visual.read(|app| state.read(app).last_error.clone()),
        None,
        "{label} did not clear last_error"
    );
    assert!(
        !strip_draws(visual, prior_err),
        "strip kept stale error after {label}"
    );
}

fn reset_to_clean_home(visual: &mut VisualTestContext, state: &gpui::Entity<AppState>) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.new_project().expect("clean session");
            state.clear_error();
            state.go_home();
            state.set_home_rail(HomeRail::Recents);
            cx.notify();
        });
    });
    visual.run_until_parked();
}

/// WHY: Home omitted `status_strip` and startup CLI open failures only wrote to
/// stderr, so every `AppState::report_error` raised on Home was invisible, and
/// `NewProject`, `DiscardRecovery`, `OpenSample`, and `DropOnHome` either left
/// stale errors on the strip after succeeding or stranded a failed Home open in
/// `UiRoute::Editor`. Closes the class by deriving `Pick` variants from
/// `surfaces/home_pick.rs` at run time, forcing each fallible `Pick` and
/// `AppState::open_cli_paths` to fail on Home, asserting the drawn Home
/// `status-strip` renders the exact `report_error` text, and verifying that the
/// next successful action clears the error. Does not cover stderr formatting.
#[gpui::test]
fn every_home_pick_failure_and_cli_open_failure_draws_on_home_until_the_next_success(
    cx: &mut TestAppContext,
) {
    use crate::surfaces::home_pick::Pick;

    let (state, mut visual, valid_project_path) = home_with_one_recent(cx, "home-errors");
    let visual = &mut visual;
    let valid_bytes =
        std::fs::read(&valid_project_path).expect("read valid saved project fixture bytes");
    let dir = valid_project_path
        .parent()
        .expect("project parent dir")
        .to_path_buf();
    let bad_project = dir.join("corrupt.aria");
    std::fs::write(&bad_project, b"not valid project json").expect("write corrupt .aria");
    let bad_template = dir.join("corrupt.ariat");
    std::fs::write(&bad_template, b"not valid template zip").expect("write corrupt .ariat");

    let declared = pick_variant_names_from_source();
    let picks = Pick::all(&valid_project_path);
    assert_eq!(
        picks.len(),
        declared.len(),
        "Pick::all count ({}) disagrees with pub enum Pick variants in source ({declared:?})",
        picks.len()
    );

    let mut decided = Vec::new();
    for pick in &picks {
        match pick {
            Pick::NewProject => {
                decided.push("NewProject");
                reset_to_clean_home(visual, &state);
                let hold = visual.update(|_, app| {
                    state.update(app, |state, _| {
                        state
                            .host()
                            .group(&aria_plugin::Origin::Editor)
                            .expect("open edit group")
                    })
                });
                press_selector(visual, "hero-tile-0");
                drop(hold);
                let err = assert_home_error_drawn(visual, &state, "NewProject");
                press_selector(visual, "hero-tile-0");
                assert_eq!(session(visual, &state), (UiRoute::Editor, None));
                assert_error_cleared(visual, &state, &err, "NewProject");
                assert!(strip_draws(visual, "New project"));
            }
            Pick::PickProject => {
                decided.push("PickProject");
                reset_to_clean_home(visual, &state);
                visual.update(|_, app| {
                    state.update(app, |state, _| {
                        state.supply_pick(Answer::Paths(vec![bad_project.clone()]));
                    });
                });
                press_selector(visual, "hero-tile-1");
                let err = assert_home_error_drawn(visual, &state, "PickProject");
                visual.update(|_, app| {
                    state.update(app, |state, _| {
                        state.supply_pick(Answer::Paths(vec![valid_project_path.clone()]));
                    });
                });
                press_selector(visual, "hero-tile-1");
                assert_eq!(
                    session(visual, &state),
                    (UiRoute::Editor, Some(valid_project_path.clone()))
                );
                assert_error_cleared(visual, &state, &err, "PickProject");
            }
            Pick::OpenRecent(recent_path) => {
                decided.push("OpenRecent");
                std::fs::write(recent_path, b"corrupt recent project").expect("corrupt recent");
                reset_to_clean_home(visual, &state);
                press_card(visual);
                let err = assert_home_error_drawn(visual, &state, "OpenRecent");
                std::fs::write(recent_path, &valid_bytes).expect("restore valid recent");
                press_card(visual);
                assert_eq!(
                    session(visual, &state),
                    (UiRoute::Editor, Some(recent_path.clone()))
                );
                assert_error_cleared(visual, &state, &err, "OpenRecent");
            }
            Pick::RelinkRecent(recent_path) => {
                decided.push("RelinkRecent");
                std::fs::remove_file(recent_path).expect("remove recent file for relink");
                reset_to_clean_home(visual, &state);
                visual.update(|_, app| {
                    state.update(app, |state, _| {
                        state.supply_pick(Answer::Paths(vec![bad_project.clone()]));
                    });
                });
                press_card(visual);
                let err = assert_home_error_drawn(visual, &state, "RelinkRecent");
                std::fs::write(recent_path, &valid_bytes)
                    .expect("restore valid recent after relink");
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        state.remember_recent(recent_path);
                        cx.notify();
                    });
                });
                visual.run_until_parked();
                press_card(visual);
                assert_error_cleared(visual, &state, &err, "RelinkRecent");
            }
            Pick::OpenSample => {
                decided.push("OpenSample");
                reset_to_clean_home(visual, &state);
                let still =
                    visual.read(|app| crate::app::recents::sample_tone(&state.read(app).data_dir));
                std::fs::remove_file(&still).expect("remove sample still file");
                std::fs::create_dir_all(&still).expect("replace sample still with a directory");
                press_selector(visual, "hero-tile-2");
                let err = assert_home_error_drawn(visual, &state, "OpenSample");
                std::fs::remove_dir_all(&still).expect("remove directory at sample still path");
                press_selector(visual, "hero-tile-2");
                assert_eq!(session(visual, &state).0, UiRoute::Editor);
                assert_error_cleared(visual, &state, &err, "OpenSample");
            }
            Pick::UseTemplate => {
                decided.push("UseTemplate");
                reset_to_clean_home(visual, &state);
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        state.set_home_rail(HomeRail::Samples);
                        state.supply_pick(Answer::Paths(vec![bad_template.clone()]));
                        cx.notify();
                    });
                });
                visual.run_until_parked();
                press_selector(visual, "hero-tile-1");
                let err = assert_home_error_drawn(visual, &state, "UseTemplate");
                reset_to_clean_home(visual, &state);
                press_selector(visual, "hero-tile-0");
                assert_error_cleared(visual, &state, &err, "UseTemplate");
            }
            Pick::RestoreRecovery => {
                decided.push("RestoreRecovery");
                reset_to_clean_home(visual, &state);
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        let rec = crate::app::recovery::recovery_path(&state.data_dir);
                        std::fs::write(&rec, b"not valid project json")
                            .expect("write corrupt recovery.aria");
                        state.refresh_pending_recovery();
                        cx.notify();
                    });
                });
                visual.run_until_parked();
                press_text(visual, "Restore");
                let _ = assert_home_error_drawn(visual, &state, "RestoreRecovery");
            }
            Pick::DiscardRecovery => {
                decided.push("DiscardRecovery");
                let prior_err = visual
                    .read(|app| state.read(app).last_error.clone())
                    .expect("RestoreRecovery error still active before DiscardRecovery");
                press_text(visual, "Discard");
                assert_eq!(session(visual, &state), (UiRoute::Home, None));
                assert_error_cleared(visual, &state, &prior_err, "DiscardRecovery");
                assert!(strip_draws(visual, "Discarded the recovered work"));
            }
        }
    }
    assert_eq!(
        decided, declared,
        "every Pick variant in surfaces/home_pick.rs must be decided in order"
    );

    // Command-line open failure through AppState::open_cli_paths (both missing
    // .aria project and missing media file).
    for bad_cli in [
        dir.join("no-such-project.aria"),
        dir.join("no-such-clip.mp4"),
    ] {
        reset_to_clean_home(visual, &state);
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.open_cli_paths(vec![bad_cli.clone()]);
                cx.notify();
            });
        });
        visual.run_until_parked();
        let cli_err = assert_home_error_drawn(visual, &state, "open_cli_paths");
        press_selector(visual, "hero-tile-0");
        assert_error_cleared(visual, &state, &cli_err, "open_cli_paths");
        assert!(strip_draws(visual, "New project"));
    }

    // Every ProjectLoadErrorKind variant must display its user-facing wording
    // across Home open, recents, recovery, drop on Home, and CLI open_cli_paths.
    for kind in aria_models::ProjectLoadErrorKind::ALL {
        let case_path = match kind {
            aria_models::ProjectLoadErrorKind::NotFound => dir.join("kind-missing.aria"),
            aria_models::ProjectLoadErrorKind::Unreadable => {
                let p = dir.join("kind-locked.aria");
                std::fs::write(&p, "{}").expect("write locked file");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000))
                        .expect("chmod 000");
                }
                p
            }
            aria_models::ProjectLoadErrorKind::NotAProject => {
                let p = dir.join("kind-not-project.aria");
                std::fs::write(&p, "not json at all").expect("write non-json");
                p
            }
            aria_models::ProjectLoadErrorKind::NewerSchema => {
                let p = dir.join("kind-newer.aria");
                std::fs::write(
                    &p,
                    r#"{"schema":{"version":99,"kind":"aria.project"},"id":1,"name":"future","metadata":{},"media":[],"timeline":{"frame_rate":{"num":24,"den":1},"tracks":[],"order":[],"clip_index":[]}}"#,
                )
                .expect("write newer schema");
                p
            }
            aria_models::ProjectLoadErrorKind::Damaged => {
                let p = dir.join("kind-damaged.aria");
                std::fs::write(
                    &p,
                    r#"{"schema":{"version":4,"kind":"aria.project"},"id":1,"name":"broken","metadata":{},"media":[],"timeline":{"frame_rate":{"num":"bad","den":1},"tracks":[],"order":[],"clip_index":[]}}"#,
                )
                .expect("write damaged schema");
                p
            }
        };
        let load_err = aria_models::Project::load_from_file(&case_path)
            .expect_err("case file must fail to load");
        assert_eq!(load_err.load_error_kind(), Some(kind));
        let expected_msg = load_err.to_string();

        for surface in ["open", "recent", "recovery", "drop", "cli"] {
            reset_to_clean_home(visual, &state);
            let expected_for_surface = visual.update(|_, app| {
                state.update(app, |state, cx| {
                    let target_path = if surface == "recovery" {
                        let rec = crate::app::recovery::recovery_path(&state.data_dir);
                        #[cfg(unix)]
                        if rec.exists() {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = std::fs::set_permissions(
                                &rec,
                                std::fs::Permissions::from_mode(0o600),
                            );
                        }
                        std::fs::write(&rec, &valid_bytes).expect("seed recovery.aria");
                        state.refresh_pending_recovery();
                        match kind {
                            aria_models::ProjectLoadErrorKind::NotFound => {
                                std::fs::remove_file(&rec).expect("remove recovery.aria");
                            }
                            aria_models::ProjectLoadErrorKind::Unreadable => {
                                #[cfg(unix)]
                                {
                                    use std::os::unix::fs::PermissionsExt;
                                    std::fs::set_permissions(
                                        &rec,
                                        std::fs::Permissions::from_mode(0o000),
                                    )
                                    .expect("chmod 000 recovery.aria");
                                }
                            }
                            _ => {
                                std::fs::copy(&case_path, &rec)
                                    .expect("copy case to recovery.aria");
                            }
                        }
                        rec
                    } else {
                        case_path.clone()
                    };
                    let msg = aria_models::Project::load_from_file(&target_path)
                        .expect_err("target path must fail to load")
                        .to_string();
                    match surface {
                        "open" => {
                            state.supply_pick(Answer::Paths(vec![target_path]));
                            if let Err(err) =
                                crate::surfaces::home_pick::apply(state, Pick::PickProject)
                            {
                                state.report_error(&err);
                            }
                        }
                        "recent" => {
                            if let Err(err) = crate::surfaces::home_pick::apply(
                                state,
                                Pick::OpenRecent(target_path),
                            ) {
                                state.report_error(&err);
                            }
                        }
                        "recovery" => {
                            if let Err(err) =
                                crate::surfaces::home_pick::apply(state, Pick::RestoreRecovery)
                            {
                                state.report_error(&err);
                            }
                        }
                        "drop" => state.request_drop_on_home(vec![target_path]),
                        "cli" => state.open_cli_paths(vec![target_path]),
                        _ => unreachable!(),
                    }
                    cx.notify();
                    msg
                })
            });
            visual.run_until_parked();
            if surface != "recovery" {
                assert_eq!(expected_for_surface, expected_msg);
            }
            assert_eq!(
                visual
                    .read(|app| state.read(app).last_error.clone())
                    .as_deref(),
                Some(expected_for_surface.as_str())
            );
            assert!(
                strip_draws(visual, &expected_for_surface),
                "surface `{surface}` for `{kind:?}` did not draw `{expected_for_surface}` on Home status-strip"
            );
            #[cfg(unix)]
            if surface == "recovery" && kind == aria_models::ProjectLoadErrorKind::Unreadable {
                use std::os::unix::fs::PermissionsExt;
                let rec = visual
                    .read(|app| crate::app::recovery::recovery_path(&state.read(app).data_dir));
                let _ = std::fs::set_permissions(&rec, std::fs::Permissions::from_mode(0o600));
            }
        }
        #[cfg(unix)]
        if kind == aria_models::ProjectLoadErrorKind::Unreadable {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&case_path, std::fs::Permissions::from_mode(0o600));
        }
    }

    // Dropping a plugin package on Home and confirming install must draw the
    // "Installed <name> <version>" confirmation on the Home status-strip.
    reset_to_clean_home(visual, &state);
    let pkg_src = dir.join("looks-pkg-src");
    std::fs::create_dir_all(&pkg_src).expect("create plugin source dir");
    std::fs::write(
        pkg_src.join(aria_plugin::package::MANIFEST_FILE),
        "[plugin]\nid = \"com.example.home.looks\"\nname = \"HomeLooks\"\nversion = \"1.2.0\"\napi = 1\npublisher = \"Example\"\n",
    )
    .expect("write plugin manifest");
    let pkg_archive = dir.join(format!(
        "home-looks-1.2.0.{}",
        aria_plugin::package::ARCHIVE_EXTENSION
    ));
    aria_plugin::package::pack(&pkg_src, &pkg_archive).expect("pack plugin archive");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .drop_on_home(vec![pkg_archive.clone()])
                .expect("drop plugin archive on Home");
            state.confirm_plugin_install();
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert_eq!(
        session(visual, &state),
        (UiRoute::Home, None),
        "installing a plugin archive on Home left Home"
    );
    assert!(
        strip_draws(visual, "Installed HomeLooks 1.2.0"),
        "Home status-strip did not draw 'Installed HomeLooks 1.2.0'"
    );
}
