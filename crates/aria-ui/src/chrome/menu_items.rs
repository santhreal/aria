//! The application menu tables: what each menu offers, and what each entry
//! runs.
//!
//! Pure data plus one-line closures, moved out of `chrome.rs` so the bar's layout
//! and the bar's contents are separate files. `items_for` is one contributor to
//! [`super::menu_host::MenuHost`], which the bar reads — so a menu cannot be drawn
//! from one list and tested from another, and a plugin's item is drawn by the same
//! code.

use std::borrow::Cow;

use aria_engine::EngineError;
use aria_models::LabelColor;

use crate::app::audio_channels::AudioChannels;
use crate::app::viewer_compare::{CompareLayout, Reference};
use crate::app::{AppState, DialogKind};
use crate::scopes::WaveformMode;
use crate::timeline_display::{Follow, Thumbnails, WaveformScale};
use crate::types::{ActivePanel, AppMenu, HomeRail, OverlayMode};

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
        AppMenu::File => file_items(state),
        AppMenu::Edit => edit_items(state),
        AppMenu::Clip => clip_items(state),
        AppMenu::Sequence => super::menu_sequence::sequence_items(state),
        AppMenu::Markers => super::menu_markers::marker_items(state),
        AppMenu::Effect => super::menu_effect::effect_items(state),
        AppMenu::Animation => super::menu_animation::animation_items(state),
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

/// The id prefix of an Open Recent row. The rest of the id is the project's path.
pub(crate) const RECENT_PREFIX: &str = "file-recent:";
/// The id prefix of an Export row for one timeline exporter. The rest of the id
/// is the exporter's id.
pub(crate) const EXPORT_PREFIX: &str = "file-export-as:";
/// The id prefix of an Edit > Label row. The rest of the id is the colour's
/// name in lowercase, or `none`.
pub(crate) const LABEL_PREFIX: &str = "edit-label-";
/// The id prefix of an Import Recent row. The rest of the id is the file's
/// path.
pub(crate) const IMPORT_RECENT_PREFIX: &str = "file-import-recent:";
/// The id prefix of a Clip > Analyze row for one registered clip analyzer.
pub(crate) const CLIP_ANALYZE_PREFIX: &str = "clip-analyze:";

const OPEN_RECENT: &str = "Open Recent";
const EXPORT: &str = "Export";
const IMPORT_RECENT: &str = "Import Recent";
const AUDIO_CHANNELS: &str = "Audio Channels";
const COMPARE: &str = "Compare";
const CANVAS_HANDLES: &str = "Canvas Handles";

const ANALYZE: &str = "Analyze";
/// Report a failed edit on the status line.
fn report(state: &mut AppState, result: Result<(), EngineError>) {
    if let Err(err) = result {
        state.report_error(&err);
    }
}

/// Clip > Audio Channels > the choice row `id` picks.
fn run_audio_channels(id: &str, state: &mut AppState, _: &mut gpui::Context<AppState>) {
    let Some(choice) = AudioChannels::of_row(id) else {
        return;
    };
    let set = state.set_audio_channels(choice);
    report(state, set)
}

/// Whether the selection plays the choice row `id` picks.
fn audio_channels_checked(id: &str, state: &AppState) -> bool {
    AudioChannels::of_row(id).is_some_and(|choice| state.selection_audio_channels() == Some(choice))
}

static FILE_OPEN: &[MenuItem] = &[
    MenuItem::new("file-new", "New Project", |_, s, _| s.request_new_project()).key("key-new"),
    MenuItem::new("file-new-sequence", "New Sequence", |_, s, _| {
        crate::commands::reach::new_sequence(s)
    })
    .key("key-new-sequence"),
    MenuItem::new("file-open", "Open…", |_, s, _| s.request_pick_project()).key("key-open"),
    MenuItem::new("file-use-template", "Use Template…", |_, s, _| {
        s.pick_and_apply_template()
    }),
];

