//! The application menu tables: what each of the six menus offers, and what each
//! entry runs.
//!
//! Pure data plus one-line closures, moved out of `chrome.rs` so the bar's layout
//! and the bar's contents are separate files. `items_for` is one contributor to
//! [`super::menu_host::MenuHost`], which the bar reads — so a menu cannot be drawn
//! from one list and tested from another, and a plugin's item is drawn by the same
//! code.

use std::borrow::Cow;

use crate::app::{AppState, DialogKind};
use crate::types::{AppMenu, HomeRail};

#[derive(Clone, Copy)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: &'static str,
    /// What the row does, told which row ran it.
    ///
    /// The id is passed rather than closed over because a `fn` pointer captures
    /// nothing: without it a whole family of rows generated from one table —
    /// every workspace in [`crate::layouts::WORKSPACES`], say — would need a
    /// hand-written closure each, which is the cost this registry exists to
    /// avoid. A row that does not care names it `_`.
    pub run: fn(&str, &mut AppState, &mut gpui::Context<AppState>),
}

/// What the bar draws for this row. Undo and Redo name the step they would apply.
pub(crate) fn row_caption(item: &MenuItem, state: &AppState) -> String {
    match item.id {
        "edit-undo" => match state.next_undo_label() {
            Some(step) => format!("Undo {step}"),
            None => item.label.to_string(),
        },
        "edit-redo" => match state.next_redo_label() {
            Some(step) => format!("Redo {step}"),
            None => item.label.to_string(),
        },
        _ => item.label.to_string(),
    }
}

pub type MenuItems = Cow<'static, [MenuItem]>;

pub(crate) fn items_for(menu: AppMenu, state: &AppState) -> MenuItems {
    match menu {
        AppMenu::File => file_items(state),
        AppMenu::Edit => edit_items(state),
        AppMenu::Sequence => super::menu_sequence::sequence_items(state),
        AppMenu::View => view_items(state),
        AppMenu::Window => window_items(state),
        AppMenu::Help => help_items(state),
    }
}

/// Run one menu row by id, through the registry the bar draws from.
///
/// The palette needs this: it offers every menu row by name, and a row it offers
/// must run the same code the bar runs. Reports whether the id was found, so a
/// row that vanished between the offer and the keypress is a no-op rather than a
/// silent wrong action.
pub(crate) fn run_item(
    menu: AppMenu,
    id: &str,
    state: &mut AppState,
    cx: &mut gpui::Context<AppState>,
) -> bool {
    let Some(host) = super::menu_host::host_of(&state.ctx) else {
        return false;
    };
    let items = host.items(menu, state);
    let Some(item) = items.iter().find(|it| it.id == id) else {
        return false;
    };
    (item.run)(item.id, state, cx);
    true
}

/// What the bar would draw for one menu, as items.
///
/// Goes through the registry, because that is what the bar reads. A fixture built
/// without the ui plugin has no host, so the shipped contributor is registered on
/// the spot: the assertion stays about the menu, not about which plugins the
/// fixture mounted.
#[cfg(test)]
pub(crate) fn items_of(menu: AppMenu, state: &AppState) -> Vec<MenuItem> {
    let host = super::menu_host::host_of(&state.ctx).unwrap_or_else(|| {
        let host = std::sync::Arc::new(super::menu_host::MenuHost::default());
        host.add(items_for);
        host
    });
    host.items(menu, state).into_owned()
}

/// The labels of the same items, for assertions about a menu's contents.
#[cfg(test)]
pub(crate) fn labels_for(menu: AppMenu, state: &AppState) -> Vec<&'static str> {
    items_of(menu, state)
        .into_iter()
        .map(|item| item.label)
        .collect()
}

