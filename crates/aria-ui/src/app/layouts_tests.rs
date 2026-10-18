//! What a workspace restores, measured against the running state.
//!
//! Every case drives the real verbs through a `gpui` entity, because applying a
//! workspace persists settings and asks the window to redraw — a test that called
//! the fields directly would prove nothing about the row the operator clicks.
//!
//! The subjects are read out of `layouts::WORKSPACES` at run time, so a fifth
//! shipped arrangement is covered by these cases without an edit here, and one
//! that cannot be applied turns them red.

use aria_plugin::PluginContext;
use gpui::AppContext;

use super::AppState;
use crate::chrome::menu_items::items_of;
use crate::layouts::{self, CUSTOM_ID, CUSTOM_NAME, PaneLayout, SAVE_ID};
use crate::plugins::UiPlugin;
use crate::settings::{SIDEBAR_MAX, TIMELINE_MIN_HEIGHT, UiSettings};
use crate::types::{ActivePanel, AppMenu, UiRoute};

fn dir() -> std::path::PathBuf {
    crate::scratch_pool::scratch("layouts")
}

/// Mounts the ui plugin, so the menu the cases read is the registry the bar
/// reads rather than a list assembled for the test.
fn state_in(dir: &std::path::Path) -> AppState {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
    AppState::new(ctx, dir.to_path_buf()).expect("state")
}

/// Run one closure against the state through an entity, and hand the state back
/// as an entity so a case can act more than once.
fn entity(state: AppState, cx: &gpui::TestAppContext) -> gpui::Entity<AppState> {
    cx.update(|app| app.new(|_| state))
}

fn apply(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext, id: &str) -> bool {
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            state.apply_workspace(id, cx)
        })
    })
}

fn panes_of(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> PaneLayout {
    cx.update(|app| entity.read(app).panes())
}

fn window_labels(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> Vec<&'static str> {
    cx.update(|app| {
        items_of(AppMenu::Window, entity.read(app))
            .into_iter()
            .map(|item| item.label)
            .collect()
    })
}

fn window_ids(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> Vec<&'static str> {
    cx.update(|app| {
        items_of(AppMenu::Window, entity.read(app))
            .into_iter()
            .map(|item| item.id)
            .collect()
    })
}

/// Window holds one row per registered dock panel, checked while the panel is
/// open. The panels are read from the running registry, so a panel added to the
/// built-in list or declared by a plugin is covered with no edit here; a panel
/// listed as having a hand-written row that is no longer registered, or a
/// generated row that does not open and close its panel, turns this red.
#[test]
fn every_registered_dock_panel_has_a_window_row_checked_while_open() {
    use crate::chrome::menu_items::{PANEL_PREFIX, PANEL_ROWS};
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let panels = cx.update(|app| subject.read(app).registered_panel_ids());
    for (_, panel, _) in PANEL_ROWS {
        assert!(
            panels.contains(panel),
            "{panel} is listed with a hand-written row but is not registered"
        );
    }
    let generated: Vec<_> = panels
        .iter()
        .copied()
        .filter(|panel| !PANEL_ROWS.iter().any(|(_, listed, _)| listed == panel))
        .collect();
    for shipped in [
        crate::dock::PANEL_CHARACTER,
        crate::dock::PANEL_PARAGRAPH,
        crate::dock::PANEL_AUDIO_METERS,
    ] {
        assert!(generated.contains(&shipped), "{shipped} has no Window row");
    }
    for panel in generated {
        let row_id = format!("{PANEL_PREFIX}{panel}");
        let press = || {
            cx.update(|app| {
                subject.update(app, |state: &mut AppState, cx| {
                    let row = items_of(AppMenu::Window, state)
                        .into_iter()
                        .find(|item| item.id == row_id)
                        .unwrap_or_else(|| panic!("no Window row for {panel}"));
                    assert_eq!(row.label, state.dock_panel_title(panel), "{panel}");
                    (row.run)(row.id, state, cx);
                    let checked = row.checked.expect("a panel row is checkable");
                    (state.dock_tree.is_open(panel), checked(row.id, state))
                })
            })
        };
        let was_open = cx.update(|app| subject.read(app).dock_tree.is_open(panel));
        assert_eq!(press(), (!was_open, !was_open), "{panel}: the first press");
        assert_eq!(press(), (was_open, was_open), "{panel}: the second press");
    }
}

/// The hand-written Window rows are labeled with their panel's title and
/// checked while their panel is open, front tab or not, as the generated rows
/// are. The rows are read from the live menu,
/// so a top-level toggle that is neither generated nor listed in `PANEL_ROWS`
/// turns this red until its panel is recorded. Each row is pressed with its
/// panel closed, which opens the panel and checks the row; the panel is then
/// put behind another tab of its frame, and the row stays checked.
///
/// Not covered: what a press on an open panel does. Project, Effect Controls and
/// Jobs close it; the others bring it forward, and that is checked where each verb
/// is defined.
#[test]
fn every_hand_written_panel_row_is_checked_while_its_panel_is_open() {
    use crate::chrome::menu_items::{PANEL_PREFIX, PANEL_ROWS};
    use crate::dock::{PANEL_HISTORY, PANEL_MARKERS};
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let rows: Vec<&'static str> = cx.update(|app| {
        items_of(AppMenu::Window, subject.read(app))
            .into_iter()
            .filter(|item| {
                item.checked.is_some() && item.under.is_none() && !item.id.starts_with(PANEL_PREFIX)
            })
            .map(|item| item.id)
            .collect()
    });
    let listed: Vec<&str> = PANEL_ROWS.iter().map(|&(row, ..)| row).collect();
    assert_eq!(rows, listed, "the Window panel rows and PANEL_ROWS differ");
    for &(row_id, panel, _) in PANEL_ROWS {
        let seen = cx.update(|app| {
            subject.update(app, |state: &mut AppState, cx| {
                let row = items_of(AppMenu::Window, state)
                    .into_iter()
                    .find(|item| item.id == row_id)
                    .expect("listed above");
                assert_eq!(row.label, state.dock_panel_title(panel), "{row_id}");
                let checked = row.checked.expect("a panel row is checkable");
                let mut seen = Vec::new();
                state.dock_tree.close(panel);
                seen.push((state.dock_tree.is_open(panel), checked(row.id, state)));
                (row.run)(row.id, state, cx);
                seen.push((state.dock_tree.is_open(panel), checked(row.id, state)));
                let frame = state.dock_tree.find(panel).expect("the press opened it");
                let other = if panel == PANEL_HISTORY {
                    PANEL_MARKERS
                } else {
                    PANEL_HISTORY
                };
                assert!(state.dock_tree.add_tab(frame, other), "{row_id}");
                seen.push((state.dock_tree.is_active(panel), checked(row.id, state)));
                seen
            })
        });
        assert_eq!(
            seen,
            [(false, false), (true, true), (false, true)],
            "{row_id}: closed, pressed, behind another tab"
        );
    }
}

