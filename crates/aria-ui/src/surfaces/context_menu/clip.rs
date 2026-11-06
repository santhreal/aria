//! The rows a clip's own menu draws, and the junction menu beside it.
//!
//! Defines [`ClipMenu`] alongside the clip and transition row tables that read
//! it. A clip row that runs a command the menu bar also offers mirrors the bar
//! row: it draws the bar row's label and check mark and is offered while the
//! bar row's guard lets it run, so the two menus name and guard one command
//! one way.

use aria_engine::EngineError;
use aria_models::{ClipId, ParentKeep, TransitionAlignment};

use crate::app::AppState;
use crate::chrome::menu_clip::{
    AUDIO_GAIN, ENABLE, EXTRACT_AUDIO, FREEZE, GROUP, LINK, RENAME, SPEED_DURATION, TIME_REMAP,
    UNGROUP, UNLINK,
};
use crate::chrome::menu_edit::{CLEAR, COPY, CUT, DUPLICATE, RIPPLE_DELETE};
use crate::chrome::menu_items::MenuItem as BarRow;
use crate::chrome::menu_sequence::ADD_EDIT;
use crate::types::{ActivePanel, RenameTarget};

use super::{MenuItem, MenuRow, always};

/// The menu bar rows the clip menu mirrors, in the order [`ClipMenu`] records
/// them.
const MIRRORED: [&BarRow; 17] = [
    &CUT,
    &COPY,
    &DUPLICATE,
    &CLEAR,
    &RIPPLE_DELETE,
    &ADD_EDIT,
    &ENABLE,
    &LINK,
    &UNLINK,
    &GROUP,
    &UNGROUP,
    &SPEED_DURATION,
    &TIME_REMAP,
    &FREEZE,
    &AUDIO_GAIN,
    &EXTRACT_AUDIO,
    &RENAME,
];
const _: () = assert!(MIRRORED.len() <= u32::BITS as usize);

/// The clip facts a row reads, gathered once when the menu opens.
#[derive(Debug, Clone, Copy)]
pub struct ClipMenu {
    /// The clip under the pointer.
    pub clip: u64,
    /// Bit `i` is set while the bar row `MIRRORED[i]` would run on the
    /// selection.
    run_bits: u32,
    /// Bit `i` is set while the bar row `MIRRORED[i]` draws its check mark.
    check_bits: u32,
    /// Whether the clip under the pointer has a parent to release.
    pub parented: bool,
    /// Frames the clip under the pointer sits from its linked partner, while
    /// the link has drifted out of sync.
    pub drift: Option<i64>,
    /// How the transition at `clip`'s right junction sits relative to the cut.
    pub transition_alignment: Option<TransitionAlignment>,
}

impl ClipMenu {
    /// Read the clip facts a menu would gather when it opens over `clip`.
    ///
    /// One owner for the reading, so the panel that draws the rows and the census
    /// that presses them cannot disagree about what a row was offered against.
    pub fn of(state: &AppState, clip: u64) -> Self {
        let (mut run_bits, mut check_bits) = (0, 0);
        for (at, row) in MIRRORED.iter().enumerate() {
            if row.blocked_by(state).is_none() {
                run_bits |= 1 << at;
            }
            if row.is_checked(state) {
                check_bits |= 1 << at;
            }
        }
        let ui = state.ui_clip_ref(ClipId::from_raw(clip));
        Self {
            clip,
            run_bits,
            check_bits,
            parented: ui.is_some_and(|c| c.parent.is_some()),
            drift: ui.and_then(|c| c.drift),
            transition_alignment: ui.and_then(|c| c.transition.as_ref().map(|t| t.alignment)),
        }
    }

    /// Whether the bar row `bar` would run on the selection the menu opened
    /// over. A row the menu does not mirror never runs.
    pub fn runs(&self, bar: &BarRow) -> bool {
        Self::holds(self.run_bits, bar)
    }

    /// Whether the bar row `bar` draws its check mark against the selection the
    /// menu opened over.
    pub fn checks(&self, bar: &BarRow) -> bool {
        Self::holds(self.check_bits, bar)
    }

    fn holds(bits: u32, bar: &BarRow) -> bool {
        MIRRORED
            .iter()
            .position(|row| row.id == bar.id)
            .is_some_and(|at| bits & (1 << at) != 0)
    }
}

