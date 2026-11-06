//! Palette rows for the session, the viewer, the pool, and the sequence.
//!
//! One of the three tables [`super::BUILTIN_COMMANDS`] joins. Split by
//! concern rather than by size: a row here acts on the project, the window, or
//! the timeline as a whole, and needs no clip selected.

use crate::chrome::menu_guards as guards;

use super::host::CommandEntry;

/// Rows that act on the session, the viewer, the pool, or the whole sequence.
pub(super) const SESSION_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-play",
        label: "Play / Pause",
        run: |state| {
            if let Some(reason) = guards::clips("pal-play", state) {
                state.status_message = Some(reason.to_string());
                return;
            }
            state.toggle_playback();
        },
        blocked: guards::clips,
    },
    CommandEntry {
        id: "pal-handles",
        label: "Cycle Canvas Handles",
        run: |state| state.cycle_overlay_mode(),
        blocked: guards::ready,
    },
    CommandEntry {
        id: "pal-bin-new",
        label: "New Bin",
        run: |state| {
            if let Err(err) = state.create_bin() {
                state.report_error(&err);
            }
        },
        blocked: guards::ready,
    },
    CommandEntry {
        id: "pal-bin-up",
        label: "Leave Bin",
        run: |state| {
            if let Some(reason) = guards::bin_up("pal-bin-up", state) {
                state.status_message = Some(reason.to_string());
                return;
            }
            let parent = state.bin_path().iter().rev().nth(1).map(|item| item.id);
            state.open_bin(parent);
        },
        blocked: guards::bin_up,
    },
    CommandEntry {
        id: "pal-captions",
        label: "Captions",
        run: |state| state.show_sidebar_tab(crate::types::SidebarTab::Captions),
        blocked: guards::ready,
    },
    CommandEntry {
        id: "pal-analysis",
        label: "Analysis",
        run: |state| state.show_sidebar_tab(crate::types::SidebarTab::Analysis),
        blocked: guards::ready,
    },
    CommandEntry {
        id: "pal-next-moment",
        label: "Next Detected Moment",
        run: |state| {
            if let Some(reason) = guards::moment("pal-next-moment", state) {
                state.status_message = Some(reason.to_string());
                return;
            }
            state.seek_next_moment();
        },
        blocked: guards::moment,
    },
    CommandEntry {
        id: "pal-previous-moment",
        label: "Previous Detected Moment",
        run: |state| {
            if let Some(reason) = guards::moment("pal-previous-moment", state) {
                state.status_message = Some(reason.to_string());
                return;
            }
            state.seek_previous_moment();
        },
        blocked: guards::moment,
    },
];
