//! The File menu: projects and sequences, saving, recent projects, import,
//! recent imports, export, and quitting.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::{AppState, DialogKind};

use super::menu_items::{MenuItem, MenuItems, report};

/// The id prefix of an Open Recent row. The rest of the id is the project's path.
pub(crate) const RECENT_PREFIX: &str = "file-recent:";
/// The id prefix of an Export row for one timeline exporter. The rest of the id
/// is the exporter's id.
pub(crate) const EXPORT_PREFIX: &str = "file-export-as:";
/// The id prefix of an Import Recent row. The rest of the id is the file's
/// path.
pub(crate) const IMPORT_RECENT_PREFIX: &str = "file-import-recent:";

const OPEN_RECENT: &str = "Open Recent";
const EXPORT: &str = "Export";
const IMPORT_RECENT: &str = "Import Recent";

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