/// Each Shift+digit panel in the keyboard spec comes forward under the name the
/// spec gives it. The class closed: a panel named one way by its tab and Window
/// row and another by the spec, the shortcut list and the manual, as Media,
/// Inspector, Preview and Mixer were for Project, Effect Controls, Viewer and
/// Audio Track Mixer. The rows are read from the spec's Panels table at run
/// time, so a panel added to it is checked with no edit here.
///
/// Not covered: Shift+0, which the spec lists under no panel, and panels with
/// no Shift+digit binding, whose names the spec gives only in prose.
#[test]
fn each_shift_digit_panel_comes_forward_under_its_spec_name() {
    use crate::keys::bindings::SHIPPED_BINDINGS;
    use crate::keys::{KeyBody, KeyRoute};
    const SPEC: &str = include_str!("../../../../docs/spec/keyboard.md");
    let rows: Vec<(&str, &str)> = SPEC
        .split("\n## ")
        .find(|section| section.starts_with("Panels\n"))
        .expect("the keyboard spec has a Panels section")
        .lines()
        .filter_map(|line| {
            let mut cells = line.split('|').map(str::trim).skip(1);
            let key = cells.next()?;
            let digit = key.strip_prefix("Shift+")?;
            if digit.len() != 1 || !digit.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let title = cells.next()?.split(';').next()?.trim();
            Some((key, title))
        })
        .collect();
    let keys: Vec<String> = (1..=9).map(|digit| format!("Shift+{digit}")).collect();
    assert_eq!(
        rows.iter().map(|&(key, _)| key).collect::<Vec<_>>(),
        keys,
        "the spec's Panels table reads {rows:?}"
    );
    for (key, title) in rows {
        let mut state = state_in(&dir());
        let binding = SHIPPED_BINDINGS
            .iter()
            .find(|binding| {
                matches!(binding.route, KeyRoute::Editor)
                    && binding.chord.label().eq_ignore_ascii_case(key)
            })
            .unwrap_or_else(|| panic!("{key} is bound in the editor"));
        let KeyBody::Plain(run) = binding.run else {
            panic!("{} runs on the state alone", binding.id);
        };
        run(&mut state);
        let front: Vec<&str> = state
            .registered_panel_ids()
            .into_iter()
            .filter(|&panel| state.dock_tree.is_active(panel))
            .map(|panel| state.dock_panel_title(panel))
            .collect();
        assert!(
            front.contains(&title),
            "{key} left {front:?} in front, and none of them is {title}"
        );
    }
}

#[test]
fn every_shipped_workspace_arranges_the_panes_it_names() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    for ws in &layouts::WORKSPACES {
        assert!(apply(&subject, &cx, ws.id), "{} was refused", ws.id);
        let here = panes_of(&subject, &cx);
        assert_eq!(here.sidebar_width, ws.panes.sidebar_width, "{}", ws.id);
        assert_eq!(
            here.sidebar_collapsed, ws.panes.sidebar_collapsed,
            "{}",
            ws.id
        );
        assert_eq!(here.inspector_width, ws.panes.inspector_width, "{}", ws.id);
        assert_eq!(
            here.inspector_collapsed, ws.panes.inspector_collapsed,
            "{}",
            ws.id
        );
        assert_eq!(here.timeline_height, ws.panes.timeline_height, "{}", ws.id);
        assert_eq!(here.scopes_open, ws.panes.scopes_open, "{}", ws.id);
        assert_eq!(here.panel, ws.panes.panel, "{}", ws.id);
        assert_eq!(here.route, ws.panes.route, "{}", ws.id);
        assert_eq!(
            cx.update(|app| active_id(&subject, app)),
            Some(ws.id),
            "{} did not read back as the active workspace",
            ws.id
        );
    }
}

/// The active row as the Settings list marks it: the list the UI draws is the
/// only place activeness is decided, so a case must not compute it a second way.
fn active_id(subject: &gpui::Entity<AppState>, app: &mut gpui::App) -> Option<&'static str> {
    subject
        .read(app)
        .workspace_rows()
        .into_iter()
        .find(|row| row.active)
        .map(|row| row.id)
}

