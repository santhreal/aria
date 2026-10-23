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
        id: "pal-extract-audio",
        label: "Extract Audio",
        run: |state| {
            if let Err(err) = state.extract_selected_audio() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-freeze",
        label: "Freeze Frame",
        run: |state| {
            if let Err(err) = state.freeze_selected_frame() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-link",
        label: "Link Clips",
        run: |state| {
            if let Err(err) = state.link_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-unlink",
        label: "Unlink Clips",
        run: |state| {
            if let Err(err) = state.unlink_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-flip-h",
        label: "Flip Horizontal",
        run: |state| {
            if let Err(err) = state.toggle_selected_flip_h() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-flip-v",
        label: "Flip Vertical",
        run: |state| {
            if let Err(err) = state.toggle_selected_flip_v() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-crop-reset",
        label: "Reset Framing",
        run: |state| {
            if let Err(err) = state.clear_selected_crop() {
                state.report_error(&err);
            }
        },
    },
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
        id: "pal-grade-reset",
        label: "Reset Grade",
        run: |state| {
            if let Err(err) = state.clear_selected_adjustments() {
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
    CommandEntry {
        id: "pal-mask-clear",
        label: "Remove Mask",
        run: |state| {
            if let Err(err) = state.clear_selected_mask() {
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
        id: "pal-chroma-clear",
        label: "Remove Chroma Key",
        run: |state| {
            if let Err(err) = state.clear_selected_chroma() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-matte-luma",
        label: "Track Matte from Luma Above",
        run: |state| {
            if let Err(err) = state.set_selected_matte_source(aria_models::MatteSource::Luma) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-matte-alpha",
        label: "Track Matte from Alpha Above",
        run: |state| {
            if let Err(err) = state.set_selected_matte_source(aria_models::MatteSource::Alpha) {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-matte-clear",
        label: "Remove Track Matte",
        run: |state| {
            if let Err(err) = state.clear_selected_matte() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-transition-crossfade",
        label: "Add Cross Fade at Cut",
        run: |state| {
            if let Err(err) = state.set_selected_transition("crossfade") {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-transition-clear",
        label: "Remove Transition at Cut",
        run: |state| {
            if let Err(err) = state.clear_selected_transition() {
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
    CommandEntry {
        id: "pal-save-template",
        label: "Save as Template",
        run: |state| state.pick_and_save_template(),
    },
    CommandEntry {
        id: "pal-use-template",
        label: "Use Template",
        run: |state| state.pick_and_apply_template(),
    },
    CommandEntry {
        id: "pal-marker-remove",
        label: "Delete Marker at Playhead",
        run: |state| {
            if let Err(err) = state.remove_marker_at_playhead() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-marker-next",
        label: "Next Marker",
        run: |state| state.go_to_next_marker(),
    },
    CommandEntry {
        id: "pal-marker-prev",
        label: "Previous Marker",
        run: |state| state.go_to_prev_marker(),
    },
    CommandEntry {
        id: "pal-clip-enable",
        label: "Enable",
        run: |state| {
            if let Err(err) = state.toggle_enabled_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-group",
        label: "Group",
        run: |state| {
            if let Err(err) = state.group_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-ungroup",
        label: "Ungroup",
        run: |state| {
            if let Err(err) = state.ungroup_selected() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-speed-duration",
        label: "Speed/Duration…",
        run: |state| state.open_speed_duration(),
    },
    CommandEntry {
        id: "pal-time-remap",
        label: "Time Remapping",
        run: |state| {
            if let Err(err) = state.toggle_time_remap() {
                state.report_error(&err);
            }
        },
    },
    CommandEntry {
        id: "pal-audio-gain",
        label: "Audio Gain…",
        run: |state| state.open_audio_gain(),
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