static FILE_SAVE: &[MenuItem] = &[
    MenuItem::new("file-home", "All Recent Projects…", |_, s, _| {
        s.request_go_home()
    })
    .under(OPEN_RECENT),
    MenuItem::new("file-close", "Close", |_, s, _| s.close_project()).key("key-close"),
    MenuItem::new("file-rename", "Rename Project…", |_, s, _| {
        s.begin_rename(crate::types::RenameTarget::Project)
    }),
    MenuItem::new("file-save", "Save", |_, s, _| {
        let saved = s.save_project();
        report(s, saved)
    })
    .key("key-save"),
    MenuItem::new("file-save-as", "Save As…", |_, s, _| s.save_project_as()).key("key-save-as"),
    MenuItem::new("file-save-copy", "Save a Copy…", |_, s, _| {
        s.save_project_copy()
    })
    .key("key-save-copy"),
    MenuItem::new("file-save-template", "Save as Template…", |_, s, _| {
        s.pick_and_save_template()
    }),
    MenuItem::new("file-revert", "Revert", |_, s, _| s.revert_project())
        .when(super::menu_guards::revertible),
    MenuItem::new("file-import", "Import…", |_, s, _| s.pick_and_import()).key("key-import"),
];

static FILE_IMPORT_CLEAR: MenuItem = MenuItem::new(
    "file-import-recent-clear",
    "Clear Recent Imports",
    |_, s, _| s.clear_import_recents(),
)
.when(super::menu_guards::import_recents)
.under(IMPORT_RECENT);

static FILE_IMPORT: &[MenuItem] = &[
    MenuItem::new("file-import-timeline", "Import Timeline…", |_, s, _| {
        s.pick_and_import_timeline()
    }),
    MenuItem::new("file-relink-all", "Relink All…", |_, s, _| s.relink_all()),
    MenuItem::new("file-export", "Media…", |_, s, _| {
        s.open_dialog(DialogKind::Export)
    })
    .key("key-export")
    .under(EXPORT),
    MenuItem::new("file-export-frame", "Frame…", |_, s, _| {
        s.pick_and_export_frame()
    })
    .key("key-export-frame")
    .under(EXPORT),
];

static FILE_CLOSE: &[MenuItem] = &[
    MenuItem::new("file-export-timeline", "Timeline…", |_, s, _| {
        s.pick_and_export_timeline()
    })
    .when(super::menu_guards::exporters)
    .under(EXPORT),
    MenuItem::new("file-install-plugin", "Install Plugin…", |_, s, _| {
        s.pick_plugin_archive()
    }),
    MenuItem::new("file-project-settings", "Project Settings…", |_, s, _| {
        s.open_settings_at("Canvas")
    }),
    MenuItem::new("file-quit", "Quit", |_, s, _| s.request_quit()).key("key-quit"),
];