/// The rows the Window menu checks under Workspaces, read from the live menu:
/// every checkable Window row whose id is a workspace row's id.
fn checked_workspace_rows(state: &AppState) -> Vec<&'static str> {
    let rows: Vec<&str> = state.workspace_rows().iter().map(|row| row.id).collect();
    items_of(AppMenu::Window, state)
        .into_iter()
        .filter(|item| rows.contains(&item.id))
        .inspect(|item| assert!(item.checked.is_some(), "{} is not checkable", item.id))
        .filter(|item| item.is_checked(state))
        .map(|item| item.id)
        .collect()
}

/// The mark is the workspace last chosen, in Settings and in the Window menu
/// alike, and only that one. A workspace remembers the edits made to it, so a
/// dragged divider is part of it: the drag is stored under the workspace and the
/// mark stays. A fresh session marks the first shipped row, and the Custom slot
/// is marked once it is chosen. The workspaces are read from
/// `layouts::WORKSPACES`, so a new one is covered with no edit here.
///
/// Not covered: plugin rows, which `a_plugin_workspace_is_offered_applied_and_withdrawn`
/// marks, and the drawn accent, which reads the same `active` field.
#[test]
fn the_chosen_workspace_stays_marked_through_a_dragged_divider() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let first = layouts::WORKSPACES[0].id;
    let marks = |app: &mut gpui::App| {
        let settings = active_id(&subject, app);
        (settings, checked_workspace_rows(subject.read(app)))
    };
    assert_eq!(
        cx.update(marks),
        (Some(first), vec![first]),
        "a fresh session"
    );
    for ws in &layouts::WORKSPACES {
        assert!(apply(&subject, &cx, ws.id));
        assert_eq!(cx.update(marks), (Some(ws.id), vec![ws.id]), "{}", ws.id);
        let dragged = cx.update(|app| {
            subject.update(app, |state: &mut AppState, cx| {
                let before = state.dock_tree.split_weights(&[]);
                let moved = state.drag_split_clamped(&[], 0, 0.05, 2000.0)
                    || state.drag_split_clamped(&[], 0, -0.05, 2000.0);
                assert!(moved, "{}: the root divider did not move", ws.id);
                assert_ne!(state.dock_tree.split_weights(&[]), before, "{}", ws.id);
                state.record_dock_edit(cx);
                state.dock_tree.clone()
            })
        });
        assert_eq!(
            cx.update(marks),
            (Some(ws.id), vec![ws.id]),
            "{}: the drag cleared the mark",
            ws.id
        );
        assert_eq!(
            cx.update(|app| subject
                .read(app)
                .ui_settings
                .workspace_trees
                .get(ws.id)
                .cloned()),
            Some(dragged),
            "{}: the workspace did not remember the drag",
            ws.id
        );
    }
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| state.save_workspace(cx));
    });
    assert!(apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(
        cx.update(marks),
        (Some(CUSTOM_ID), vec![CUSTOM_ID]),
        "the saved slot"
    );
}

/// Aiming the keyboard at another pane is not an arrangement: after a click on
/// any panel the workspace it was in is still the marked one.
#[test]
fn focusing_a_pane_keeps_the_workspace_active() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let editing = layouts::WORKSPACES[0].id;
    assert!(apply(&subject, &cx, editing));
    for pane in ActivePanel::ALL {
        cx.update(|app| {
            subject.update(app, |state: &mut AppState, _| state.focus_pane(pane));
        });
        assert_eq!(
            cx.update(|app| active_id(&subject, app)),
            Some(editing),
            "{pane:?}"
        );
    }
}

/// A workspace with the Scopes tab in front measures, and switching to one that
/// shows no scope stops measuring and drops the reading. The workspace is the
/// one whose shipped tree puts the Scopes tab in front, read at run time.
#[test]
fn a_workspace_that_closes_the_scopes_drops_the_reading_with_them() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let colour = layouts::WORKSPACES
        .iter()
        .find(|ws| {
            layouts::workspace_tree(ws.id, &[])
                .is_some_and(|tree| tree.is_active(crate::dock::PANEL_SCOPES))
        })
        .expect("one shipped workspace puts the Scopes tab in front");
    assert!(apply(&subject, &cx, colour.id));
    assert!(cx.update(|app| subject.read(app).scopes_wanted()));
    let editing = layouts::WORKSPACES[0].id;
    assert!(apply(&subject, &cx, editing));
    assert!(cx.update(|app| {
        let state = subject.read(app);
        !state.scopes_wanted() && state.scope_textures.is_none()
    }));
}

#[test]
fn the_saved_slot_is_offered_only_once_it_holds_something() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    assert!(!window_ids(&subject, &cx).contains(&CUSTOM_ID));
    assert!(window_ids(&subject, &cx).contains(&SAVE_ID));
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.save_workspace(cx)));
    assert!(window_labels(&subject, &cx).contains(&CUSTOM_NAME));
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.forget_workspace(cx)));
    assert!(!window_ids(&subject, &cx).contains(&CUSTOM_ID));
}

