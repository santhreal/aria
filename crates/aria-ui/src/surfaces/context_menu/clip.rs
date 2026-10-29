//! The rows a clip's own menu draws, and the junction menu beside it.
//!
//! Split out of the parent by size: the tables are the bulk of the menu, and a
//! target's facts belong beside the rows that read them.

use aria_engine::EngineError;
use aria_models::{ClipId, ParentKeep, TransitionAlignment};

use crate::app::AppState;
use crate::types::{ActivePanel, RenameTarget};

use super::{MenuItem, MenuRow, always};

/// The clip facts a row reads, gathered once when the menu opens.
#[derive(Debug, Clone, Copy)]
pub struct ClipMenu {
    /// The clip under the pointer.
    pub clip: u64,
    /// The selection holds more than one clip, so linking is possible.
    pub multi: bool,
    /// The selection is already linked, so unlinking is.
    pub linked: bool,
    /// The selection forms a clip group, so ungrouping is possible.
    pub grouped: bool,
    /// Every selected clip is enabled, which Enable's check mark shows.
    pub enabled: bool,
    /// Whether the clip supports time remapping.
    pub can_time_remap: bool,
    /// The selected clip plays through a time remap, which Time Remapping's
    /// check mark shows.
    pub remapped: bool,
    /// Whether the selection carries audio for Audio Gain.
    pub has_audio: bool,
    pub can_extract_audio: bool,
    pub can_freeze_frame: bool,
    /// Whether the clip under the pointer has a parent to release.
    pub parented: bool,
    /// How the transition at `clip`'s right junction sits relative to the cut.
    pub transition_alignment: Option<TransitionAlignment>,
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
            grouped: state.selection_is_grouped(),
            enabled: state.selection_is_enabled(),
            can_time_remap: state.selected_can_ramp(),
            remapped: state.selection_is_remapped(),
            has_audio: !state.selected_audio_clips().is_empty(),
            can_extract_audio: state.selected_can_extract_audio(),
            can_freeze_frame: state.selected_can_freeze_frame(),
            parented: state
                .ui_clip_ref(ClipId::from_raw(clip))
                .is_some_and(|c| c.parent.is_some()),
            transition_alignment: state
                .ui_clip_ref(ClipId::from_raw(clip))
                .and_then(|c| c.transition.as_ref().map(|t| t.alignment)),
        }
    }
}

/// A clip menu, in the order it draws.
pub const CLIP_ITEMS: &[MenuItem<ClipMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-split",
        label: |_| "Split",
        when: always(),
        checked: None,
        run: |state, _| state.split_at_playhead(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-delete",
        label: |_| "Delete",
        when: always(),
        checked: None,
        run: |state, _| state.clear_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-duplicate",
        label: |_| "Duplicate",
        when: always(),
        checked: None,
        run: |state, _| state.duplicate_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-copy",
        label: |_| "Copy",
        when: always(),
        checked: None,
        run: |state, _| {
            state.copy_selected();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-rename-clip",
        label: |_| "Rename Clip",
        when: always(),
        checked: None,
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
        checked: None,
        run: |state, _| state.freeze_selected_frame(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-extract-audio",
        label: |_| "Extract Audio",
        when: |clip| clip.can_extract_audio,
        checked: None,
        run: |state, _| state.extract_selected_audio(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-link",
        label: |_| "Link Clips",
        when: |clip| clip.multi,
        checked: None,
        run: |state, _| state.link_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-unlink",
        label: |_| "Unlink Clips",
        when: |clip| clip.linked,
        checked: None,
        run: |state, _| state.unlink_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-enable",
        label: |_| "Enable",
        when: always(),
        checked: Some(|clip| clip.enabled),
        run: |state, _| state.toggle_enabled_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-group",
        label: |_| "Group",
        when: |clip| clip.multi,
        checked: None,
        run: |state, _| state.group_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-ungroup",
        label: |_| "Ungroup",
        when: |clip| clip.grouped,
        checked: None,
        run: |state, _| state.ungroup_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-release-parent",
        label: |_| "Release Parent",
        when: |clip| clip.parented,
        checked: None,
        // The clip stays where it is drawn at the playhead.
        run: |state, clip| {
            state.set_clip_parent(ClipId::from_raw(clip.clip), None, ParentKeep::Place)
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-speed-duration",
        label: |_| "Speed/Duration…",
        when: always(),
        checked: None,
        run: |state, _| {
            state.open_speed_duration();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-time-remap",
        label: |_| "Time Remapping",
        when: |clip| clip.can_time_remap,
        checked: Some(|clip| clip.remapped),
        run: |state, _| state.toggle_time_remap(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-audio-gain",
        label: |_| "Audio Gain…",
        when: |clip| clip.has_audio,
        checked: None,
        run: |state, _| {
            state.open_audio_gain();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-shift-selection",
        label: |_| "Shift Selection…",
        when: always(),
        checked: None,
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
        checked: None,
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
        label: |_| "Remove Transition",
        when: always(),
        checked: None,
        run: |state, clip| state.clear_clip_transition(ClipId::from_raw(clip.clip)),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-transition-align-center",
        label: |_| "Center at Cut",
        when: always(),
        checked: Some(|clip| clip.transition_alignment == Some(TransitionAlignment::Center)),
        run: |state, clip| {
            state.set_clip_transition_alignment(
                ClipId::from_raw(clip.clip),
                TransitionAlignment::Center,
            )
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-transition-align-start",
        label: |_| "Start at Cut",
        when: always(),
        checked: Some(|clip| clip.transition_alignment == Some(TransitionAlignment::Start)),
        run: |state, clip| {
            state.set_clip_transition_alignment(
                ClipId::from_raw(clip.clip),
                TransitionAlignment::Start,
            )
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-transition-align-end",
        label: |_| "End at Cut",
        when: always(),
        checked: Some(|clip| clip.transition_alignment == Some(TransitionAlignment::End)),
        run: |state, clip| {
            state.set_clip_transition_alignment(
                ClipId::from_raw(clip.clip),
                TransitionAlignment::End,
            )
        },
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