pub(super) fn file_items(state: &AppState) -> MenuItems {
    let recents = state.recents();
    let imported = state.import_recents();
    let exporters = state.ctx.exporters();
    let mut items = Vec::with_capacity(
        FILE_OPEN.len()
            + recents.len()
            + FILE_SAVE.len()
            + imported.len()
            + 1
            + FILE_IMPORT.len()
            + exporters.len()
            + FILE_CLOSE.len(),
    );
    items.extend_from_slice(FILE_OPEN);
    // One row per remembered project, newest first. The id holds the path, so
    // one `fn` runs every row.
    for recent in recents.iter() {
        let path = recent.path.display().to_string();
        items.push(
            MenuItem::new(
                crate::interned::intern(&format!("{RECENT_PREFIX}{path}")),
                crate::interned::intern(&path),
                |id, s, _| {
                    if let Some(path) = id.strip_prefix(RECENT_PREFIX) {
                        s.open_recent(std::path::Path::new(path));
                    }
                },
            )
            .when(super::menu_guards::recent)
            .under(OPEN_RECENT),
        );
    }
    items.extend_from_slice(FILE_SAVE);
    // One row per recently imported file, newest first, then the row that
    // empties the list.
    for path in imported {
        let path = path.display().to_string();
        items.push(
            MenuItem::new(
                crate::interned::intern(&format!("{IMPORT_RECENT_PREFIX}{path}")),
                crate::interned::intern(&path),
                |id, s, _| {
                    if let Some(path) = id.strip_prefix(IMPORT_RECENT_PREFIX) {
                        s.import_recent(std::path::Path::new(path));
                    }
                },
            )
            .when(super::menu_guards::import_recent)
            .under(IMPORT_RECENT),
        );
    }
    items.push(FILE_IMPORT_CLEAR);
    items.extend_from_slice(FILE_IMPORT);
    // One row per mounted timeline exporter, each asking for a file of its own
    // format only.
    for exporter in &exporters {
        items.push(
            MenuItem::new(
                crate::interned::intern(&format!("{EXPORT_PREFIX}{}", exporter.id)),
                crate::interned::intern(&format!("{}…", exporter.name)),
                |id, s, _| {
                    if let Some(exporter) = id.strip_prefix(EXPORT_PREFIX) {
                        s.pick_and_export_timeline_as(exporter);
                    }
                },
            )
            .under(EXPORT),
        );
    }
    items.extend_from_slice(FILE_CLOSE);
    Cow::Owned(items)
}

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
    MenuItem::new("edit-paste-fit", "Paste To Fit", |_, s, _| {
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

/// Whether every selected clip is enabled, with at least one selected.
fn selection_enabled(_: &str, state: &AppState) -> bool {
    !state.selection.is_empty()
        && state
            .selection
            .iter()
            .all(|clip| state.ui_clip_ref(*clip).is_some_and(|ui| ui.enabled))
}

static CLIP_ITEMS: &[MenuItem] = &[
    MenuItem::new("clip-rename", "Rename…", |_, s, _| {
        if let Some(clip) = s.selected_clip() {
            s.begin_rename(crate::types::RenameTarget::Clip(clip.raw()));
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-subclip", "Make Subclip", |_, s, _| {
        let made = s.create_subclip_from_source(None).map(drop);
        report(s, made)
    })
    .when(super::menu_guards::source_range),
    MenuItem::new("clip-speed-duration", "Speed/Duration…", |_, s, _| {
        s.open_speed_duration()
    })
    .key("key-speed-duration")
    .when(super::menu_guards::selection),
    MenuItem::new("clip-time-remap", "Time Remapping", |_, s, _| {
        let remapped = s.toggle_time_remap();
        report(s, remapped)
    })
    .key("key-time-remap")
    .when(super::menu_guards::selection)
    .checked(|_, s| s.selected_ui_clip_ref().is_some_and(|clip| clip.time_remap)),
    MenuItem::new("clip-enable", "Enable", |_, s, _| {
        let toggled = s.toggle_enabled_selected();
        report(s, toggled)
    })
    .key("key-clip-enable")
    .when(super::menu_guards::selection)
    .checked(selection_enabled),
    MenuItem::new("seq-link", "Link", |_, s, _| {
        let linked = s.link_selected();
        report(s, linked)
    })
    .when(super::menu_guards::several),
    MenuItem::new("seq-unlink", "Unlink", |_, s, _| {
        let unlinked = s.unlink_selected();
        report(s, unlinked)
    })
    .when(super::menu_guards::linked),
    MenuItem::new("clip-group", "Group", |_, s, _| {
        let grouped = s.group_selected();
        report(s, grouped)
    })
    .key("key-clip-group")
    .when(super::menu_guards::several),
    MenuItem::new("clip-ungroup", "Ungroup", |_, s, _| {
        let ungrouped = s.ungroup_selected();
        report(s, ungrouped)
    })
    .key("key-clip-ungroup")
    .when(super::menu_guards::grouped),
    MenuItem::new("seq-nest", "Nest…", |_, s, _| {
        crate::commands::reach::nest_selection(s)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-replace-source", "From Source Monitor", |_, s, _| {
        let replaced = s.replace_with_source();
        report(s, replaced)
    })
    .when(super::menu_guards::source)
    .under("Replace with Clip"),
    MenuItem::new("seq-freeze", "Freeze Frame", |_, s, _| {
        let frozen = s.freeze_selected_frame();
        report(s, frozen)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-bake-clip", "Render and Replace", |_, s, _| {
        if let Err(err) = s.render_and_replace() {
            s.status_message = Some(err);
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-bake-restore", "Restore Unrendered", |_, s, _| {
        if let Err(err) = s.restore_unrendered() {
            s.status_message = Some(err);
        }
    })
    .when(super::menu_guards::selection),
    MenuItem::new("clip-audio-gain", "Audio Gain…", |_, s, _| {
        s.open_audio_gain()
    })
    .key("key-audio-gain")
    .when(super::menu_guards::selection),
    MenuItem::new("clip-channels-stereo", "Stereo", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-swap", "Swap Channels", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-left", "Left to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-right", "Right to Both", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("clip-channels-mono", "Mono", run_audio_channels)
        .checked(audio_channels_checked)
        .when(super::menu_guards::sound)
        .under(AUDIO_CHANNELS),
    MenuItem::new("seq-extract-audio", "Extract Audio", |_, s, _| {
        let extracted = s.extract_selected_audio();
        report(s, extracted)
    })
    .when(super::menu_guards::selection),
    MenuItem::new("seq-flip-h", "Flip Horizontal", |_, s, _| {
        let flipped = s.toggle_selected_flip_h();
        report(s, flipped)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-flip-v", "Flip Vertical", |_, s, _| {
        let flipped = s.toggle_selected_flip_v();
        report(s, flipped)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-crop-reset", "Reset Framing", |_, s, _| {
        let reset = s.clear_selected_crop();
        report(s, reset)
    })
    .when(super::menu_guards::selection)
    .under("Transform"),
    MenuItem::new("seq-matte-luma", "Luma Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Luma);
        report(s, set)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
    MenuItem::new("seq-matte-alpha", "Alpha Matte from Above", |_, s, _| {
        let set = s.set_selected_matte_source(aria_models::MatteSource::Alpha);
        report(s, set)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
    MenuItem::new("seq-matte-clear", "Remove Track Matte", |_, s, _| {
        let cleared = s.clear_selected_matte();
        report(s, cleared)
    })
    .when(super::menu_guards::selection)
    .under("Track Matte"),
];

fn run_clip_analyzer(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    if let Some(analyzer_id) = id.strip_prefix(CLIP_ANALYZE_PREFIX)
        && let Err(err) = state.run_registered_analyzer(analyzer_id, cx)
    {
        state.report_error(err);
    }
}

pub(super) fn clip_items(state: &AppState) -> MenuItems {
    let analyzers = state
        .ctx
        .analyzers_for_scope(aria_plugin::AnalyzerScope::Clip);
    if analyzers.is_empty() {
        return Cow::Borrowed(CLIP_ITEMS);
    }
    let intern = crate::interned::intern;
    let mut items = Vec::with_capacity(CLIP_ITEMS.len() + analyzers.len());
    items.extend_from_slice(CLIP_ITEMS);
    for entry in analyzers {
        let spec = entry.spec();
        items.push(
            MenuItem::new(
                intern(&format!("{CLIP_ANALYZE_PREFIX}{}", spec.id)),
                intern(&spec.label),
                run_clip_analyzer,
            )
            .when(super::menu_guards::sound)
            .under(ANALYZE),
        );
    }
    Cow::Owned(items)
}

static VIEW_ITEMS: &[MenuItem] = &[
    MenuItem::new("seq-in", "Zoom In", |_, s, _| {
        s.set_zoom(s.zoom * crate::transport::ZOOM_STEP, true);
    })
    .key("key-zoom-in"),
    MenuItem::new("seq-out", "Zoom Out", |_, s, _| {
        s.set_zoom(s.zoom / crate::transport::ZOOM_STEP, true);
    })
    .key("key-zoom-out"),
    MenuItem::new("seq-fit", "Fit", |_, s, _| s.zoom_fit_sequence()).key("key-fit"),
    MenuItem::new("seq-fit-selection", "Fit Selection", |_, s, _| {
        s.zoom_fit_selection()
    })
    .key("key-fit-selection")
    .when(super::menu_guards::selection),
    MenuItem::new(
        "view-playback-resolution",
        "Playback Resolution",
        |_, s, cx| s.cycle_playback_resolution(cx),
    )
    .under("Resolution"),
    MenuItem::new("view-paused-resolution", "Paused Resolution", |_, s, cx| {
        s.toggle_paused_resolution(cx)
    })
    .under("Resolution"),
    MenuItem::new("view-rulers", "Show Rulers", |_, s, _| s.toggle_rulers())
        .key("key-rulers")
        .checked(|_, s| s.viewer_guides.rulers),
    MenuItem::new("view-guides-show", "Show Guides", |_, s, _| {
        s.toggle_guides_shown()
    })
    .checked(|_, s| !s.viewer_guides.hidden),
    MenuItem::new("view-guides-clear", "Clear Guides", |_, s, _| {
        s.clear_guides()
    }),
    MenuItem::new("view-title-safe", "Title Safe", |_, s, _| {
        s.toggle_title_safe()
    })
    .checked(|_, s| s.preview_view.title_safe)
    .under("Safe Margins"),
    MenuItem::new("view-action-safe", "Action Safe", |_, s, _| {
        s.toggle_action_safe()
    })
    .checked(|_, s| s.preview_view.action_safe)
    .under("Safe Margins"),
    MenuItem::new("view-guide-snap", "Snap to Guides", |_, s, _| {
        s.toggle_guide_snap()
    })
    .checked(|_, s| s.viewer_guides.snap),
    MenuItem::new("view-transparency-grid", "Transparency Grid", |_, s, cx| {
        s.toggle_transparency_grid(cx)
    })
    .checked(|_, s| s.transparency_grid_on()),
    MenuItem::new("view-proxies", "Proxies", |_, s, cx| s.toggle_proxies(cx))
        .checked(|_, s| s.proxies_on()),
    MenuItem::new("view-grid", "Grid", |_, s, _| s.toggle_preview_grid())
        .checked(|_, s| s.preview_view.grid),
    MenuItem::new("view-split", "Split Viewer", |_, s, _| {
        s.toggle_viewer_split()
    })
    .checked(|_, s| s.sources.split()),
    MenuItem::new("view-compare-frame", "Current Frame", |_, s, cx| {
        s.compare_with_frame(cx)
    })
    .checked(|_, s| matches!(s.viewer_compare.reference(), Some(Reference::Frame { .. })))
    .under(COMPARE),
    MenuItem::new("view-compare-still", "Still…", |_, s, cx| {
        s.pick_compare_still(cx)
    })
    .checked(|_, s| matches!(s.viewer_compare.reference(), Some(Reference::Still { .. })))
    .under(COMPARE),
    MenuItem::new("view-compare-source", "Source Tab", |_, s, _| {
        s.compare_with_source()
    })
    .when(super::menu_guards::source)
    .checked(|_, s| s.viewer_compare.reference() == Some(&Reference::Source))
    .under(COMPARE),
    MenuItem::new("view-compare-vertical", "Vertical Wipe", |_, s, _| {
        s.set_compare_layout(CompareLayout::Vertical)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::Vertical)
    .under(COMPARE),
    MenuItem::new("view-compare-horizontal", "Horizontal Wipe", |_, s, _| {
        s.set_compare_layout(CompareLayout::Horizontal)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::Horizontal)
    .under(COMPARE),
    MenuItem::new("view-compare-side", "Side by Side", |_, s, _| {
        s.set_compare_layout(CompareLayout::SideBySide)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::SideBySide)
    .under(COMPARE),
    MenuItem::new("view-compare-stop", "Stop Comparing", |_, s, _| {
        s.stop_comparing()
    })
    .when(super::menu_guards::comparing)
    .under(COMPARE),
    MenuItem::new("view-scopes", "Scope Strip", |_, s, _| s.toggle_scopes())
        .checked(|_, s| s.scopes_open),
    waveform_row(WaveformMode::Luma),
    waveform_row(WaveformMode::RgbParade),
    waveform_row(WaveformMode::YcbcrParade),
    MenuItem::new("view-zoom-fit", "Fit Preview", |_, s, _| {
        s.preview_zoom_fit()
    }),
    MenuItem::new("view-zoom-actual", "Preview 100%", |_, s, _| {
        s.preview_zoom_actual()
    }),
    MenuItem::new("view-handles-transform", "Transform", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Transform)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Transform)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-handles-crop", "Crop", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Crop)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Crop)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-handles-mask", "Mask", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Mask)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Mask)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-fullscreen", "Fullscreen Preview", |_, s, _| {
        s.toggle_preview_fullscreen()
    })
    .checked(|_, s| s.preview_view.fullscreen),
    MenuItem::new("view-loop", "Loop Playback", |_, s, _| s.toggle_loop())
        .key("key-loop")
        .checked(|_, s| s.looping),
    MenuItem::new("view-thumbs-head", "Head Thumbnails", |_, s, cx| {
        s.set_timeline_thumbnails(Thumbnails::Head, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::Head)
    .under(TIMELINE_DISPLAY),
    MenuItem::new(
        "view-thumbs-head-tail",
        "Head and Tail Thumbnails",
        |_, s, cx| s.set_timeline_thumbnails(Thumbnails::HeadAndTail, cx),
    )
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::HeadAndTail)
    .under(TIMELINE_DISPLAY),
    MenuItem::new(
        "view-thumbs-continuous",
        "Continuous Thumbnails",
        |_, s, cx| s.set_timeline_thumbnails(Thumbnails::Continuous, cx),
    )
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::Continuous)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-log", "Logarithmic Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Logarithmic, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Logarithmic)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-linear", "Linear Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Linear, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Linear)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-hidden", "Hide Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Hidden, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Hidden)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-follow-page", "Page Scroll", |_, s, cx| {
        s.set_timeline_follow(Follow::Page, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.follow == Follow::Page)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-follow-smooth", "Smooth Scroll", |_, s, cx| {
        s.set_timeline_follow(Follow::Smooth, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.follow == Follow::Smooth)
    .under(TIMELINE_DISPLAY),
];

/// The View submenu holding the timeline's thumbnail, waveform, and follow
/// choices.
pub(crate) const TIMELINE_DISPLAY: &str = "Timeline Display Settings";

/// The View submenu choosing what the scopes' waveform monitor shows.
pub(crate) const SCOPE_WAVEFORM: &str = "Scope Waveform";

/// The View row showing `mode` in the scopes' waveform monitor.
const fn waveform_row(mode: WaveformMode) -> MenuItem {
    MenuItem::new(mode.menu_id(), mode.label(), run_waveform_row)
        .checked(waveform_row_checked)
        .under(SCOPE_WAVEFORM)
}

/// Show the waveform mode whose row ran.
fn run_waveform_row(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    if let Some(mode) = WaveformMode::ALL
        .into_iter()
        .find(|mode| mode.menu_id() == id)
    {
        state.set_waveform_mode(mode, cx);
    }
}

/// Whether the row is the waveform mode shown.
fn waveform_row_checked(id: &str, state: &AppState) -> bool {
    state.waveform_mode().menu_id() == id
}

pub(super) fn view_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(VIEW_ITEMS)
}

const WORKSPACES: &str = "Workspaces";

/// Id prefix of the Window rows generated one per registered dock panel that
/// no row in [`PANEL_ROWS`] opens. The rest of the id is the panel id.
pub(crate) const PANEL_PREFIX: &str = "win-panel:";

/// The Window panel rows written by hand: each row's id, the dock panel it
/// opens, and what a press runs. A row is labeled with its panel's title and checked while the
/// panel is open, front tab or not. Every other registered panel, built in or
/// declared by a plugin, gets a generated row.
pub(crate) const PANEL_ROWS: &[(&str, crate::dock::PanelId, RowRun)] = &[
    ("win-media", crate::dock::PANEL_PROJECT, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_PROJECT);
    }),
    ("win-effects", crate::dock::PANEL_EFFECTS, |_, s, _| {
        s.open_effects_panel()
    }),
    ("win-program", crate::dock::PANEL_VIEWER, |_, s, _| {
        s.focus_pane(ActivePanel::Preview)
    }),
    ("win-timeline", crate::dock::PANEL_TIMELINE, |_, s, _| {
        s.focus_pane(ActivePanel::Timeline)
    }),
    ("win-inspector", crate::dock::PANEL_INSPECTOR, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_INSPECTOR);
    }),
    ("win-assistant", crate::dock::PANEL_ASSISTANT, |_, s, _| {
        s.toggle_pane_tab(ActivePanel::Ai);
    }),
    ("win-mixer", crate::dock::PANEL_MIXER, |_, s, _| {
        s.toggle_pane_tab(ActivePanel::Mixer);
    }),
    ("win-jobs", crate::dock::PANEL_JOBS, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_JOBS);
    }),
];

