//! Palette rows for the project as a whole: what it carries and where a source
//! lives.
//!
//! One of the groups [`super::BUILTIN_COMMANDS`] joins. A row here
//! acts on the pool, the sequence inventory, or the session's measurements
//! rather than on one clip's properties.
//!
//! Nothing here names a clip question: the row that walks them reads
//! [`aria_models::clip_facts`] at run time, so a question added to that table
//! is reachable from the palette without a row being added here.

use super::host::CommandEntry;

/// Rows that act on the project, the pool, or the session's measurements.
///
/// Crate-visible rather than `pub(super)` like its sibling groups: the cases
/// that drive these rows live beside the verbs they call, in
/// `app::project_reach`, because the fixture they need is the editor's and not
/// the palette's.
pub(crate) const PROJECT_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-project-clip-under-playhead",
        label: "Select Clip Under Playhead",
        run: |state| state.select_clip_under_playhead(),
    },
    CommandEntry {
        id: "pal-project-next-flagged-clip",
        label: "Go to Next Flagged Clip",
        run: |state| state.go_to_next_flagged_clip(),
    },
    CommandEntry {
        id: "pal-project-duplicate-to-lane",
        label: "Duplicate Clip onto Another Lane",
        run: |state| {
            if let Err(err) = state.duplicate_selection_to_next_lane() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-project-reveal-media",
        label: "Reveal a File in the Pool",
        run: |state| state.pick_and_reveal_media(),
    },
    CommandEntry {
        id: "pal-project-describe-template",
        label: "Describe a Template Bundle",
        run: |state| state.pick_and_describe_template(),
    },
    CommandEntry {
        id: "pal-project-history-pending",
        label: "Name the Next Undo and Redo",
        run: |state| state.report_pending_history(),
    },
    CommandEntry {
        id: "pal-project-content-size",
        label: "Report Drawn Size of Selection",
        run: |state| state.report_content_size(),
    },
];