static FILE_ITEMS: &[MenuItem] = &[
    MenuItem {
        id: "file-new",
        label: "New",
        run: |_, s, _| s.request_new_project(),
    },
    MenuItem {
        id: "file-open",
        label: "Open…",
        run: |_, s, _| s.request_pick_project(),
    },
    MenuItem {
        id: "file-home",
        label: "Open Recent / Home",
        run: |_, s, _| s.request_go_home(),
    },
    MenuItem {
        id: "file-save",
        label: "Save",
        run: |_, s, _| {
            if let Err(err) = s.save_project() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "file-save-as",
        label: "Save As…",
        run: |_, s, _| s.save_project_as(),
    },
    MenuItem {
        id: "file-save-template",
        label: "Save as Template…",
        run: |_, s, _| s.pick_and_save_template(),
    },
    MenuItem {
        id: "file-use-template",
        label: "Use Template…",
        run: |_, s, _| s.pick_and_apply_template(),
    },
    MenuItem {
        id: "file-rename",
        label: "Rename Project…",
        run: |_, s, _| s.begin_rename(crate::types::RenameTarget::Project),
    },
    MenuItem {
        id: "file-import",
        label: "Import…",
        run: |_, s, _| s.pick_and_import(),
    },
    MenuItem {
        id: "file-relink-all",
        label: "Relink All…",
        run: |_, s, _| s.relink_all(),
    },
    MenuItem {
        id: "file-export",
        label: "Export…",
        run: |_, s, _| s.open_dialog(DialogKind::Export),
    },
    MenuItem {
        id: "file-close",
        label: "Close",
        run: |_, s, _| s.request_go_home(),
    },
];

pub(super) fn file_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(FILE_ITEMS)
}