/// The check of a [`PANEL_ROWS`] row: its panel is open.
fn panel_row_open(id: &str, state: &AppState) -> bool {
    PANEL_ROWS
        .iter()
        .find(|(row, ..)| *row == id)
        .is_some_and(|&(_, panel, _)| state.dock_tree.is_open(panel))
}

/// The binding that brings a panel forward, drawn beside its Window row.
const PANEL_KEYS: &[(crate::dock::PanelId, &str)] = &[
    (crate::dock::PANEL_EFFECTS, "key-pane-effects"),
    (crate::dock::PANEL_TIMELINE, "key-pane-timeline"),
    (crate::dock::PANEL_ASSISTANT, "key-assistant"),
    (crate::dock::PANEL_MIXER, "key-mixer"),
    (crate::dock::PANEL_INFO, "key-pane-info"),
    (crate::dock::PANEL_AUDIO_METERS, "key-pane-audio-meters"),
    (crate::dock::PANEL_HISTORY, "key-history"),
    (crate::dock::PANEL_SCOPES, "key-scopes"),
];

/// The Window row opening `panel`, labeled with its title and drawing the
/// chord [`PANEL_KEYS`] gives it.
fn panel_row(
    state: &AppState,
    id: &'static str,
    panel: crate::dock::PanelId,
    run: RowRun,
) -> MenuItem {
    let row = MenuItem::new(id, state.dock_panel_title(panel), run);
    match PANEL_KEYS.iter().find(|&&(keyed, _)| keyed == panel) {
        Some(&(_, key)) => row.key(key),
        None => row,
    }
}