/// Capture and apply must cover the same fields: an arrangement that is saved and
/// restored has to come back exactly, including the pane the keyboard was aimed
/// at and the screen it was saved on.
#[test]
fn a_saved_arrangement_comes_back_whole_after_a_relaunch() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    let wanted = PaneLayout {
        sidebar_width: 311.0,
        sidebar_collapsed: true,
        inspector_width: 433.0,
        inspector_collapsed: true,
        timeline_height: 377.0,
        scopes_open: false,
        panel: ActivePanel::Inspector,
        route: UiRoute::Home,
    };
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state.ui_settings.sidebar_width = wanted.sidebar_width;
            state.ui_settings.sidebar_collapsed = wanted.sidebar_collapsed;
            state.ui_settings.inspector_width = wanted.inspector_width;
            state.ui_settings.inspector_collapsed = wanted.inspector_collapsed;
            state.ui_settings.timeline_height = wanted.timeline_height;
            state.active_panel = wanted.panel;
            state.save_workspace(cx);
            state.ui_settings.save(&state.data_dir).expect("save");
        })
    });
    // A second launch against the same dir: the slot has to survive the file, not
    // just the session.
    let relaunched = entity(state_in(&scratch), &cx);
    assert!(apply(&relaunched, &cx, layouts::WORKSPACES[0].id));
    assert!(apply(&relaunched, &cx, CUSTOM_ID));
    let here = panes_of(&relaunched, &cx);
    assert_eq!(
        here, wanted,
        "the saved arrangement did not come back whole"
    );
}

#[test]
fn a_saved_slot_from_a_wilder_build_is_pulled_inside_the_pane_bounds() {
    let scratch = dir();
    let settings = UiSettings {
        workspace: Some(PaneLayout {
            sidebar_width: 9000.0,
            timeline_height: 1.0,
            ..PaneLayout::default()
        }),
        ..UiSettings::default()
    };
    settings.save(&scratch).expect("save");
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    assert!(apply(&subject, &cx, CUSTOM_ID));
    let here = panes_of(&subject, &cx);
    assert_eq!(here.sidebar_width, SIDEBAR_MAX);
    assert_eq!(here.timeline_height, TIMELINE_MIN_HEIGHT);
}

#[test]
fn a_workspace_nobody_ships_is_refused_and_changes_nothing() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let before = panes_of(&subject, &cx);
    assert!(!apply(&subject, &cx, "win-workspace-nothing"));
    // The slot is empty, so its row is not on offer and applying it is refused
    // rather than restoring the default arrangement under the operator.
    assert!(!apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(panes_of(&subject, &cx), before);
}

/// Where the operator is goes with the arrangement, through the guarded verbs:
/// a slot saved on the start screen restores the start screen.
#[test]
fn an_arrangement_saved_at_home_takes_the_operator_home() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.save_workspace(cx)));
    assert_eq!(panes_of(&subject, &cx).route, UiRoute::Home);
    assert!(apply(&subject, &cx, layouts::WORKSPACES[0].id));
    assert_eq!(cx.update(|app| subject.read(app).route), UiRoute::Editor);
    assert!(apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(cx.update(|app| subject.read(app).route), UiRoute::Home);
}

/// The Window menu draws the table, in the table's order, and nothing else.
#[test]
fn the_menu_family_is_the_workspace_table() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let ids = window_ids(&subject, &cx);
    let shipped: Vec<&str> = layouts::WORKSPACES.iter().map(|ws| ws.id).collect();
    let drawn: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| id.starts_with("win-workspace-") && *id != SAVE_ID && *id != layouts::RESET_ID)
        .collect();
    assert_eq!(drawn, shipped, "menu order must follow the table");
    let labels = window_labels(&subject, &cx);
    for ws in &layouts::WORKSPACES {
        assert!(labels.contains(&ws.name), "{} is not drawn", ws.name);
    }
}

/// Clicking the menu row is what an operator does, so the row is what is driven
/// here: the closure has to reach the verb with its own id.
#[test]
fn the_menu_row_arranges_the_panes_it_names() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let wanted = layouts::WORKSPACES
        .iter()
        .find(|ws| ws.panes.sidebar_collapsed)
        .expect("one shipped workspace collapses the rail");
    let ran = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, wanted.id, state, cx)
        })
    });
    assert!(ran, "{} did not run", wanted.id);
    assert!(panes_of(&subject, &cx).sidebar_collapsed);
    let saved = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, SAVE_ID, state, cx)
        })
    });
    assert!(saved);
    assert_eq!(
        cx.update(|app| subject.read(app).ui_settings.workspace),
        Some(panes_of(&subject, &cx)),
        "Save Workspace must keep exactly what is on screen"
    );
}

/// A plugin workspace with every field set, sizes inside the pane bounds.
fn grading() -> aria_plugin::WorkspaceSpec {
    aria_plugin::WorkspaceSpec {
        id: "grading".into(),
        name: "Grading Room".into(),
        panes: aria_plugin::WorkspacePanes {
            sidebar_width: Some(260.0),
            sidebar_collapsed: Some(true),
            inspector_width: Some(520.0),
            inspector_collapsed: Some(false),
            timeline_height: Some(230.0),
            scopes: Some(true),
            focus: Some(aria_plugin::WorkspaceFocus::Preview),
            ..aria_plugin::WorkspacePanes::default()
        },
    }
}

