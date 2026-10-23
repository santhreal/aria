use super::*;

/// Window holds one row per registered dock panel, checked while the panel is
/// open. The panels are read from the running registry, so a panel added to the
/// built-in list or declared by a plugin is covered with no edit here; a panel
/// listed as having a hand-written row that is no longer registered, or a
/// generated row that does not open and close its panel, turns this red.
#[test]
fn every_registered_dock_panel_has_a_window_row_checked_while_open() {
    use crate::chrome::menu_window::{PANEL_PREFIX, PANEL_ROWS};
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
    use crate::chrome::menu_window::{PANEL_PREFIX, PANEL_ROWS};
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

/// Every shipped workspace restores the whole arrangement it states, whichever
/// workspace was applied before it. The comparison is of the whole
/// [`PaneLayout`], so a field added to it is covered with no edit here, and
/// every ordered pair is walked, so state one workspace sets and another
/// leaves alone (the graph editor followed the Animation workspace into all the
/// others) turns this red.
///
/// Not covered: state outside `PaneLayout`, such as the Effects & Presets
/// filter, which has its own case.
#[test]
fn every_shipped_workspace_arranges_the_panes_it_names_after_any_other() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    for before in &layouts::WORKSPACES {
        for ws in &layouts::WORKSPACES {
            assert!(apply(&subject, &cx, before.id), "{} was refused", before.id);
            assert!(apply(&subject, &cx, ws.id), "{} was refused", ws.id);
            assert_eq!(
                panes_of(&subject, &cx),
                ws.panes,
                "{} after {} did not restore its arrangement",
                ws.id,
                before.id
            );
            assert_eq!(
                cx.update(|app| active_id(&subject, app)),
                Some(ws.id),
                "{} did not read back as the active workspace",
                ws.id
            );
        }
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
        graph_open: true,
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
            state.toggle_timeline_graph_editor();
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
    // rather than silently restoring the default arrangement.
    assert!(!apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(panes_of(&subject, &cx), before);
}

/// The current screen goes with the arrangement, through the guarded verbs:
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

/// A click lands on the menu row, so the row is what is driven here: the
/// closure has to reach the verb with its own id.
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