/// The Window rows after the panels.
static WINDOW_TOOLS: &[MenuItem] = &[
    MenuItem::new("win-palette", "Command Palette", |_, s, _| {
        s.open_dialog(DialogKind::Palette)
    })
    .key("key-palette"),
    MenuItem::new("win-home", "Home", |_, s, _| s.request_go_home()),
];

pub(super) fn window_items(state: &AppState) -> MenuItems {
    let mut items = Vec::new();
    // One row per workspace from the list Settings draws: the shipped ones,
    // the plugins', and the saved slot once there is something saved in it.
    // Each row is told its own id, which is the workspace's id.
    for ws in state.workspace_rows() {
        items.push(
            MenuItem::new(ws.id, ws.name, |id, s, cx| {
                s.apply_workspace(id, cx);
            })
            .checked(|id, s| s.is_active_workspace(id))
            .under(WORKSPACES),
        );
    }
    items.push(
        MenuItem::new(crate::layouts::SAVE_ID, "Save Workspace", |_, s, cx| {
            s.save_workspace(cx)
        })
        .under(WORKSPACES),
    );
    items.push(
        MenuItem::new(
            crate::layouts::RESET_ID,
            "Reset to Saved Layout",
            |_, s, cx| s.reset_workspace_layout(cx),
        )
        .under(WORKSPACES),
    );
    items.extend(
        PANEL_ROWS
            .iter()
            .map(|&(id, panel, run)| panel_row(state, id, panel, run).checked(panel_row_open)),
    );
    // One row per other registered dock panel, so a panel added to the registry
    // or declared by a plugin is opened from the menu with no row written for
    // it. The id holds the panel id, so one `fn` runs every row.
    for panel in state.registered_panel_ids() {
        if PANEL_ROWS.iter().any(|&(_, opened, _)| opened == panel) {
            continue;
        }
        let id = crate::interned::intern(&format!("{PANEL_PREFIX}{panel}"));
        items.push(
            panel_row(state, id, panel, |id, s, _| {
                if let Some(panel) = id.strip_prefix(PANEL_PREFIX) {
                    s.toggle_dock_panel(crate::interned::intern(panel));
                }
            })
            .checked(|id, s| {
                id.strip_prefix(PANEL_PREFIX)
                    .is_some_and(|panel| s.dock_tree.is_open(panel))
            }),
        );
    }
    items.extend_from_slice(WINDOW_TOOLS);
    // A report on a project with no sources and no clips is a column of zeroes,
    // so the item is offered once there is something to report on.
    if state.project_report_available() {
        items.push(MenuItem::new(
            "win-project-report",
            "Project Report",
            |_, s, _| s.open_dialog(DialogKind::ProjectReport),
        ));
    }
    Cow::Owned(items)
}

static HELP_ITEMS: &[MenuItem] = &[
    MenuItem::new("help-learn", "Keyboard Shortcuts", |_, s, _| {
        s.set_home_rail(HomeRail::Learn);
        s.request_go_home();
    }),
    MenuItem::new("help-plugins", "Plugin Development", |_, _, cx| {
        cx.open_url(PLUGIN_GUIDE)
    }),
    MenuItem::new("help-data", "Open Data Folder", |_, s, _| {
        crate::app::recents::reveal_in_folder(&s.data_dir)
    }),
    MenuItem::new("help-updates", "Check for Updates", |_, s, cx| {
        s.check_for_updates(cx)
    }),
    MenuItem::new("help-about", "About", |_, s, _| {
        s.open_dialog(DialogKind::About)
    })
    .checked(|_, s| s.dialog == DialogKind::About),
];

/// The plugin developer guide, in the source repository of this build.
pub(crate) const PLUGIN_GUIDE: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/plugins/README.md"
);

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

    /// Labels are unique *within* each list a menu draws: two identical rows in
    /// one dropdown or one flyout are indistinguishable to the operator. A
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
}