/// A plugin workspace is a Window-menu row and a Settings row after the
/// shipped ones, applies the arrangement it registered, reads as active once
/// applied, and leaves the menu when its registration drops.
#[test]
fn a_plugin_workspace_is_offered_applied_and_withdrawn() {
    let cx = gpui::TestAppContext::single();
    let state = state_in(&dir());
    let ctx = std::sync::Arc::clone(&state.ctx);
    let subject = entity(state, &cx);
    let held = ctx.register_workspace(grading()).expect("registers");
    let id = format!("{}/grading", layouts::PLUGIN_PREFIX);

    let ids = window_ids(&subject, &cx);
    let at = ids
        .iter()
        .position(|row| *row == id)
        .expect("the Window menu has no row for the plugin workspace");
    let last_shipped = ids
        .iter()
        .position(|row| *row == layouts::WORKSPACES[layouts::WORKSPACES.len() - 1].id)
        .expect("shipped rows");
    assert!(at > last_shipped, "plugin rows follow the shipped ones");
    assert!(window_labels(&subject, &cx).contains(&"Grading Room"));
    let row = cx.update(|app| {
        subject
            .read(app)
            .workspace_rows()
            .into_iter()
            .find(|row| row.id == id)
            .map(|row| (row.plugin, row.active))
    });
    assert_eq!(row, Some((Some("Host"), false)));

    assert!(apply(&subject, &cx, &id));
    let here = panes_of(&subject, &cx);
    let wanted = PaneLayout {
        sidebar_width: 260.0,
        sidebar_collapsed: true,
        inspector_width: 520.0,
        inspector_collapsed: false,
        timeline_height: 230.0,
        scopes_open: true,
        panel: ActivePanel::Preview,
        route: here.route,
    };
    assert_eq!(here, wanted);
    let active = cx.update(|app| {
        subject
            .read(app)
            .workspace_rows()
            .into_iter()
            .filter(|row| row.active)
            .map(|row| row.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(active, [id.as_str()]);

    drop(held);
    assert!(!window_ids(&subject, &cx).contains(&id.as_str()));
    assert!(
        !apply(&subject, &cx, &id),
        "a withdrawn workspace still applied"
    );
}

/// A field a plugin leaves unset takes the Editing arrangement, and a size
/// outside the pane bounds is pulled inside them.
#[test]
fn a_plugin_workspace_fills_unset_fields_and_clamps_sizes() {
    let editing = PaneLayout::default();
    let unset = layouts::plugin_panes(&aria_plugin::WorkspacePanes::default());
    assert_eq!(
        unset,
        PaneLayout {
            route: UiRoute::Editor,
            ..editing
        }
    );
    let wild = layouts::plugin_panes(&aria_plugin::WorkspacePanes {
        sidebar_width: Some(9000.0),
        timeline_height: Some(1.0),
        ..aria_plugin::WorkspacePanes::default()
    });
    assert_eq!(wild.sidebar_width, SIDEBAR_MAX);
    assert_eq!(wild.timeline_height, TIMELINE_MIN_HEIGHT);
}

/// Every focus a plugin can name lands on a distinct pane, so a new focus
/// variant that maps onto an existing pane turns this red.
#[test]
fn every_plugin_focus_names_its_own_pane() {
    use aria_plugin::WorkspaceFocus;
    let all = [
        WorkspaceFocus::Media,
        WorkspaceFocus::Preview,
        WorkspaceFocus::Timeline,
        WorkspaceFocus::Inspector,
        WorkspaceFocus::Assistant,
        WorkspaceFocus::Mixer,
    ];
    let panels: Vec<ActivePanel> = all
        .iter()
        .map(|focus| {
            layouts::plugin_panes(&aria_plugin::WorkspacePanes {
                focus: Some(*focus),
                ..aria_plugin::WorkspacePanes::default()
            })
            .panel
        })
        .collect();
    for panel in ActivePanel::ALL {
        assert_eq!(
            panels.iter().filter(|p| **p == panel).count(),
            1,
            "{panel:?} is not the focus of exactly one plugin focus"
        );
    }
}

#[test]
fn all_five_shipped_workspaces_produce_valid_dock_trees_and_remember_edits_until_reset() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let names: Vec<&str> = layouts::WORKSPACES.iter().map(|ws| ws.name).collect();
    assert_eq!(
        names,
        vec!["Editing", "Animation", "Color", "Audio", "Effects"]
    );
    for ws in &layouts::WORKSPACES {
        assert!(apply(&subject, &cx, ws.id));
        cx.update(|app| {
            subject
                .read(app)
                .dock_tree
                .check_invariants()
                .unwrap_or_else(|e| panic!("{} broke dock invariants: {e}", ws.id));
        });
    }

    // Switch to Editing, close the Inspector tab (`FrameId::RIGHT`), switch to
    // Colour, and switch back to Editing: the remembered edit persists until
    // Reset to Saved Layout (`RESET_ID`) restores the shipped Editing tree.
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            assert!(state.close_dock_panel(crate::dock::PANEL_INSPECTOR, cx));
        });
    });
    assert!(cx.update(|app| {
        !subject
            .read(app)
            .dock_tree
            .is_open(crate::dock::PANEL_INSPECTOR)
    }));
    assert!(apply(&subject, &cx, "win-workspace-colour"));
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    assert!(
        cx.update(|app| {
            !subject
                .read(app)
                .dock_tree
                .is_open(crate::dock::PANEL_INSPECTOR)
        }),
        "switching back to Editing must restore its remembered dock edits"
    );

    let reset_ran = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, layouts::RESET_ID, state, cx)
        })
    });
    assert!(reset_ran);
    assert!(
        cx.update(|app| {
            subject
                .read(app)
                .dock_tree
                .is_open(crate::dock::PANEL_INSPECTOR)
        }),
        "Reset to Saved Layout must restore the shipped Editing tree"
    );
}

