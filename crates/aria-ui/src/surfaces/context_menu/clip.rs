//! The rows a clip's own menu draws, and the junction menu beside it.
//!
//! Split out of the parent by size: the tables are the bulk of the menu, and a
//! target's facts belong beside the rows that read them.

use aria_engine::EngineError;
use aria_models::ClipId;

use crate::app::AppState;
use crate::types::{ActivePanel, RenameTarget};

use super::{MenuItem, MenuRow, always};

/// The clip facts a row reads, gathered once when the menu opens.
#[derive(Clone, Copy)]
pub struct ClipMenu {
    /// The clip under the pointer.
    pub clip: u64,
    /// The selection holds more than one clip, so linking is possible.
    pub multi: bool,
    /// The selection is already linked, so unlinking is.
    pub linked: bool,
    pub can_extract_audio: bool,
    pub can_freeze_frame: bool,
}

impl ClipMenu {
    /// Read the clip facts a menu would gather when it opens over `clip`.
    ///
    /// One owner for the reading, so the panel that draws the rows and the census
    /// that presses them cannot disagree about what a row was offered against.
    pub fn of(state: &AppState, clip: u64) -> Self {
        Self {
            clip,
            multi: state.selection.len() > 1,
            linked: state.selection_is_linked(),
            can_extract_audio: state.selected_can_extract_audio(),
            can_freeze_frame: state.selected_can_freeze_frame(),
        }
    }
}

/// A clip menu, in the order it draws.
pub const CLIP_ITEMS: &[MenuItem<ClipMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-split",
        label: |_| "Split",
        when: always(),
        run: |state, _| state.split_at_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-delete",
        label: |_| "Delete",
        when: always(),
        run: |state, _| state.delete_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-duplicate",
        label: |_| "Duplicate",
        when: always(),
        run: |state, _| state.duplicate_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-copy",
        label: |_| "Copy",
        when: always(),
        run: |state, _| {
            state.copy_selected();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-rename-clip",
        label: |_| "Rename Clip",
        when: always(),
        run: |state, clip| {
            // The rename bar takes the keyboard, and a menu still on screen would
            // eat the first keystroke.
            state.close_context();
            state.begin_rename(RenameTarget::Clip(clip.clip));
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-freeze",
        label: |_| "Freeze Frame",
        when: |clip| clip.can_freeze_frame,
        run: |state, _| state.freeze_selected_frame(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-extract-audio",
        label: |_| "Extract Audio",
        when: |clip| clip.can_extract_audio,
        run: |state, _| state.extract_selected_audio(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-link",
        label: |_| "Link Clips",
        when: |clip| clip.multi,
        run: |state, _| state.link_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-unlink",
        label: |_| "Unlink Clips",
        when: |clip| clip.linked,
        run: |state, _| state.unlink_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-shift-selection",
        label: |_| "Shift selection…",
        when: always(),
        run: |state, _| {
            state.begin_rename(RenameTarget::ShiftFrames);
            Ok(())
        },
    }),
    MenuItem::Labels,
    MenuItem::Row(MenuRow {
        id: "ctx-properties",
        label: |_| "Properties",
        when: always(),
        run: |state, _| {
            state.active_panel = ActivePanel::Inspector;
            Ok(())
        },
    }),
];

/// A junction menu: take the blend off, then every blend that could go on.
pub const TRANSITION_ITEMS: &[MenuItem<ClipMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-transition-remove",
        label: |_| "Remove transition",
        when: always(),
        run: |state, clip| state.clear_clip_transition(ClipId::from_raw(clip.clip)),
    }),
    MenuItem::Blends,
];

/// Put the blend `id` on a junction.
///
/// One owner for what a catalog row asks for: the rows themselves come from
/// `app::clip_transitions`, so a new blend appears in the menu without a second
/// list, and this is the call a census drives instead of the closure the panel
/// wraps it in.
pub fn apply_blend(state: &mut AppState, junction: &ClipMenu, id: &str) -> Result<(), EngineError> {
    state.set_clip_transition(ClipId::from_raw(junction.clip), id)
}
