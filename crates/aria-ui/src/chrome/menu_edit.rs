//! The Edit menu: history, the clipboard, deletion, selection, labels, and
//! the shortcut and preference editors.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use aria_models::LabelColor;

use crate::app::AppState;

use super::menu_items::{MenuItem, MenuItems, report};

/// The id prefix of an Edit > Label row. The rest of the id is the colour's
/// name in lowercase, or `none`.
const LABEL_PREFIX: &str = "edit-label-";

static EDIT_ITEMS: &[MenuItem] = &[
    MenuItem::new("edit-undo", "Undo", |_, s, _| {
        s.undo();
    })
    .key("key-undo")
    .when(super::menu_guards::undo),
    MenuItem::new("edit-redo", "Redo", |_, s, _| {
        s.redo();
    })
    .key("key-redo")
    .when(super::menu_guards::redo),
    MenuItem::new("edit-cut", "Cut", |_, s, _| {
        let cut = s.cut_selected();
        report(s, cut)
    })
    .key("key-cut")
    .when(super::menu_guards::selection),
    MenuItem::new("edit-copy", "Copy", |_, s, _| s.copy_selected())
        .key("key-copy")
        .when(super::menu_guards::selection),
    MenuItem::new("edit-paste", "Paste", |_, s, _| {
        let pasted = s.paste_clipboard();
        report(s, pasted)
    })
    .key("key-paste")
    .when(super::menu_guards::clipboard),
    MenuItem::new("edit-paste-insert", "Paste Insert", |_, s, _| {
        let pasted = s.paste_insert();
        report(s, pasted)
    })
    .key("key-paste-insert")
    .when(super::menu_guards::clipboard),
    MenuItem::new("edit-paste-attributes", "Paste Attributes", |_, s, _| {
        let pasted = s.paste_attributes();
        report(s, pasted)
    })
    .key("key-paste-attributes")
    .when(super::menu_guards::clipboard),
    MenuItem::new(
        "edit-remove-attributes",
        "Remove Attributes…",
        |_, s, _| s.open_remove_attributes(),
    )
    .when(super::menu_guards::selection),
    MenuItem::new("edit-paste-fit", "Paste to Fit", |_, s, _| {
        let pasted = s.paste_to_fit();
        report(s, pasted)
    })
    .when(super::menu_guards::clipboard),
    MenuItem::new("edit-clear", "Clear", |_, s, _| {
        let cleared = s.clear_selected();
        report(s, cleared)
    })
    .key("key-delete")
    .when(super::menu_guards::selection),
    MenuItem::new("edit-ripple", "Ripple Delete", |_, s, _| {
        let deleted = s.ripple_delete_selected();
        report(s, deleted)
    })
    .key("key-ripple-delete")
    .when(super::menu_guards::selection),
    MenuItem::new("edit-dup", "Duplicate", |_, s, _| {
        let duplicated = s.duplicate_selected();
        report(s, duplicated)
    })
    .key("key-duplicate")
    .when(super::menu_guards::selection),
    MenuItem::new("edit-select-all", "Select All", |_, s, _| {
        s.select_all_unlocked()
    })
    .key("key-select-all"),
    MenuItem::new("edit-deselect", "Deselect All", |_, s, _| {
        s.clear_selection()
    })
    .key("key-deselect-all")
    .when(super::menu_guards::selection),
    MenuItem::new("edit-find", "Find…", |_, s, _| s.find_in_project()).key("key-find"),
];

static EDIT_TAIL: &[MenuItem] = &[
    MenuItem::new(
        "edit-keyboard-shortcuts",
        "Keyboard Shortcuts…",
        |_, s, _| s.open_keyboard_shortcuts(),
    )
    .key("key-keyboard-shortcuts"),
    MenuItem::new("edit-preferences", "Preferences…", |_, s, _| {
        s.open_settings_at("")
    })
    .key("key-settings"),
];

/// The Label row for `color`, or for no colour.
fn label_row(color: Option<LabelColor>) -> MenuItem {
    let (id, name) = match color {
        Some(color) => (
            crate::interned::intern(&format!("{LABEL_PREFIX}{}", color.id())),
            color.name(),
        ),
        None => ("edit-label-none", "None"),
    };
    MenuItem::new(id, name, |id, s, _| {
        let color = label_of(id);
        let labelled = s.set_selection_label(color);
        report(s, labelled)
    })
    .when(super::menu_guards::selection)
    .checked(|id, s| {
        s.selected_ui_clip_ref()
            .is_some_and(|clip| clip.label == label_of(id))
    })
    .under("Label")
}

/// The colour an Edit > Label row sets, by its id.
fn label_of(id: &str) -> Option<LabelColor> {
    let name = id.strip_prefix(LABEL_PREFIX)?;
    LabelColor::ALL.into_iter().find(|color| color.id() == name)
}

/// The Edit menu, built once: its Label rows come from the model's colours,
/// which are fixed for the process.
static EDIT_MENU: std::sync::LazyLock<Vec<MenuItem>> = std::sync::LazyLock::new(|| {
    let mut items =
        Vec::with_capacity(EDIT_ITEMS.len() + LabelColor::ALL.len() + 1 + EDIT_TAIL.len());
    items.extend_from_slice(EDIT_ITEMS);
    items.extend(
        LabelColor::ALL
            .into_iter()
            .map(|color| label_row(Some(color))),
    );
    items.push(label_row(None));
    items.extend_from_slice(EDIT_TAIL);
    items
});

pub(super) fn edit_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(EDIT_MENU.as_slice())
}
