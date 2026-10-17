//! Editor-route bindings for the transport, the timeline, and the project.
//!
//! One of the three tables [`super::bindings::SHIPPED_BINDINGS`] joins. The rows
//! follow `docs/spec/keyboard.md` section by section, so a chord is found under
//! the heading the spec lists it under.

use super::bind;
use super::binding_actions::{
    clear_selected, gain, new_project, open_settings, redo, ripple_delete, trim_edges, zoom_in,
    zoom_out,
};
use super::{Binding, Chord};
use crate::app::DialogKind;

/// Transport, marking, editing, timeline view, and the application chords.
pub(super) const EDIT_ROWS: &[Binding] = &[
    // Transport.
    bind!("key-play", Chord::plain("space"), Editor, |state| state.space(), after: Play),
    bind!("key-play-work-area", Chord::shift("space"), Editor, |state| state.shift_space(), after: Play),
    bind!(
        "key-render-in-to-out",
        Chord::plain("enter"),
        Editor,
        |state| {
            if let Err(err) = state.render_in_to_out() {
                state.status_message = Some(err);
            }
        }
    ),
    bind!("key-loop", Chord::ctrl_shift("l"), Editor, |state| state
        .toggle_loop()),
    bind!("key-shuttle-back", Chord::plain("j"), Editor, |state| state.shuttle_key_down(-1, false), after: Play),
    bind!("key-shuttle-stop", Chord::plain("k"), Editor, |state| state
        .k_down(false)),
    bind!("key-shuttle-forward", Chord::plain("l"), Editor, |state| state.shuttle_key_down(1, false), after: Play),
    bind!("key-shuttle-slower", Chord::shift("j"), Editor, |state| state.step_shuttle_fine(-1), after: Play),
    bind!("key-shuttle-faster", Chord::shift("l"), Editor, |state| state.step_shuttle_fine(1), after: Play),
    bind!("key-edit-prev", Chord::plain("up"), Editor, |state| state
        .go_to_edit(false, false)),
    bind!("key-edit-next", Chord::plain("down"), Editor, |state| state
        .go_to_edit(true, false)),
    bind!("key-edit-prev-all", Chord::shift("up"), Editor, |state| {
        state.go_to_edit(false, true)
    }),
    bind!("key-edit-next-all", Chord::shift("down"), Editor, |state| {
        state.go_to_edit(true, true)
    }),
    // Marking. Shift goes to the mark, Ctrl+Shift clears it.
    bind!("key-mark-in", Chord::plain("i"), Editor, |state| state.mark_in_here(), after: Preview),
    bind!("key-mark-in-go", Chord::shift("i"), Editor, |state| state.go_to_in(), after: Preview),
    bind!("key-mark-in-clear", Chord::ctrl_shift("i"), Editor, |state| state.clear_mark_in_here(), after: Preview),
    bind!("key-mark-out", Chord::plain("o"), Editor, |state| state.mark_out_here(), after: Preview),
    bind!("key-mark-out-go", Chord::shift("o"), Editor, |state| state.go_to_out(), after: Preview),
    bind!("key-mark-out-clear", Chord::ctrl_shift("o"), Editor, |state| state.clear_mark_out_here(), after: Preview),
    bind!("key-mark-clear", Chord::ctrl_shift("x"), Editor, |state| state.clear_marks_here(), after: Preview),
    bind!("key-mark-clip", Chord::plain("x"), Editor, |state| state.mark_clip_at_playhead(), after: Preview),
    bind!("key-marker-add", Chord::plain("m"), Editor, |state| {
        if let Err(err) = state.add_or_edit_marker() {
            state.report_error(&err);
        }
    }),
    bind!("key-marker-next", Chord::shift("m"), Editor, |state| state
        .go_to_next_marker()),
    bind!("key-marker-prev", Chord::ctrl_shift("m"), Editor, |state| {
        state.go_to_prev_marker()
    }),
    bind!("key-marker-remove", Chord::ctrl_alt("m"), Editor, |state| {
        if let Err(err) = state.remove_marker_at_playhead() {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-marker-clear-all",
        Chord::ctrl_shift_alt("m"),
        Editor,
        |state| {
            if let Err(err) = state.clear_all_markers() {
                state.report_error(&err);
            }
        }
    ),
    bind!("key-match-frame", Chord::plain("f"), Editor, |state| {
        state.match_frame();
    }),
    // Editing.
    bind!("key-source-insert", Chord::plain(","), Editor, |state| {
        if let Err(err) = state.cut_source_in(crate::types::EditMode::Ripple) {
            state.report_error(&err);
        }
    }, after: Preview),
    bind!("key-source-overwrite", Chord::plain("."), Editor, |state| {
        if let Err(err) = state.cut_source_in(crate::types::EditMode::Overwrite) {
            state.report_error(&err);
        }
    }, after: Preview),
    bind!("key-lift", Chord::plain(";"), Editor, |state| {
        if let Err(err) = state.remove_marked_range(false) {
            state.report_error(&err);
        }
    }),
    bind!("key-extract", Chord::plain("'"), Editor, |state| {
        if let Err(err) = state.remove_marked_range(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-split", Chord::ctrl("k"), Editor, |state| {
        if let Err(err) = state.add_edit(false) {
            state.report_error(&err);
        }
    }),
    bind!("key-split-all", Chord::ctrl_shift("k"), Editor, |state| {
        if let Err(err) = state.add_edit(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-trim-in", Chord::plain("q"), Editor, |state| {
        if let Err(err) = state.ripple_trim_to_playhead(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-trim-out", Chord::plain("w"), Editor, |state| {
        if let Err(err) = state.ripple_trim_to_playhead(false) {
            state.report_error(&err);
        }
    }),
    bind!("key-trim-in-keep-gap", Chord::alt("["), Editor, |state| {
        if let Err(err) = state.trim_to_playhead(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-trim-out-keep-gap", Chord::alt("]"), Editor, |state| {
        if let Err(err) = state.trim_to_playhead(false) {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-delete",
        Chord::plain("backspace"),
        Editor,
        clear_selected
    ),
    bind!(
        "key-delete-forward",
        Chord::plain("delete"),
        Editor,
        clear_selected
    ),
    bind!(
        "key-ripple-delete",
        Chord::shift("backspace"),
        Editor,
        ripple_delete
    ),
    bind!(
        "key-ripple-delete-forward",
        Chord::shift("delete"),
        Editor,
        ripple_delete
    ),
    bind!("key-trim-back", Chord::ctrl("left"), Editor, |state| {
        trim_edges(state, -1)
    }),
    bind!("key-trim-forward", Chord::ctrl("right"), Editor, |state| {
        trim_edges(state, 1)
    }),
    bind!(
        "key-trim-back-fast",
        Chord::ctrl_shift("left"),
        Editor,
        |state| trim_edges(state, -5)
    ),
    bind!(
        "key-trim-forward-fast",
        Chord::ctrl_shift("right"),
        Editor,
        |state| trim_edges(state, 5)
    ),
    bind!("key-transition", Chord::ctrl("t"), Editor, |state| {
        if let Err(err) = state.add_default_transition(aria_models::TransitionMedia::Picture) {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-audio-transition",
        Chord::ctrl_shift("t"),
        Editor,
        |state| {
            if let Err(err) = state.add_default_transition(aria_models::TransitionMedia::Audio) {
                state.report_error(&err);
            }
        }
    ),
    bind!(
        "key-select-at-playhead",
        Chord::plain("d"),
        Editor,
        |state| state.select_at_playhead()
    ),
    bind!("key-select-all", Chord::ctrl("a"), Editor, |state| state
        .select_all_unlocked()),
    bind!(
        "key-deselect-all",
        Chord::ctrl_shift("a"),
        Editor,
        |state| state.clear_selection()
    ),
    bind!("key-snap", Chord::plain("s"), Editor, |state| state
        .toggle_snapping()),
    bind!("key-link", Chord::ctrl("l"), Editor, |state| {
        if let Err(err) = state.toggle_link_selected() {
            state.report_error(&err);
        }
    }),
    bind!("key-gain-up", Chord::plain("]"), Editor, |state| gain(
        state, 1.0
    )),
    bind!("key-gain-down", Chord::plain("["), Editor, |state| gain(
        state, -1.0
    )),
    bind!("key-gain-up-fast", Chord::shift("]"), Editor, |state| gain(
        state, 6.0
    )),
    bind!("key-gain-down-fast", Chord::shift("["), Editor, |state| {
        gain(state, -6.0)
    }),
    // Keyframes.
    bind!(
        "key-keyframe-prev",
        Chord::shift_alt("left"),
        Editor,
        |state| state.go_to_keyframe(false)
    ),
    bind!(
        "key-keyframe-next",
        Chord::shift_alt("right"),
        Editor,
        |state| state.go_to_keyframe(true)
    ),
    // Timeline view. Two chords reach zoom-in because a keyboard reports either.
    bind!("key-zoom-in", Chord::plain("="), Editor, zoom_in),
    bind!("key-zoom-in-plus", Chord::plain("+"), Editor, zoom_in),
    bind!("key-zoom-out", Chord::plain("-"), Editor, zoom_out),
    bind!("key-fit", Chord::plain("\\"), Editor, |state| state
        .zoom_fit_sequence()),
    bind!("key-fit-selection", Chord::shift("f"), Editor, |state| {
        state.zoom_fit_selection()
    }),
    // Premiere's Shift+Z is the same fit; Z alone stays the zoom tool.
    bind!("key-fit-selection-z", Chord::shift("z"), Editor, |state| {
        state.zoom_fit_selection()
    }),
    bind!("key-tracks-taller", Chord::ctrl("="), Editor, |state| {
        if let Err(err) = state.step_track_heights(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-tracks-shorter", Chord::ctrl("-"), Editor, |state| {
        if let Err(err) = state.step_track_heights(false) {
            state.report_error(&err);
        }
    }),
    // Application.
    bind!("key-undo", Chord::ctrl("z"), Editor, |state| {
        state.undo();
    }),
    bind!("key-redo", Chord::ctrl_shift("z"), Editor, redo),
    bind!("key-redo-y", Chord::ctrl("y"), Editor, redo),
    bind!("key-new", Chord::ctrl_alt("n"), Editor, new_project),
    bind!(
        "key-new-sequence",
        Chord::ctrl("n"),
        Editor,
        crate::commands::reach::new_sequence
    ),
    bind!("key-open", Chord::ctrl("o"), Editor, |state| state
        .request_pick_project()),
    bind!("key-close", Chord::ctrl_shift("w"), Editor, |state| state
        .close_project()),
    bind!("key-save", Chord::ctrl("s"), Editor, |state| {
        if let Err(err) = state.save_project() {
            state.report_error(&err);
        }
    }),
    bind!("key-save-as", Chord::ctrl_shift("s"), Editor, |state| state
        .save_project_as()),
    bind!("key-save-copy", Chord::ctrl_alt("s"), Editor, |state| state
        .save_project_copy()),
    bind!("key-import", Chord::ctrl("i"), Editor, |state| state
        .pick_and_import()),
    bind!("key-export", Chord::ctrl("m"), Editor, |state| state
        .open_dialog(DialogKind::Export)),
    bind!(
        "key-export-frame",
        Chord::ctrl_shift("e"),
        Editor,
        |state| state.pick_and_export_frame()
    ),
    // Clip menu.
    bind!("key-clip-enable", Chord::shift("e"), Editor, |state| {
        if let Err(err) = state.toggle_enabled_selected() {
            state.report_error(&err);
        }
    }),
    bind!("key-clip-group", Chord::ctrl("g"), Editor, |state| {
        if let Err(err) = state.group_selected() {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-clip-ungroup",
        Chord::ctrl_shift("g"),
        Editor,
        |state| {
            if let Err(err) = state.ungroup_selected() {
                state.report_error(&err);
            }
        }
    ),
    bind!("key-speed-duration", Chord::ctrl("r"), Editor, |state| {
        state.open_speed_duration()
    }),
    bind!("key-time-remap", Chord::ctrl_alt("t"), Editor, |state| {
        if let Err(err) = state.toggle_time_remap() {
            state.report_error(&err);
        }
    }),
    bind!("key-audio-gain", Chord::plain("g"), Editor, |state| state
        .open_audio_gain()),
    bind!("key-cut", Chord::ctrl("x"), Editor, |state| {
        if let Err(err) = state.cut_selected() {
            state.report_error(&err);
        }
    }),
    bind!("key-copy", Chord::ctrl("c"), Editor, |state| state
        .copy_selected()),
    bind!("key-paste", Chord::ctrl("v"), Editor, |state| {
        if let Err(err) = state.paste_clipboard() {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-paste-insert",
        Chord::ctrl_shift("v"),
        Editor,
        |state| {
            if let Err(err) = state.paste_insert() {
                state.report_error(&err);
            }
        }
    ),
    bind!(
        "key-paste-attributes",
        Chord::ctrl_alt("v"),
        Editor,
        |state| {
            if let Err(err) = state.paste_attributes() {
                state.report_error(&err);
            }
        }
    ),
    bind!("key-duplicate", Chord::ctrl("d"), Editor, |state| {
        if let Err(err) = state.duplicate_selected() {
            state.report_error(&err);
        }
    }),
    bind!("key-find", Chord::ctrl("f"), Editor, |state| state
        .find_in_project()),
    bind!("key-palette", Chord::ctrl_shift("p"), Editor, |state| state
        .open_dialog(DialogKind::Palette)),
    // A lone Alt opens the bar as well; that press is no chord, so the shell
    // detects it in `keys::lone_alt` rather than here.
    bind!("key-menu-bar", Chord::plain("f10"), Editor, |state| state
        .toggle_menu_from_keyboard()),
    bind!("key-history", Chord::ctrl_shift("h"), Editor, |state| state
        .focus_dock_panel(crate::dock::PANEL_HISTORY)),
    bind!(
        "key-keyboard-shortcuts",
        Chord::ctrl_alt("k"),
        Editor,
        |state| state.open_keyboard_shortcuts()
    ),
    bind!("key-settings", Chord::ctrl(","), Editor, open_settings),
    bind!("key-quit", Chord::ctrl("q"), Editor, |state| state
        .request_quit()),
];
