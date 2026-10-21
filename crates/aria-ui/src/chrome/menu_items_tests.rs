//! Every menu row has a unique id and a label, and Undo and Redo name their
//! step.

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

/// Labels are unique *within* each list a menu draws: two identical rows in
/// one dropdown or one flyout are indistinguishable on screen. A
/// flyout's caption is a row of the dropdown it opens from. Across lists a
/// repeat is legitimate (Home appears under File and Window; the Color
/// workspace sits in the Workspaces flyout beside the Color panel's row).
#[test]
fn labels_are_unique_within_each_menu() {
    let state = state();
    for menu in AppMenu::ALL {
        let items = items_for(menu, &state);
        let mut lists: std::collections::BTreeMap<Option<&str>, Vec<&str>> =
            std::collections::BTreeMap::new();
        for item in items.iter() {
            lists.entry(item.under).or_default().push(item.label);
        }
        let captions: Vec<&str> = lists.keys().flatten().copied().collect();
        lists.entry(None).or_default().extend(captions);
        for (list, labels) in lists {
            let unique: BTreeSet<&str> = labels.iter().copied().collect();
            assert_eq!(
                unique.len(),
                labels.len(),
                "duplicate label in {menu:?} {list:?}: {labels:?}"
            );
        }
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