#[test]
fn save_as_new_workspace_writes_file_round_trips_and_rejects_stale_version() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    assert!(apply(&subject, &cx, "win-workspace-editing"));

    let path = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state.dock_tree.dock(
                crate::dock::PANEL_MIXER,
                crate::dock::FrameId::RIGHT,
                crate::dock::Zone::Bottom,
            );
            state.record_dock_edit(cx);
            state
                .save_workspace_as("Dialogue Cut", cx)
                .expect("save named workspace")
        })
    });
    assert_eq!(path, scratch.join("workspaces/Dialogue Cut.json"));
    assert!(path.exists());
    let expected_tree = cx.update(|app| subject.read(app).dock_tree.clone());

    // Switch away, then load the named workspace back from disk.
    assert!(apply(&subject, &cx, "win-workspace-colour"));
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state
                .load_named_workspace("Dialogue Cut", cx)
                .expect("load named workspace");
        });
    });
    assert_eq!(
        cx.update(|app| subject.read(app).dock_tree.clone()),
        expected_tree
    );

    // Export to an external path and import into a fresh session.
    let export_path = scratch.join("exported-dialogue.json");
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            state
                .export_workspace(&export_path)
                .expect("export workspace");
        });
    });
    let fresh = entity(state_in(&scratch.join("fresh-profile")), &cx);
    let imported_name = cx.update(|app| {
        fresh.update(app, |state: &mut AppState, cx| {
            state
                .import_workspace(&export_path, cx)
                .expect("import workspace")
        })
    });
    assert_eq!(imported_name, "Dialogue Cut");
    assert_eq!(
        cx.update(|app| fresh.read(app).dock_tree.clone()),
        expected_tree
    );

    // A stale version in the JSON is rejected and records the error via `report_error`.
    let raw = std::fs::read_to_string(&export_path).expect("read exported json");
    let stale = raw
        .replace("\"version\": 2", "\"version\": 1")
        .replace("\"version\":2", "\"version\":1");
    let stale_path = scratch.join("stale-workspace.json");
    std::fs::write(&stale_path, stale).expect("write stale json");
    let err = cx.update(|app| {
        fresh.update(app, |state: &mut AppState, cx| {
            state.import_workspace(&stale_path, cx)
        })
    });
    assert!(err.is_err());
    assert!(
        cx.update(|app| fresh.read(app).last_error.is_some()),
        "rejecting a stale workspace file must report the error"
    );
}

#[test]
fn plugin_workspace_with_declared_dock_tree_applies_and_audio_workspace_filters_effects() {
    let cx = gpui::TestAppContext::single();
    let state = state_in(&dir());
    let ctx = std::sync::Arc::clone(&state.ctx);
    let subject = entity(state, &cx);

    // Audio workspace filters the Effects browser to Audio Effects; switching
    // to another workspace clears the filter.
    assert!(apply(&subject, &cx, "win-workspace-audio"));
    assert_eq!(
        cx.update(|app| subject.read(app).effects_browser.only),
        Some(crate::app::effects_browser::Group::AudioEffects)
    );
    assert!(apply(&subject, &cx, "win-workspace-editing"));
    assert_eq!(
        cx.update(|app| subject.read(app).effects_browser.only),
        None
    );

    // Plugin workspace with an explicit `DockTree` JSON in `WorkspacePanes::tree`.
    let mut custom_tree = crate::dock::DockTree::editing();
    custom_tree.float(crate::dock::PANEL_MIXER, (100.0, 100.0, 500.0, 320.0));
    let tree_json = custom_tree.to_json().expect("tree json");
    let _held = ctx
        .register_workspace(aria_plugin::WorkspaceSpec {
            id: "float-mix".into(),
            name: "Floating Mix".into(),
            panes: aria_plugin::WorkspacePanes {
                tree: Some(tree_json),
                ..aria_plugin::WorkspacePanes::default()
            },
        })
        .expect("register plugin workspace with tree");
    let plugin_id = format!("{}/float-mix", layouts::PLUGIN_PREFIX);
    assert!(apply(&subject, &cx, &plugin_id));
    assert_eq!(
        cx.update(|app| subject.read(app).dock_tree.floating.len()),
        1
    );
}

#[test]
fn animation_workspace_opens_timeline_graph_and_workspaces_scale_spec_px_at_1920_and_1280() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);

    assert!(!cx.update(|app| subject.read(app).timeline_graph_open()));
    assert!(apply(&subject, &cx, "win-workspace-animation"));
    assert!(
        cx.update(|app| subject.read(app).timeline_graph_open()),
        "Animation workspace must open the timeline graph editor"
    );

    for width in [1920.0_f32, 1280.0_f32] {
        cx.update(|app| {
            subject.update(app, |state: &mut AppState, cx| {
                state.observe_main_window_size(width, 900.0);
                assert!(state.apply_workspace("win-workspace-editing", cx));
                let crate::dock::Node::Split { children: rows, .. } = &state.dock_tree.root else {
                    panic!("expected root column split");
                };
                let crate::dock::Node::Split {
                    children: upper, ..
                } = &rows[0].1
                else {
                    panic!("expected upper row split");
                };
                assert!(
                    (upper[0].0 * width - 320.0).abs() < 0.5,
                    "left frame at {width}px was {}px",
                    upper[0].0 * width
                );
                assert!(
                    (upper[2].0 * width - 300.0).abs() < 0.5,
                    "right frame at {width}px was {}px",
                    upper[2].0 * width
                );
                let crate::dock::Node::Split {
                    children: lower, ..
                } = &rows[1].1
                else {
                    panic!("expected lower row split");
                };
                assert!(
                    (lower[1].0 * width - 64.0).abs() < 0.5,
                    "Audio Meters at {width}px was {}px",
                    lower[1].0 * width
                );
            });
        });
    }
}