/// A clip menu, in the order it draws: the clipboard and deletions, the
/// edits, the selection's links, its time, its sound, its label, then its
/// name and properties.
pub const CLIP_ITEMS: &[MenuItem<ClipMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-cut",
        label: |_| CUT.label,
        when: |clip| clip.runs(&CUT),
        checked: None,
        run: |state, _| state.cut_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-copy",
        label: |_| COPY.label,
        when: |clip| clip.runs(&COPY),
        checked: None,
        run: |state, _| {
            state.copy_selected();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-duplicate",
        label: |_| DUPLICATE.label,
        when: |clip| clip.runs(&DUPLICATE),
        checked: None,
        run: |state, _| state.duplicate_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-delete",
        label: |_| CLEAR.label,
        when: |clip| clip.runs(&CLEAR),
        checked: None,
        run: |state, _| state.clear_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-ripple-delete",
        label: |_| RIPPLE_DELETE.label,
        when: |clip| clip.runs(&RIPPLE_DELETE),
        checked: None,
        run: |state, _| state.ripple_delete_selected(),
    }),
    MenuItem::Rule,
    MenuItem::Row(MenuRow {
        id: "ctx-split",
        label: |_| ADD_EDIT.label,
        when: |clip| clip.runs(&ADD_EDIT),
        checked: None,
        run: |state, _| state.add_edit(false),
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
    MenuItem::Rule,
    MenuItem::Row(MenuRow {
        id: "ctx-enable",
        label: |_| ENABLE.label,
        when: |clip| clip.runs(&ENABLE),
        checked: Some(|clip| clip.checks(&ENABLE)),
        run: |state, _| state.toggle_enabled_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-link",
        label: |_| LINK.label,
        when: |clip| clip.runs(&LINK),
        checked: None,
        run: |state, _| state.link_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-unlink",
        label: |_| UNLINK.label,
        when: |clip| clip.runs(&UNLINK),
        checked: None,
        run: |state, _| state.unlink_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-move-into-sync",
        label: |_| "Move into Sync",
        when: |clip| clip.drift.is_some(),
        checked: None,
        run: |state, clip| state.move_into_sync(ClipId::from_raw(clip.clip)),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-group",
        label: |_| GROUP.label,
        when: |clip| clip.runs(&GROUP),
        checked: None,
        run: |state, _| state.group_selected(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-ungroup",
        label: |_| UNGROUP.label,
        when: |clip| clip.runs(&UNGROUP),
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
    MenuItem::Rule,
    MenuItem::Row(MenuRow {
        id: "ctx-speed-duration",
        label: |_| SPEED_DURATION.label,
        when: |clip| clip.runs(&SPEED_DURATION),
        checked: None,
        run: |state, _| {
            state.open_speed_duration();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-time-remap",
        label: |_| TIME_REMAP.label,
        when: |clip| clip.runs(&TIME_REMAP),
        checked: Some(|clip| clip.checks(&TIME_REMAP)),
        run: |state, _| state.toggle_time_remap(),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-freeze",
        label: |_| FREEZE.label,
        when: |clip| clip.runs(&FREEZE),
        checked: None,
        run: |state, _| state.freeze_selected_frame(),
    }),
    MenuItem::Rule,
    MenuItem::Row(MenuRow {
        id: "ctx-audio-gain",
        label: |_| AUDIO_GAIN.label,
        when: |clip| clip.runs(&AUDIO_GAIN),
        checked: None,
        run: |state, _| {
            state.open_audio_gain();
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-extract-audio",
        label: |_| EXTRACT_AUDIO.label,
        when: |clip| clip.runs(&EXTRACT_AUDIO),
        checked: None,
        run: |state, _| state.extract_selected_audio(),
    }),
    MenuItem::Rule,
    MenuItem::Labels,
    MenuItem::Rule,
    MenuItem::Row(MenuRow {
        id: "ctx-rename-clip",
        label: |_| RENAME.label,
        when: |clip| clip.runs(&RENAME),
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

/// A junction menu: take the blend off, then where it sits on the cut, then
/// every blend that could go on.
pub const TRANSITION_ITEMS: &[MenuItem<ClipMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-transition-remove",
        label: |_| "Remove Transition",
        when: always(),
        checked: None,
        run: |state, clip| state.clear_clip_transition(ClipId::from_raw(clip.clip)),
    }),
    MenuItem::Rule,
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
    MenuItem::Rule,
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
