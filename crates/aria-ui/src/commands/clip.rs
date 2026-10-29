//! Palette rows for the selected clip, its lane, and the markers around it.
//!
//! One of the three tables [`super::BUILTIN_COMMANDS`] joins. Every row
//! here needs a selection or a lane, and refuses by staying inert when there is
//! none — the same refusal the inspector control gives.
//!
//! `CLIP_JOB_ROWS` is the exception and a separate table for it: a row that
//! starts a job needs the entity context, so it cannot sit beside rows that run
//! against a headless state.

use super::host::{CommandEntry, WindowedEntry};

/// A row that adds one mask shape to the selected clip's stack.
///
/// A macro rather than a loop over `MaskKind::ALL`: a row's body is a function
/// pointer, and a const table cannot build one per shape at run time.
/// `every_mask_shape_has_a_palette_row` reads the enum, so a shape added there
/// without a row here is red.
macro_rules! add_mask_row {
    ($id:literal, $label:literal, $kind:ident) => {
        CommandEntry {
            id: $id,
            label: $label,
            run: |state| {
                if let Err(err) = state.add_selected_mask(aria_models::MaskKind::$kind) {
                    state.report_error(&err);
                }
            },
        }
    };
}

/// Rows that act on the selected clip, its lane, or a marker.
pub(super) const CLIP_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-motion-blur",
        label: "Toggle Motion Blur",
        run: |state| {
            if let Err(err) = state.toggle_selected_motion_blur() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-denoise",
        label: "Toggle Denoise",
        run: |state| {
            if let Err(err) = state.toggle_selected_denoise() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-pitch-lock",
        label: "Toggle Pitch Lock",
        run: |state| {
            if let Err(err) = state.toggle_selected_pitch_lock() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-mask-invert",
        label: "Invert Mask",
        run: |state| {
            if let Err(err) = state.toggle_active_mask_invert() {
                state.report_error(&err);
            }
        },
    },
    add_mask_row!("pal-mask-add-linear", "Add Linear Mask", Linear),
    add_mask_row!("pal-mask-add-mirror", "Add Mirror Mask", Mirror),
    add_mask_row!("pal-mask-add-circle", "Add Circle Mask", Circle),
    add_mask_row!("pal-mask-add-rectangle", "Add Rectangle Mask", Rectangle),
    add_mask_row!("pal-mask-add-heart", "Add Heart Mask", Heart),
    add_mask_row!("pal-mask-add-star", "Add Star Mask", Star),
    add_mask_row!("pal-mask-add-path", "Add Path Mask", Path),
    CommandEntry {
        id: "pal-chroma-green",
        label: "Key Green Screen",
        run: |state| {
            if let Err(err) = state.set_selected_chroma_color([0, 177, 64]) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-slot-toggle",
        label: "Mark as Template Slot",
        run: |state| {
            if let Err(err) = state.toggle_selected_replaceable() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-text-editable",
        label: "Text Editable in Template",
        run: |state| {
            if let Err(err) = state.toggle_selected_text_editable() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-key-opacity",
        label: "Keyframe Opacity at Playhead",
        run: |state| {
            if let Err(err) = state.add_keyframe_at_playhead(aria_models::ClipParam::Opacity) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-key-position",
        label: "Keyframe Position at Playhead",
        run: |state| {
            if let Err(err) = state.add_keyframe_at_playhead(aria_models::ClipParam::Position) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-key-scale",
        label: "Keyframe Scale at Playhead",
        run: |state| {
            if let Err(err) = state.add_keyframe_at_playhead(aria_models::ClipParam::Scale) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-key-remove",
        label: "Remove Opacity Keyframe at Playhead",
        run: |state| {
            if let Err(err) = state.remove_keyframe_at_playhead(aria_models::ClipParam::Opacity) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-shift-selection",
        label: "Shift Selection by Frames…",
        run: |state| {
            state.begin_rename(crate::types::RenameTarget::ShiftFrames);
        },
    },
    CommandEntry {
        id: "pal-track-up",
        label: "Move Track Up",
        run: |state| {
            if let Some(track) = state.selected_track()
                && let Err(err) = state.move_track_up(track)
            {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-track-down",
        label: "Move Track Down",
        run: |state| {
            if let Some(track) = state.selected_track()
                && let Err(err) = state.move_track_down(track)
            {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-track-solo",
        label: "Solo Track",
        run: |state| {
            if let Some(track) = state.selected_track()
                && let Err(err) = state.toggle_track_solo(track)
            {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-track-duck",
        label: "Toggle Track as Duck Source",
        run: |state| {
            if let Some(track) = state.selected_track()
                && let Err(err) = state.toggle_track_duck_source(track)
            {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-track-open-gap",
        label: "Open One-Second Gap at Playhead",
        run: |state| {
            if let Some(track) = state.selected_track() {
                let delta = state.store.sequence.frame_rate.nominal_fps();
                if let Err(err) = state.shift_clips_from_playhead(track, delta) {
                    state.report_error(&err);
                }
            }
        },
    },
    CommandEntry {
        id: "pal-track-close-gap",
        label: "Close One-Second Gap at Playhead",
        run: |state| {
            if let Some(track) = state.selected_track() {
                let delta = state.store.sequence.frame_rate.nominal_fps();
                if let Err(err) = state.shift_clips_from_playhead(track, -delta) {
                    state.report_error(&err);
                }
            }
        },
    },
];

/// The clip rows that start work instead of editing.
///
/// One of the groups [`super::BUILTIN_JOB_COMMANDS`] joins. A clip command that
/// spawns a job belongs beside the clip commands rather than with the detectors,
/// and the second table is what lets it: these are the rows that cannot run
/// without a window.
pub(super) const CLIP_JOB_ROWS: &[WindowedEntry] = &[WindowedEntry {
    id: "pal-track-mask",
    label: "Track Mask Forward from the Playhead",
    run: |state, cx| state.run_mask_track(cx),
}];