/// A panel id nothing registers: a plugin that declared it and was then removed.
const UNREGISTERED: crate::dock::PanelId = "aria.panel.unregistered-test";

#[test]
fn unregistered_panels_are_filtered_and_all_unregistered_frame_is_omitted_from_drawn_layout() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| {
        let state = subject.read(app);
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_EFFECTS));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_AUDIO_METERS));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_CHARACTER));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_PARAGRAPH));
        assert!(state.is_dock_panel_registered(crate::dock::PANEL_JOBS));
        assert!(!state.is_dock_panel_registered(UNREGISTERED));

        let right = state
            .dock_tree
            .frame(crate::dock::FrameId::RIGHT)
            .expect("RIGHT frame");
        assert_eq!(right.tabs, vec![crate::dock::PANEL_EFFECTS]);
        assert!(state.node_has_registered_panel(&crate::dock::Node::Frame(right.clone())));

        let only_unregistered = crate::dock::Node::Frame(crate::dock::Frame::new(
            crate::dock::FrameId(99),
            vec![UNREGISTERED],
        ));
        assert!(
            !state.node_has_registered_panel(&only_unregistered),
            "a frame whose every tab is unregistered must be omitted from the drawn layout"
        );
    });
}

#[test]
fn restore_split_weights_restores_workspace_saved_ratio_and_min_sizes_clamp_drag_and_resize() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    assert!(apply(&subject, &cx, "win-workspace-editing"));

    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            assert!(state.dock_tree.drag_split_at(&[], 0, 0.20));
            assert!(state.restore_split_weights(&[], cx));
            let crate::dock::Node::Split { children, .. } = &state.dock_tree.root else {
                panic!("expected root split");
            };
            assert!((children[0].0 - 0.55).abs() < 1e-4);
            assert!((children[1].0 - 0.45).abs() < 1e-4);

            // Dragging upper row splitter 0 left cannot shrink the Left frame below 160 px at 1000 px width.
            assert!(state.drag_split_clamped(&[0], 0, -0.50, 1000.0));
            let crate::dock::Node::Split { children: rows, .. } = &state.dock_tree.root else {
                panic!("expected root split");
            };
            let crate::dock::Node::Split {
                children: upper, ..
            } = &rows[0].1
            else {
                panic!("expected upper row split");
            };
            assert!(
                upper[0].0 * 1000.0 >= 160.0 - 0.5,
                "Left frame shrank below 160px minimum: {}",
                upper[0].0 * 1000.0
            );

            // Window resize to 840 px clamps Left and Right frames up to >= 160 px and takes delta from Viewer slack.
            state.dock_tree = crate::dock::DockTree::editing_for_width(1920.0);
            assert!(state.rebalance_splits_for_window(840.0, 600.0));
            let crate::dock::Node::Split {
                children: rows2, ..
            } = &state.dock_tree.root
            else {
                panic!("expected root split");
            };
            let crate::dock::Node::Split {
                children: upper2, ..
            } = &rows2[0].1
            else {
                panic!("expected upper row split");
            };
            assert!(upper2[0].0 * 840.0 >= 160.0 - 0.5);
            assert!(upper2[2].0 * 840.0 >= 160.0 - 0.5);
            assert!(upper2[1].0 * 840.0 >= 320.0 - 0.5);
        });
    });
}

#[test]
fn float_open_failure_redocks_frame_and_save_workspace_menu_prompts_for_name() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);

    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            let fid = state
                .float_dock_panel(crate::dock::PANEL_MIXER, (80.0, 80.0, 480.0, 320.0), cx)
                .expect("float mixer");
            assert_eq!(state.dock_tree.floating.len(), 1);
            assert!(state.handle_float_open_failure(fid, "display server refused window", cx));
            assert!(state.dock_tree.floating.is_empty());
            assert!(state.dock_tree.is_open(crate::dock::PANEL_MIXER));
            assert!(
                state
                    .last_error
                    .as_deref()
                    .is_some_and(|msg| msg.contains("display server refused window"))
            );

            // Window > Workspaces > Save as New Workspace opens RenameTarget::SaveWorkspace,
            // and committing the typed name writes `<data dir>/workspaces/<name>.json`.
            assert!(crate::chrome::menu_items::run_item(
                AppMenu::Window,
                layouts::SAVE_ID,
                state,
                cx,
            ));
            assert_eq!(
                state.rename,
                Some(crate::types::RenameTarget::SaveWorkspace)
            );
            for ch in "Evening Cut".chars() {
                state.rename_push(ch);
            }
            state.commit_rename().expect("commit workspace name");
        });
    });
    assert!(scratch.join("workspaces/Evening Cut.json").exists());
}

