//! Editor-route bindings for the transport, the timeline, and the project.
//!
//! One of the three tables [`super::bindings::SHIPPED_BINDINGS`] joins. Moved
//! here unchanged: the shipped table was one 543-line array, and a chord is
//! easier to find in a file named after what it does.

use super::bind;
use super::binding_actions::{
    delete_selected, new_project, open_settings, redo, zoom_in, zoom_out,
};
use super::{Binding, Chord};
use crate::app::DialogKind;

/// Transport, markers, zoom, editing, marks, source cut-ins, history, project.
pub(super) const EDIT_ROWS: &[Binding] = &[
    // Transport.
    bind!("key-play", Chord::plain("space"), Editor, |state| state.space(), after: Play),
    bind!("key-shuttle-back", Chord::plain("j"), Editor, |state| state.set_shuttle(-1), after: Play),
    bind!("key-shuttle-stop", Chord::plain("k"), Editor, |state| state
        .set_shuttle(0)),
    bind!("key-shuttle-forward", Chord::plain("l"), Editor, |state| state.set_shuttle(1), after: Play),
    // Markers.
    bind!("key-marker-add", Chord::plain("m"), Editor, |state| {
        if let Err(err) = state.add_marker_at_playhead() {
            state.report_error(&err);
        }
    }),
    bind!("key-marker-remove", Chord::alt("m"), Editor, |state| {
        if let Err(err) = state.remove_marker_at_playhead() {
            state.report_error(&err);
        }
    }),
    bind!("key-marker-next", Chord::shift("m"), Editor, |state| state
        .go_to_next_marker()),
    bind!("key-marker-prev", Chord::shift_alt("m"), Editor, |state| {
        state.go_to_prev_marker()
    }),
    // Zoom. Two chords reach zoom-in because a keyboard reports either.
    bind!("key-zoom-in", Chord::plain("="), Editor, zoom_in),
    bind!("key-zoom-in-plus", Chord::plain("+"), Editor, zoom_in),
    bind!("key-zoom-out", Chord::plain("-"), Editor, zoom_out),
    bind!("key-fit", Chord::plain("f"), Editor, |state| state
        .zoom_fit_sequence()),
    bind!("key-fit-selection", Chord::shift("f"), Editor, |state| {
        state.zoom_fit_selection()
    }),
    // Premiere's Shift+Z is the same fit; Z alone stays the zoom tool.
    bind!("key-fit-selection-z", Chord::shift("z"), Editor, |state| {
        state.zoom_fit_selection()
    }),
    // Editing.
    bind!("key-split", Chord::plain("s"), Editor, |state| {
        if let Err(err) = state.split_at_playhead() {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-delete",
        Chord::plain("backspace"),
        Editor,
        delete_selected
    ),
    bind!(
        "key-delete-forward",
        Chord::plain("delete"),
        Editor,
        delete_selected
    ),
    bind!("key-trim-in", Chord::plain("q"), Editor, |state| {
        if let Err(err) = state.trim_selected_to_playhead(true) {
            state.report_error(&err);
        }
    }),
    bind!("key-trim-out", Chord::plain("w"), Editor, |state| {
        if let Err(err) = state.trim_selected_to_playhead(false) {
            state.report_error(&err);
        }
    }),
    // Marks. Alt clears, Shift goes to the mark, plain sets it.
    bind!("key-mark-in", Chord::plain("i"), Editor, |state| state.mark_in_here(), after: Preview),
    bind!("key-mark-in-go", Chord::shift("i"), Editor, |state| state.go_to_in(), after: Preview),
    bind!("key-mark-in-clear", Chord::alt("i"), Editor, |state| state.clear_mark_in_here(), after: Preview),
    bind!("key-mark-out", Chord::plain("o"), Editor, |state| state.mark_out_here(), after: Preview),
    bind!("key-mark-out-go", Chord::shift("o"), Editor, |state| state.go_to_out(), after: Preview),
    bind!("key-mark-out-clear", Chord::alt("o"), Editor, |state| state.clear_mark_out_here(), after: Preview),
    // Cutting the loaded source in. Inert with no monitor open.
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
    bind!("key-match-frame", Chord::alt("f"), Editor, |state| {
        state.match_frame();
    }),
    // History and the project.
    bind!("key-undo", Chord::ctrl("z"), Editor, |state| {
        state.undo();
    }),
    bind!("key-redo", Chord::ctrl_shift("z"), Editor, redo),
    bind!("key-redo-y", Chord::ctrl("y"), Editor, redo),
    bind!("key-save", Chord::ctrl("s"), Editor, |state| {
        if let Err(err) = state.save_project() {
            state.report_error(&err);
        }
    }),
    bind!("key-save-as", Chord::ctrl_shift("s"), Editor, |state| state
        .save_project_as()),
    bind!("key-select-all", Chord::ctrl("a"), Editor, |state| state
        .select_all_unlocked()),
    bind!("key-open", Chord::ctrl("o"), Editor, |state| state
        .request_pick_project()),
    bind!("key-close", Chord::ctrl("w"), Editor, |state| state
        .close_project()),
    bind!("key-new", Chord::ctrl("n"), Editor, new_project),
    bind!("key-export", Chord::ctrl("e"), Editor, |state| state
        .open_dialog(DialogKind::Export)),
    bind!("key-settings", Chord::ctrl(","), Editor, open_settings),
    bind!("key-palette", Chord::ctrl_shift("p"), Editor, |state| state
        .open_dialog(DialogKind::Palette)),
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
    bind!("key-history", Chord::ctrl_shift("h"), Editor, |state| state
        .open_dialog(crate::app::DialogKind::History)),
];
