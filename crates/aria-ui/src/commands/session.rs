//! Palette rows for the session, the viewer, the pool, and the sequence.
//!
//! One of the three tables [`super::BUILTIN_COMMANDS`] joins. Split by
//! concern rather than by size: a row here acts on the project, the window, or
//! the timeline as a whole, and needs no clip selected.

use super::host::CommandEntry;

/// Rows that act on the session, the viewer, the pool, or the whole sequence.
pub(super) const SESSION_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-play",
        label: "Play / Pause",
        run: |state| state.toggle_playback(),
    },
    CommandEntry {
        id: "pal-handles",
        label: "Cycle Canvas Handles",
        run: |state| state.cycle_overlay_mode(),
    },
    CommandEntry {
        id: "pal-bin-new",
        label: "New Bin",
        run: |state| {
            if let Err(err) = state.create_bin() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-bin-up",
        label: "Leave Bin",
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
        label: "Next Detected Moment",
        run: |state| state.seek_next_moment(),
    },
    CommandEntry {
        id: "pal-previous-moment",
        label: "Previous Detected Moment",
        run: |state| state.seek_previous_moment(),
    },
];