#[test]
fn six_deep_nested_splitter_and_half_window_nested_split_span_and_window_placement() {
    use crate::dock::{
        Axis, DOCK_VERSION, DockTree, Frame, FrameId, Node, PANEL_ASSISTANT, PANEL_AUDIO_METERS,
        PANEL_EFFECTS, PANEL_INSPECTOR, PANEL_MIXER, PANEL_PROJECT, PANEL_TIMELINE, PANEL_VIEWER,
    };
    use crate::settings::{UiSettings, WindowPlacement};

    // Build a DockTree nested 6 splits deep: path `[0, 1, 0, 1, 0, 1]`.
    let deep_split = Node::Split {
        axis: Axis::Row,
        children: vec![
            (
                0.5,
                Node::Frame(Frame::new(FrameId(10), vec![PANEL_PROJECT])),
            ),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(11), vec![PANEL_EFFECTS])),
            ),
        ],
    };
    let d5 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (
                0.5,
                Node::Frame(Frame::new(FrameId(9), vec![PANEL_INSPECTOR])),
            ),
            (0.5, deep_split),
        ],
    };
    let d4 = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d5),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(8), vec![PANEL_ASSISTANT])),
            ),
        ],
    };
    let d3 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (0.5, Node::Frame(Frame::new(FrameId(7), vec![PANEL_MIXER]))),
            (0.5, d4),
        ],
    };
    let d2 = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d3),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(6), vec![PANEL_AUDIO_METERS])),
            ),
        ],
    };
    let d1 = Node::Split {
        axis: Axis::Column,
        children: vec![
            (0.5, Node::Frame(Frame::new(FrameId(5), vec![PANEL_VIEWER]))),
            (0.5, d2),
        ],
    };
    let root = Node::Split {
        axis: Axis::Row,
        children: vec![
            (0.5, d1),
            (
                0.5,
                Node::Frame(Frame::new(FrameId(4), vec![PANEL_TIMELINE])),
            ),
        ],
    };

    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            state.dock_tree = DockTree {
                version: DOCK_VERSION,
                root,
                floating: Vec::new(),
                maximized: None,
                focused: FrameId(10),
            };
            let deep_path = [0usize, 1, 0, 1, 0, 1];
            crate::shell::dock_view::set_active_split_drag_path(&deep_path);
            let recovered = crate::shell::dock_view::active_split_drag_path(&deep_path[..4]);
            assert_eq!(recovered, deep_path);

            // Drag the 6-deep splitter: only the 6-deep split's weights change;
            // ancestor splits at depth <= 4 remain untouched at [0.5, 0.5].
            assert!(state.drag_split_clamped(&recovered, 0, 0.15, 1000.0));
            assert_eq!(
                state.dock_tree.split_weights(&[0, 1, 0, 1]),
                Some(vec![0.5, 0.5])
            );
            let leaf_w = state
                .dock_tree
                .split_weights(&deep_path)
                .expect("6-deep weights");
            assert!(
                (leaf_w[0] - 0.65).abs() < 1e-4,
                "6-deep weight[0]={}",
                leaf_w[0]
            );
            assert!(
                (leaf_w[1] - 0.35).abs() < 1e-4,
                "6-deep weight[1]={}",
                leaf_w[1]
            );

            // Defect 2: a nested Row split that occupies half the window (`[0]` has weight 0.5
            // in a Row root of width 1200 px -> container is 600 px wide). Dragging its
            // splitter by N = 60 px moves the boundary by N = 60 px (weight delta = 60 / 600 = 0.10).
            state.dock_tree = DockTree {
                version: DOCK_VERSION,
                root: Node::Split {
                    axis: Axis::Row,
                    children: vec![
                        (
                            0.5,
                            Node::Split {
                                axis: Axis::Row,
                                children: vec![
                                    (
                                        0.5,
                                        Node::Frame(Frame::new(FrameId::LEFT, vec![PANEL_PROJECT])),
                                    ),
                                    (
                                        0.5,
                                        Node::Frame(Frame::new(
                                            FrameId::RIGHT,
                                            vec![PANEL_EFFECTS],
                                        )),
                                    ),
                                ],
                            },
                        ),
                        (
                            0.5,
                            Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
                        ),
                    ],
                },
                floating: Vec::new(),
                maximized: None,
                focused: FrameId::LEFT,
            };
            assert!((state.dock_tree.split_span_fraction(&[0], Axis::Row) - 0.5).abs() < 1e-5);
            assert!(state.drag_split_by_pixels(&[0], 0, 60.0, 1200.0));
            let nested_w = state.dock_tree.split_weights(&[0]).expect("nested weights");
            let boundary_px_before = 0.5 * 600.0;
            let boundary_px_after = nested_w[0] * 600.0;
            assert!(
                ((boundary_px_after - boundary_px_before) - 60.0).abs() < 0.5,
                "dragging by 60 px in a half-window split must move boundary by 60 px, got {}",
                boundary_px_after - boundary_px_before
            );
        });
    });

    // WindowPlacement per display arrangement (`docs/spec/layout.md:24-26`):
    // First launch opens maximized on primary display; saved placement restores
    // size, position, display, and maximized state per display arrangement.
    let mut settings = UiSettings::default();
    let dual = [[0.0, 0.0, 1920.0, 1080.0], [1920.0, 0.0, 2560.0, 1440.0]];
    let first = settings.resolve_window_placement(&dual);
    assert!(first.maximized);
    assert_eq!(first.display_index, 0);

    settings.record_window_placement(
        &dual,
        WindowPlacement {
            x: 2040.0,
            y: 120.0,
            width: 1600.0,
            height: 960.0,
            maximized: false,
            display_index: 1,
        },
    );
    let restored = settings.resolve_window_placement(&dual);
    assert_eq!(restored.display_index, 1);
    assert!(!restored.maximized);
    assert_eq!(
        (restored.x, restored.y, restored.width, restored.height),
        (2040.0, 120.0, 1600.0, 960.0)
    );

    // Different display arrangement (single monitor) opens maximized on primary display
    // while preserving the dual-monitor arrangement's saved entry.
    let single = [[0.0, 0.0, 1920.0, 1080.0]];
    let single_resolved = settings.resolve_window_placement(&single);
    assert!(single_resolved.maximized);
    assert_eq!(single_resolved.display_index, 0);
    assert_eq!(settings.resolve_window_placement(&dual).display_index, 1);
}
