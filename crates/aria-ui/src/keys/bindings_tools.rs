//! Editor-route bindings for the tools, the panels and the playhead, the
//! Viewer's own keys, then the Home route.
//!
//! One of the three tables [`super::bindings::SHIPPED_BINDINGS`] joins.

use super::bind;
use super::binding_actions::{new_project, open_settings, step, viewer_arrow};
use super::{Binding, Chord};
use crate::app::DialogKind;
use crate::types::{ActivePanel, EditorTool};

/// Tool selection, panel focus, workspaces, the playhead, the Viewer, and the
/// Home rail.
pub(super) const TOOL_ROWS: &[Binding] = &[
    // Tools.
    bind!("key-tool-pointer", Chord::plain("v"), Editor, |state| {
        state.tool = EditorTool::Pointer
    }),
    bind!(
        "key-tool-track-forward",
        Chord::plain("a"),
        Editor,
        |state| { state.tool = EditorTool::TrackSelectForward }
    ),
    bind!(
        "key-tool-track-backward",
        Chord::shift("a"),
        Editor,
        |state| { state.tool = EditorTool::TrackSelectBackward }
    ),
    bind!("key-tool-ripple", Chord::plain("b"), Editor, |state| {
        state.tool = EditorTool::Ripple
    }),
    bind!("key-tool-roll", Chord::plain("n"), Editor, |state| {
        state.tool = EditorTool::Roll
    }),
    bind!("key-tool-rate", Chord::plain("r"), Editor, |state| {
        state.tool = EditorTool::Rate
    }),
    bind!("key-tool-razor", Chord::plain("c"), Editor, |state| {
        state.tool = EditorTool::Razor
    }),
    bind!("key-tool-slip", Chord::plain("y"), Editor, |state| {
        state.tool = EditorTool::Slip
    }),
    bind!("key-tool-slide", Chord::plain("u"), Editor, |state| {
        state.tool = EditorTool::Slide
    }),
    bind!("key-tool-hand", Chord::plain("h"), Editor, |state| {
        state.tool = EditorTool::Hand
    }),
    bind!("key-tool-zoom", Chord::plain("z"), Editor, |state| {
        state.tool = EditorTool::Zoom
    }),
    bind!("key-tool-pen", Chord::plain("p"), Editor, |state| {
        state.tool = EditorTool::Pen
    }),
    bind!("key-tool-type", Chord::plain("t"), Editor, |state| {
        state.tool = EditorTool::Type
    }),
    // Panels. Shift and a digit bring a pane forward.
    bind!("key-pane-project", Chord::shift("1"), Editor, |state| state
        .focus_pane(ActivePanel::Media)),
    bind!("key-pane-viewer", Chord::shift("2"), Editor, |state| state
        .viewer_key()),
    bind!("key-pane-timeline", Chord::shift("3"), Editor, |state| {
        state.focus_pane(ActivePanel::Timeline)
    }),
    bind!(
        "key-pane-effect-controls",
        Chord::shift("4"),
        Editor,
        |state| state.focus_pane(ActivePanel::Inspector)
    ),
    bind!("key-pane-effects", Chord::shift("5"), Editor, |state| state
        .open_effects_panel()),
    bind!("key-mixer", Chord::shift("6"), Editor, |state| state
        .toggle_pane_tab(ActivePanel::Mixer)),
    bind!("key-scopes", Chord::shift("7"), Editor, |state| state
        .focus_dock_panel(crate::dock::PANEL_SCOPES)),
    bind!("key-pane-info", Chord::shift("8"), Editor, |state| state
        .focus_dock_panel(crate::dock::PANEL_INFO)),
    bind!(
        "key-pane-audio-meters",
        Chord::shift("9"),
        Editor,
        |state| { state.focus_dock_panel(crate::dock::PANEL_AUDIO_METERS) }
    ),
    bind!("key-assistant", Chord::shift("0"), Editor, |state| state
        .toggle_pane_tab(ActivePanel::Ai)),
    bind!("key-pane-maximize", Chord::plain("`"), Editor, |state| {
        state.toggle_maximize_focused()
    }),
    bind!("key-pane-next", Chord::ctrl("tab"), Editor, |state| state
        .cycle_pane(true)),
    bind!("key-pane-prev", Chord::ctrl_shift("tab"), Editor, |state| {
        state.cycle_pane(false)
    }),
    bind!("key-pane-close", Chord::ctrl("w"), Editor, |state| {
        if let Err(err) = state.close_focused() {
            state.report_error(&err);
        }
    }),
    bind!("key-workspace-1", Chord::shift_alt("1"), Editor, windowed: |state, cx| state.switch_workspace_slot(0, cx)),
    bind!("key-workspace-2", Chord::shift_alt("2"), Editor, windowed: |state, cx| state.switch_workspace_slot(1, cx)),
    bind!("key-workspace-3", Chord::shift_alt("3"), Editor, windowed: |state, cx| state.switch_workspace_slot(2, cx)),
    bind!("key-workspace-4", Chord::shift_alt("4"), Editor, windowed: |state, cx| state.switch_workspace_slot(3, cx)),
    bind!("key-workspace-5", Chord::shift_alt("5"), Editor, windowed: |state, cx| state.switch_workspace_slot(4, cx)),
    bind!("key-workspace-6", Chord::shift_alt("6"), Editor, windowed: |state, cx| state.switch_workspace_slot(5, cx)),
    bind!("key-workspace-7", Chord::shift_alt("7"), Editor, windowed: |state, cx| state.switch_workspace_slot(6, cx)),
    bind!("key-workspace-8", Chord::shift_alt("8"), Editor, windowed: |state, cx| state.switch_workspace_slot(7, cx)),
    bind!("key-workspace-9", Chord::shift_alt("9"), Editor, windowed: |state, cx| state.switch_workspace_slot(8, cx)),
    // Playhead.
    bind!("key-start", Chord::plain("home"), Editor, |state| state
        .go_to_start_here()),
    bind!("key-end", Chord::plain("end"), Editor, |state| state
        .go_to_end_here()),
    bind!("key-step-back", Chord::plain("left"), Editor, |state| step(
        state, -1
    )),
    bind!(
        "key-step-back-fast",
        Chord::shift("left"),
        Editor,
        |state| step(state, -5)
    ),
    bind!("key-step-forward", Chord::plain("right"), Editor, |state| {
        step(state, 1)
    }),
    bind!(
        "key-step-forward-fast",
        Chord::shift("right"),
        Editor,
        |state| step(state, 5)
    ),
    // The Viewer's own keys, which outrank the window's while it has focus.
    bind!("key-viewer-left", Chord::plain("left"), Viewer, |state| {
        viewer_arrow(state, -1.0, 0.0)
    }),
    bind!("key-viewer-right", Chord::plain("right"), Viewer, |state| {
        viewer_arrow(state, 1.0, 0.0)
    }),
    bind!("key-viewer-up", Chord::plain("up"), Viewer, |state| {
        viewer_arrow(state, 0.0, -1.0)
    }),
    bind!("key-viewer-down", Chord::plain("down"), Viewer, |state| {
        viewer_arrow(state, 0.0, 1.0)
    }),
    bind!(
        "key-viewer-left-fast",
        Chord::shift("left"),
        Viewer,
        |state| viewer_arrow(state, -10.0, 0.0)
    ),
    bind!(
        "key-viewer-right-fast",
        Chord::shift("right"),
        Viewer,
        |state| viewer_arrow(state, 10.0, 0.0)
    ),
    bind!("key-viewer-up-fast", Chord::shift("up"), Viewer, |state| {
        viewer_arrow(state, 0.0, -10.0)
    }),
    bind!(
        "key-viewer-down-fast",
        Chord::shift("down"),
        Viewer,
        |state| viewer_arrow(state, 0.0, 10.0)
    ),
    bind!("key-viewer-fit", Chord::shift("/"), Viewer, |state| state
        .preview_zoom_fit()),
    bind!("key-rulers", Chord::ctrl_shift("r"), Editor, |state| state
        .toggle_rulers()),
    // Home. A different set, because Home has no timeline: Ctrl+O opens the rail
    // rather than a file picker.
    bind!("key-home-new", Chord::ctrl_alt("n"), Home, new_project),
    bind!("key-home-open", Chord::ctrl("o"), Home, |state| state
        .set_home_rail(crate::types::HomeRail::Open)),
    bind!("key-home-settings", Chord::ctrl(","), Home, open_settings),
    bind!("key-home-quit", Chord::ctrl("q"), Home, |state| state
        .request_quit()),
    bind!("key-home-escape", Chord::plain("escape"), Home, |state| {
        if state.dialog != DialogKind::None {
            state.open_dialog(DialogKind::None);
        } else if state.context_menu.is_some() {
            state.close_context();
        } else if state.open_menu.is_some() {
            state.close_menu();
        }
    }),
];
