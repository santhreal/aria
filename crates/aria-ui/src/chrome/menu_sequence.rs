//! The Sequence menu: everything the bar offers for the timeline itself.
//!
//! Moved out of [`super::menu_items`] unchanged. It was the largest of the six
//! tables by a wide margin — a menu of the sequence covers trimming, marks,
//! tracks, markers, links, framing, grade, mask, matte and transitions — and one
//! file per menu is what keeps the other five readable.
//!
//! [`super::menu_items::items_for`] still dispatches here, so the bar, the
//! palette and the tests all reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::AppState;
use crate::commands::reach;

use super::menu_items::{MenuItem, MenuItems};

static SEQUENCE_ITEMS: &[MenuItem] = &[
    MenuItem {
        id: "seq-fit",
        label: "Fit",
        run: |_, s, _| s.zoom_fit_sequence(),
    },
    MenuItem {
        id: "seq-fit-selection",
        label: "Fit Selection",
        run: |_, s, _| s.zoom_fit_selection(),
    },
    MenuItem {
        id: "seq-match-frame",
        label: "Match Frame",
        run: |_, s, _| {
            s.match_frame();
        },
    },
    MenuItem {
        id: "seq-source-insert",
        label: "Insert Source",
        run: |_, s, _| {
            if let Err(err) = s.cut_source_in(crate::types::EditMode::Ripple) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-source-overwrite",
        label: "Overwrite Source",
        run: |_, s, _| {
            if let Err(err) = s.cut_source_in(crate::types::EditMode::Overwrite) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-in",
        label: "Zoom In",
        run: |_, s, _| {
            s.set_zoom(s.zoom * crate::transport::ZOOM_STEP, true);
        },
    },
    MenuItem {
        id: "seq-out",
        label: "Zoom Out",
        run: |_, s, _| {
            s.set_zoom(s.zoom / crate::transport::ZOOM_STEP, true);
        },
    },
    MenuItem {
        id: "seq-v",
        label: "Add Video Track",
        run: |_, s, _| {
            let n = s
                .store
                .sequence
                .tracks
                .iter()
                .filter(|t| matches!(t.kind, crate::types::UiTrackKind::Video))
                .count()
                + 1;
            if let Err(err) = s.add_track(aria_models::TrackKind::Video, format!("V{n}")) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-a",
        label: "Add Audio Track",
        run: |_, s, _| {
            let n = s
                .store
                .sequence
                .tracks
                .iter()
                .filter(|t| matches!(t.kind, crate::types::UiTrackKind::Audio))
                .count()
                + 1;
            if let Err(err) = s.add_track(aria_models::TrackKind::Audio, format!("A{n}")) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-adjustment",
        label: "Add Adjustment Layer",
        run: |_, s, _| {
            if let Err(err) = s.add_adjustment_layer() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-marker",
        label: "Add Marker",
        run: |_, s, _| {
            if let Err(err) = s.add_marker_at_playhead() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-mark-in",
        label: "Mark In",
        run: |_, s, _| s.set_mark_in(),
    },
    MenuItem {
        id: "seq-mark-out",
        label: "Mark Out",
        run: |_, s, _| s.set_mark_out(),
    },
    MenuItem {
        id: "seq-clear-in",
        label: "Clear In",
        run: |_, s, _| s.clear_mark_in(),
    },
    MenuItem {
        id: "seq-clear-out",
        label: "Clear Out",
        run: |_, s, _| s.clear_mark_out(),
    },
    MenuItem {
        id: "seq-ripple-in",
        label: "Ripple Trim In",
        run: |_, s, _| {
            if let Err(err) = s.trim_selected_to_playhead(true) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-ripple-out",
        label: "Ripple Trim Out",
        run: |_, s, _| {
            if let Err(err) = s.trim_selected_to_playhead(false) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-marker-remove",
        label: "Delete Marker at Playhead",
        run: |_, s, _| {
            if let Err(err) = s.remove_marker_at_playhead() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-marker-rename",
        label: "Rename Marker at Playhead",
        run: |_, s, _| s.begin_rename_marker_at_playhead(),
    },
    MenuItem {
        id: "seq-marker-next",
        label: "Next Marker",
        run: |_, s, _| s.go_to_next_marker(),
    },
    MenuItem {
        id: "seq-marker-prev",
        label: "Previous Marker",
        run: |_, s, _| s.go_to_prev_marker(),
    },
    MenuItem {
        id: "seq-link",
        label: "Link Clips",
        run: |_, s, _| {
            if let Err(err) = s.link_selected() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-unlink",
        label: "Unlink Clips",
        run: |_, s, _| {
            if let Err(err) = s.unlink_selected() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-extract-audio",
        label: "Extract Audio",
        run: |_, s, _| {
            if let Err(err) = s.extract_selected_audio() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-freeze",
        label: "Freeze Frame",
        run: |_, s, _| {
            if let Err(err) = s.freeze_selected_frame() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-flip-h",
        label: "Flip Horizontal",
        run: |_, s, _| {
            if let Err(err) = s.toggle_selected_flip_h() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-flip-v",
        label: "Flip Vertical",
        run: |_, s, _| {
            if let Err(err) = s.toggle_selected_flip_v() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-crop-reset",
        label: "Reset Framing",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_crop() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-grade-reset",
        label: "Reset Grade",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_adjustments() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-mask-clear",
        label: "Remove Mask",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_mask() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-chroma-clear",
        label: "Remove Chroma Key",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_chroma() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-matte-luma",
        label: "Track Matte from Luma Above",
        run: |_, s, _| {
            if let Err(err) = s.set_selected_matte_source(aria_models::MatteSource::Luma) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-matte-alpha",
        label: "Track Matte from Alpha Above",
        run: |_, s, _| {
            if let Err(err) = s.set_selected_matte_source(aria_models::MatteSource::Alpha) {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-matte-clear",
        label: "Remove Track Matte",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_matte() {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-transition-crossfade",
        label: "Add Cross Fade at Cut",
        run: |_, s, _| {
            if let Err(err) = s.set_selected_transition("crossfade") {
                s.report_error(&err);
            }
        },
    },
    MenuItem {
        id: "seq-transition-clear",
        label: "Remove Transition at Cut",
        run: |_, s, _| {
            if let Err(err) = s.clear_selected_transition() {
                s.report_error(&err);
            }
        },
    },
    // Rows whose action had no operator path at all before. The bodies live
    // in `crate::commands::reach`, which the palette rows
    // call too, so the bar and the palette cannot drift into doing two
    // different things. The labels match the palette's exactly, which is what
    // makes the palette offer each row once instead of twice.
    MenuItem {
        id: "seq-lane-shift-later",
        label: "Shift lane one second later",
        run: |_, s, _| reach::shift_lane(s, true),
    },
    MenuItem {
        id: "seq-lane-shift-earlier",
        label: "Shift lane one second earlier",
        run: |_, s, _| reach::shift_lane(s, false),
    },
    MenuItem {
        id: "seq-ripple-insert",
        label: "Insert pool item at playhead",
        run: |_, s, _| reach::ripple_insert(s),
    },
    MenuItem {
        id: "seq-marker-snap",
        label: "Move nearest marker to playhead",
        run: |_, s, _| reach::snap_marker(s),
    },
    MenuItem {
        id: "seq-effect-reset",
        label: "Reset effect parameters",
        run: |_, s, _| reach::reset_effects(s),
    },
    MenuItem {
        id: "seq-canvas-background",
        label: "Reset canvas background",
        run: |_, s, _| reach::reset_canvas_background(s),
    },
    MenuItem {
        id: "seq-duck-amount",
        label: "Cycle duck amount",
        run: |_, s, _| s.cycle_duck_amount(),
    },
    MenuItem {
        id: "seq-duck-attack",
        label: "Cycle duck attack",
        run: |_, s, _| s.cycle_duck_attack(),
    },
    MenuItem {
        id: "seq-duck-release",
        label: "Cycle duck release",
        run: |_, s, _| s.cycle_duck_release(),
    },
    MenuItem {
        id: "seq-nest",
        label: "Nest Selection",
        run: |_, s, _| reach::nest_selection(s),
    },
    MenuItem {
        id: "seq-nest-open",
        label: "Open Nested Sequence",
        run: |_, s, _| reach::open_selected_nest(s),
    },
    MenuItem {
        id: "seq-nest-leave",
        label: "Leave Nested Sequence",
        run: |_, s, _| reach::leave_sequence(s),
    },
    MenuItem {
        id: "seq-nest-place",
        label: "Place Nest At Playhead",
        run: |_, s, _| reach::place_selected_nest(s),
    },
    MenuItem {
        id: "seq-bake-clip",
        label: "Render and replace",
        run: |_, s, _| {
            if let Err(err) = s.render_and_replace() {
                s.status_message = Some(err);
            }
        },
    },
    MenuItem {
        id: "seq-bake-restore",
        label: "Restore unrendered",
        run: |_, s, _| {
            if let Err(err) = s.restore_unrendered() {
                s.status_message = Some(err);
            }
        },
    },
];

pub(super) fn sequence_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(SEQUENCE_ITEMS)
}
