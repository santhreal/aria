//! The application menu registry: the row type, how a row runs, and the
//! dispatch from each menu to the module holding its rows.
//!
//! Each menu's rows are pure data plus one-line closures in a `menu_<name>.rs`
//! sibling, so the bar's layout and the bar's contents are separate files. `items_for` is one contributor to
//! [`super::menu_host::MenuHost`], which the bar reads — so a menu cannot be drawn
//! from one list and tested from another, and a plugin's item is drawn by the same
//! code.

use std::borrow::Cow;

use aria_engine::EngineError;

use crate::app::AppState;
use crate::types::AppMenu;

/// What a row does, told which row ran it.
///
/// The id is passed rather than closed over because a `fn` pointer captures
/// nothing: without it a whole family of rows generated from one table —
/// every workspace in [`crate::layouts::WORKSPACES`], say — would need a
/// hand-written closure each, which is the cost this registry exists to avoid.
/// A row that does not care names it `_`.
pub type RowRun = fn(&str, &mut AppState, &mut gpui::Context<AppState>);

/// Why the row cannot run against this state, or `None` when it can. The
/// reason is the disabled row's tooltip, and the status line when the row is
/// chosen from the keyboard or the palette anyway.
pub type RowBlocked = fn(&str, &AppState) -> Option<&'static str>;

/// Whether a toggle row draws its check mark.
pub type RowChecked = fn(&str, &AppState) -> bool;

#[derive(Clone, Copy)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: &'static str,
    pub run: RowRun,
    /// The binding whose current chord the row draws right-aligned, by binding
    /// id. Read through the key registry on every draw, so a rebinding shows
    /// in the menu at once.
    pub key: Option<&'static str>,
    pub blocked: RowBlocked,
    /// `Some` for a toggle: the row draws a check mark while this holds.
    pub checked: Option<RowChecked>,
    /// The submenu the row sits in, by its caption, or `None` for a row of the
    /// drop itself. Rows with one caption are drawn together in one flyout, at
    /// the place the first of them sits.
    pub under: Option<&'static str>,
}

impl MenuItem {
    /// A row that can always run, with no chord, no check mark, and no submenu.
    pub const fn new(id: &'static str, label: &'static str, run: RowRun) -> Self {
        Self {
            id,
            label,
            run,
            key: None,
            blocked: super::menu_guards::ready,
            checked: None,
            under: None,
        }
    }

    /// Draw the chord of binding `id`.
    pub const fn key(mut self, id: &'static str) -> Self {
        self.key = Some(id);
        self
    }

    /// Draw the row disabled, with the reason as its tooltip, while `blocked`
    /// names one.
    pub const fn when(mut self, blocked: RowBlocked) -> Self {
        self.blocked = blocked;
        self
    }

    /// Draw a check mark while `checked` holds.
    pub const fn checked(mut self, checked: RowChecked) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Put the row in the submenu captioned `caption`.
    pub const fn under(mut self, caption: &'static str) -> Self {
        self.under = Some(caption);
        self
    }

    /// Why the row cannot run now, or `None` when it can.
    pub fn blocked_by(&self, state: &AppState) -> Option<&'static str> {
        (self.blocked)(self.id, state)
    }

    /// Whether the row draws a check mark now.
    pub fn is_checked(&self, state: &AppState) -> bool {
        self.checked.is_some_and(|checked| checked(self.id, state))
    }

    /// The chord the row draws, as the key registry spells it now.
    pub fn chord(&self, state: &AppState) -> Option<String> {
        state.chord_label(self.key?)
    }
}

/// One entry of an open drop: a row, or a submenu holding the rows that share a
/// caption. Indices are into the drop's item list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropEntry {
    Row(usize),
    Sub {
        caption: &'static str,
        rows: Vec<usize>,
    },
}

impl DropEntry {
    /// The text the entry draws in the drop.
    pub fn caption<'a>(&self, items: &'a [MenuItem]) -> &'a str {
        match self {
            Self::Row(at) => items[*at].label,
            Self::Sub { caption, .. } => caption,
        }
    }
}

/// The entries `items` draw as, in order: a row stands alone, and every row of
/// one submenu caption is gathered where the first of them sits.
pub fn drop_entries(items: &[MenuItem]) -> Vec<DropEntry> {
    let mut entries: Vec<DropEntry> = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let Some(caption) = item.under else {
            entries.push(DropEntry::Row(at));
            continue;
        };
        let held = entries.iter_mut().find_map(|entry| match entry {
            DropEntry::Sub { caption: c, rows } if *c == caption => Some(rows),
            _ => None,
        });
        match held {
            Some(rows) => rows.push(at),
            None => entries.push(DropEntry::Sub {
                caption,
                rows: vec![at],
            }),
        }
    }
    entries
}

/// What the bar draws for this row. Undo and Redo name the step they would
/// apply, and a row with a value names the value it holds.
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
        "view-playback-resolution" => format!(
            "{}: {}",
            item.label,
            state.ui_settings.playback_resolution.label()
        ),
        "view-paused-resolution" => format!(
            "{}: {}",
            item.label,
            state.ui_settings.paused_resolution.label()
        ),
        _ => item.label.to_string(),
    }
}

/// Run `item` unless it is blocked, in which case the reason goes to the status
/// line and nothing runs. Returns whether it ran.
///
/// The one way a row runs: the bar's click, the drop's Enter, the palette and
/// the platform bar all come here, so a row drawn disabled cannot be run from a
/// second surface that forgot to look.
pub(crate) fn press(
    item: &MenuItem,
    state: &mut AppState,
    cx: &mut gpui::Context<AppState>,
) -> bool {
    if let Some(reason) = item.blocked_by(state) {
        state.status_message = Some(reason.to_string());
        return false;
    }
    (item.run)(item.id, state, cx);
    cx.notify();
    true
}

pub type MenuItems = Cow<'static, [MenuItem]>;

pub(crate) fn items_for(menu: AppMenu, state: &AppState) -> MenuItems {
    match menu {
        AppMenu::File => super::menu_file::file_items(state),
        AppMenu::Edit => super::menu_edit::edit_items(state),
        AppMenu::Clip => super::menu_clip::clip_items(state),
        AppMenu::Sequence => super::menu_sequence::sequence_items(state),
        AppMenu::Markers => super::menu_markers::marker_items(state),
        AppMenu::Effect => super::menu_effect::effect_items(state),
        AppMenu::Animation => super::menu_animation::animation_items(state),
        AppMenu::View => super::menu_view_rows::view_items(state),
        AppMenu::Window => super::menu_window::window_items(state),
        AppMenu::Help => super::menu_help::help_items(state),
    }
}

/// Run one menu row by id, through the registry the bar draws from.
///
/// The palette needs this: it offers every menu row by name, and a row it offers
/// must run the same code the bar runs. Reports whether the id was found, so a
/// row that vanished between the offer and the keypress is a no-op rather than a
/// silent wrong action. A row found blocked is not run; [`press`] reports why.
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
    press(item, state, cx);
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

/// Report a failed edit on the status line.
pub(super) fn report<T>(state: &mut AppState, result: Result<T, EngineError>) {
    if let Err(err) = result {
        state.report_error(&err);
    }
}

#[cfg(test)]
#[path = "menu_items_tests.rs"]
mod tests;
