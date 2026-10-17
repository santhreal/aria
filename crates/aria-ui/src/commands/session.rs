//! Palette rows for the session, the viewer, the pool, and the sequence.
//!
//! One of the three tables [`super::BUILTIN_COMMANDS`] joins. Split by
//! concern rather than by size: a row here acts on the project, the window, or
//! the timeline as a whole, and needs no clip selected.

use crate::app::DialogKind;

use super::host::CommandEntry;

/// Rows that act on the session, the viewer, the pool, or the whole sequence.
pub(super) const SESSION_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-play",
        label: "Play / Pause",
        run: |state| state.toggle_playback(),
    },
    CommandEntry {
        id: "pal-split",
        label: "Split at playhead",
        run: |state| {
            if let Err(err) = state.split_at_playhead() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-delete",
        label: "Delete selected",
        run: |state| {
            if let Err(err) = state.clear_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-ripple-delete",
        label: "Ripple delete selected",
        run: |state| {
            if let Err(err) = state.ripple_delete_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-undo",
        label: "Undo",
        run: |state| {
            state.undo();
        },
    },
    CommandEntry {
        id: "pal-redo",
        label: "Redo",
        run: |state| {
            state.redo();
        },
    },
    CommandEntry {
        id: "pal-save",
        label: "Save project",
        run: |state| {
            if let Err(err) = state.save_project() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-open",
        label: "Go Home",
        run: |state| state.request_go_home(),
    },
    CommandEntry {
        id: "pal-new",
        label: "New project",
        run: |state| state.request_new_project(),
    },
    CommandEntry {
        id: "pal-export",
        label: "Export",
        run: |state| state.open_dialog(DialogKind::Export),
    },
    CommandEntry {
        id: "pal-export-frame",
        label: "Export frame",
        run: |state| state.pick_and_export_frame(),
    },
    CommandEntry {
        id: "pal-settings",
        label: "Settings",
        run: |state| state.open_dialog(DialogKind::Settings),
    },
    CommandEntry {
        id: "pal-jobs",
        label: "Jobs",
        run: |state| state.open_dialog(DialogKind::Jobs),
    },
    CommandEntry {
        id: "pal-history",
        label: "History",
        run: |state| state.focus_dock_panel(crate::dock::PANEL_HISTORY),
    },
    CommandEntry {
        id: "pal-preview-fullscreen",
        label: "Fullscreen preview",
        run: |state| state.toggle_preview_fullscreen(),
    },
    CommandEntry {
        id: "pal-preview-title-safe",
        label: "Title safe",
        run: |state| state.toggle_title_safe(),
    },
    CommandEntry {
        id: "pal-preview-action-safe",
        label: "Action safe",
        run: |state| state.toggle_action_safe(),
    },
    CommandEntry {
        id: "pal-preview-grid",
        label: "Preview grid",
        run: |state| state.toggle_preview_grid(),
    },
    CommandEntry {
        id: "pal-preview-scopes",
        label: "Scope strip",
        run: |state| state.toggle_scopes(),
    },
    CommandEntry {
        id: "pal-preview-fit",
        label: "Fit preview",
        run: |state| state.preview_zoom_fit(),
    },
    CommandEntry {
        id: "pal-preview-actual",
        label: "Preview 100%",
        run: |state| state.preview_zoom_actual(),
    },
    CommandEntry {
        id: "pal-handles",
        label: "Cycle canvas handles",
        run: |state| state.cycle_overlay_mode(),
    },
    CommandEntry {
        id: "pal-bin-new",
        label: "New bin",
        run: |state| {
            if let Err(err) = state.create_bin() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-bin-up",
        label: "Leave bin",
        run: |state| {
            let parent = state.bin_path().iter().rev().nth(1).map(|item| item.id);
            state.open_bin(parent);
        },
    },
    CommandEntry {
        id: "pal-captions",
        label: "Captions",
        run: |state| state.sidebar_tab = crate::types::SidebarTab::Captions,
    },
    CommandEntry {
        id: "pal-analysis",
        label: "Analysis",
        run: |state| state.sidebar_tab = crate::types::SidebarTab::Analysis,
    },
    CommandEntry {
        id: "pal-next-moment",
        label: "Next detected moment",
        run: |state| state.seek_next_moment(),
    },
    CommandEntry {
        id: "pal-previous-moment",
        label: "Previous detected moment",
        run: |state| state.seek_previous_moment(),
    },
    CommandEntry {
        id: "pal-fit",
        label: "Fit timeline",
        run: |state| state.zoom_fit_sequence(),
    },
    CommandEntry {
        id: "pal-fit-selection",
        label: "Fit selection",
        run: |state| state.zoom_fit_selection(),
    },
    CommandEntry {
        id: "pal-paste-insert",
        label: "Paste Insert",
        run: |state| {
            if let Err(err) = state.paste_insert() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-paste-attributes",
        label: "Paste Attributes",
        run: |state| {
            if let Err(err) = state.paste_attributes() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-relink",
        label: "Relink All",
        run: |state| state.relink_all(),
    },
    CommandEntry {
        id: "pal-marker",
        label: "Add Marker",
        run: |state| {
            if let Err(err) = state.add_marker_at_playhead() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-adjustment-layer",
        label: "Add Adjustment Layer",
        run: |state| {
            if let Err(err) = state.add_adjustment_layer() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-mark-in",
        label: "Mark In",
        run: |state| state.set_mark_in(),
    },
    CommandEntry {
        id: "pal-mark-out",
        label: "Mark Out",
        run: |state| state.set_mark_out(),
    },
];