static EDIT_ITEMS: &[MenuItem] = &[
    MenuItem {
        id: "edit-undo",
        label: "Undo",
        run: |_, s, _| {
            if !s.undo() {
                s.status_message = Some("Nothing to undo".into());
            }
        },
    },
    MenuItem {
        id: "edit-redo",
        label: "Redo",
        run: |_, s, _| {
            if !s.redo() {
                s.status_message = Some("Nothing to redo".into());
            }
        },
    },
    MenuItem {
        id: "edit-cut",
        label: "Cut",
        run: |_, s, _| {
            s.copy_selected();
            if let Err(err) = s.delete_selected() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-copy",
        label: "Copy",
        run: |_, s, _| s.copy_selected(),
    },
    MenuItem {
        id: "edit-paste",
        label: "Paste",
        run: |_, s, _| {
            if let Err(err) = s.paste_clipboard() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-paste-insert",
        label: "Paste Insert",
        run: |_, s, _| {
            if let Err(err) = s.paste_insert() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-paste-attributes",
        label: "Paste Attributes",
        run: |_, s, _| {
            if let Err(err) = s.paste_attributes() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-paste-fit",
        label: "Paste To Fit",
        run: |_, s, _| {
            if let Err(err) = s.paste_to_fit() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-dup",
        label: "Duplicate",
        run: |_, s, _| {
            if let Err(err) = s.duplicate_selected() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-split",
        label: "Split",
        run: |_, s, _| {
            if let Err(err) = s.split_at_playhead() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-ripple",
        label: "Ripple Delete",
        run: |_, s, _| {
            if let Err(err) = s.delete_selected() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "edit-select-all",
        label: "Select All",
        run: |_, s, _| s.select_all_unlocked(),
    },
    MenuItem {
        id: "edit-deselect",
        label: "Deselect",
        run: |_, s, _| s.clear_selection(),
    },
];

pub(super) fn edit_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(EDIT_ITEMS)
}

static VIEW_ITEMS: &[MenuItem] = &[
    MenuItem {
        id: "view-sidebar",
        label: "Sidebar",
        run: |_, s, _| s.ui_settings.sidebar_collapsed = !s.ui_settings.sidebar_collapsed,
    },
    MenuItem {
        id: "view-inspector",
        label: "Inspector",
        run: |_, s, _| s.ui_settings.inspector_collapsed = !s.ui_settings.inspector_collapsed,
    },
    MenuItem {
        id: "view-ai",
        label: "Assistant",
        run: |_, s, _| {
            s.active_panel = s.active_panel.toggle_to(crate::types::ActivePanel::Ai);
        },
    },
    MenuItem {
        id: "view-mixer",
        label: "Mixer",
        run: |_, s, _| {
            s.active_panel = s.active_panel.toggle_to(crate::types::ActivePanel::Mixer);
        },
    },
    MenuItem {
        id: "view-fullscreen",
        label: "Fullscreen Preview",
        run: |_, s, _| s.toggle_preview_fullscreen(),
    },
    MenuItem {
        id: "view-title-safe",
        label: "Title Safe",
        run: |_, s, _| s.toggle_title_safe(),
    },
    MenuItem {
        id: "view-action-safe",
        label: "Action Safe",
        run: |_, s, _| s.toggle_action_safe(),
    },
    MenuItem {
        id: "view-grid",
        label: "Grid",
        run: |_, s, _| s.toggle_preview_grid(),
    },
    MenuItem {
        id: "view-scopes",
        label: "Scopes",
        run: |_, s, _| s.toggle_scopes(),
    },
    MenuItem {
        id: "view-zoom-fit",
        label: "Fit Preview",
        run: |_, s, _| s.preview_zoom_fit(),
    },
    MenuItem {
        id: "view-zoom-actual",
        label: "Preview 100%",
        run: |_, s, _| s.preview_zoom_actual(),
    },
    MenuItem {
        id: "view-handles",
        label: "Cycle Canvas Handles",
        run: |_, s, _| s.cycle_overlay_mode(),
    },
    MenuItem {
        id: "view-settings",
        label: "Appearance / Settings",
        run: |_, s, _| s.open_dialog(DialogKind::Settings),
    },
];

pub(super) fn view_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(VIEW_ITEMS)
}

pub(super) fn window_items(state: &AppState) -> MenuItems {
    let mut items = vec![
        MenuItem {
            id: "win-home",
            label: "Home",
            run: |_, s, _| s.request_go_home(),
        },
        MenuItem {
            id: "win-palette",
            label: "Command Palette",
            run: |_, s, _| s.open_dialog(DialogKind::Palette),
        },
        MenuItem {
            id: "win-jobs",
            label: "Jobs",
            run: |_, s, _| s.open_dialog(DialogKind::Jobs),
        },
        MenuItem {
            id: "win-history",
            label: "History",
            run: |_, s, _| s.open_dialog(DialogKind::History),
        },
    ];
    // A report on a project with no sources and no clips is a column of zeroes,
    // so the item is offered once there is something to report on.
    if state.project_report_available() {
        items.push(MenuItem {
            id: "win-project-report",
            label: "Project Report",
            run: |_, s, _| s.open_dialog(DialogKind::ProjectReport),
        });
    }
    // One row per workspace, generated from the table that defines them, so a
    // fifth arrangement is a row in `crate::layouts` and nothing here. Each row
    // is told its own id, which is the workspace's id.
    for ws in &crate::layouts::WORKSPACES {
        items.push(MenuItem {
            id: ws.id,
            label: ws.name,
            run: |id, s, cx| {
                s.apply_workspace(id, cx);
            },
        });
    }
    // The saved slot is offered only once there is something saved in it: a
    // Custom row that restores the default arrangement says nothing.
    if state.ui_settings.workspace.is_some() {
        items.push(MenuItem {
            id: crate::layouts::CUSTOM_ID,
            label: crate::layouts::CUSTOM_NAME,
            run: |id, s, cx| {
                s.apply_workspace(id, cx);
            },
        });
    }
    items.push(MenuItem {
        id: crate::layouts::SAVE_ID,
        label: "Save Workspace",
        run: |_, s, cx| s.save_workspace(cx),
    });
    Cow::Owned(items)
}

static HELP_ITEMS: &[MenuItem] = &[
    MenuItem {
        id: "help-learn",
        label: "Keyboard shortcuts",
        run: |_, s, _| {
            s.set_home_rail(HomeRail::Learn);
            s.request_go_home();
        },
    },
    MenuItem {
        id: "help-data",
        label: "Open data dir",
        run: |_, s, _| crate::app::recents::reveal_in_folder(&s.data_dir),
    },
    MenuItem {
        id: "help-updates",
        label: "Check for updates",
        run: |_, s, cx| s.check_for_updates(cx),
    },
];

pub(super) fn help_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(HELP_ITEMS)
}

#[cfg(test)]
mod tests {
    use super::{MenuItem, items_for};
    use crate::app::AppState;
    use crate::types::AppMenu;
    use aria_plugin::PluginContext;
    use std::collections::BTreeSet;

    fn state() -> AppState {
        let dir = crate::scratch_pool::scratch("menu");
        AppState::new(PluginContext::new(), dir).unwrap()
    }

    fn all_items(state: &AppState) -> Vec<MenuItem> {
        AppMenu::ALL
            .into_iter()
            .flat_map(|menu| items_for(menu, state).into_owned())
            .collect()
    }

    /// Every menu is enumerated from `AppMenu::ALL` at run time, so adding a menu
    /// or an entry without a unique namespaced id turns this red. A duplicate id
    /// collides in GPUI's element identity, which shows up as the wrong row
    /// responding to a click.
    #[test]
    fn every_menu_entry_has_a_unique_id() {
        let state = state();
        let items = all_items(&state);
        assert!(!items.is_empty());
        let ids: BTreeSet<&str> = items.iter().map(|item| item.id).collect();
        assert_eq!(ids.len(), items.len(), "duplicate menu entry id");
    }

    /// Each menu populates at least one entry, and every entry is labelled. An
    /// empty menu draws as a bare rectangle with nothing in it.
    #[test]
    fn every_menu_is_populated_and_labelled() {
        let state = state();
        for menu in AppMenu::ALL {
            let items = items_for(menu, &state);
            assert!(!items.is_empty(), "{menu:?} has no entries");
            for item in items.iter() {
                assert!(!item.label.trim().is_empty(), "{} has no label", item.id);
                assert_eq!(
                    item.label.trim(),
                    item.label,
                    "{} has padded whitespace in its label",
                    item.id
                );
            }
        }
    }

    /// Labels are unique *within* a menu: two identical rows in one dropdown are
    /// indistinguishable to the operator. Across menus a repeat is legitimate
    /// (Home appears under File and Window).
    #[test]
    fn labels_are_unique_within_each_menu() {
        let state = state();
        for menu in AppMenu::ALL {
            let items = items_for(menu, &state);
            let labels: BTreeSet<&str> = items.iter().map(|item| item.label).collect();
            assert_eq!(labels.len(), items.len(), "duplicate label in {menu:?}");
        }
    }

    #[test]
    fn undo_and_redo_rows_name_the_pending_step() {
        let mut state = state();
        let undo = items_for(AppMenu::Edit, &state)
            .into_owned()
            .into_iter()
            .find(|item| item.id == "edit-undo")
            .expect("undo row");
        let redo = items_for(AppMenu::Edit, &state)
            .into_owned()
            .into_iter()
            .find(|item| item.id == "edit-redo")
            .expect("redo row");
        assert_eq!(super::row_caption(&undo, &state), "Undo");
        assert_eq!(super::row_caption(&redo, &state), "Redo");
        state.add_marker_at_playhead().expect("a marker lands");
        let step = state.next_undo_label().expect("named step");
        assert_eq!(super::row_caption(&undo, &state), format!("Undo {step}"));
        assert!(state.undo());
        assert_eq!(super::row_caption(&redo, &state), format!("Redo {step}"));
    }
}
