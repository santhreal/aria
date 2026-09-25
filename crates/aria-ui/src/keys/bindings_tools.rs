//! Editor-route bindings for the tools and the playhead, then the Home route.
//!
//! One of the three tables [`super::bindings::SHIPPED_BINDINGS`] joins. Moved
//! here unchanged.

use super::bind;
use super::binding_actions::{new_project, open_settings, step};
use super::{Binding, Chord};
use crate::app::DialogKind;
use crate::types::{ActivePanel, EditorTool};

/// Tool selection, playhead movement and nudging, and the Home rail.
pub(super) const TOOL_ROWS: &[Binding] = &[
    // Tools.
    bind!("key-tool-pointer", Chord::plain("v"), Editor, |state| {
        state.tool = EditorTool::Pointer
    }),
    bind!("key-tool-pointer-a", Chord::plain("a"), Editor, |state| {
        state.tool = EditorTool::Pointer
    }),
    bind!("key-tool-razor", Chord::plain("b"), Editor, |state| state
        .tool =
        EditorTool::Razor),
    bind!("key-tool-hand", Chord::plain("h"), Editor, |state| state
        .tool =
        EditorTool::Hand),
    bind!("key-tool-zoom", Chord::plain("z"), Editor, |state| state
        .tool =
        EditorTool::Zoom),
    bind!("key-tool-slip", Chord::plain("y"), Editor, |state| state
        .tool =
        EditorTool::Slip),
    bind!("key-tool-slide", Chord::plain("u"), Editor, |state| state
        .tool =
        EditorTool::Slide),
    bind!("key-tool-roll", Chord::plain("n"), Editor, |state| state
        .tool =
        EditorTool::Roll),
    bind!("key-tool-rate", Chord::plain("r"), Editor, |state| state
        .tool =
        EditorTool::Rate),
    bind!("key-tool-ripple", Chord::plain("p"), Editor, |state| {
        state.tool = EditorTool::Ripple
    }),
    bind!("key-assistant", Chord::shift("a"), Editor, |state| state
        .active_panel =
        state.active_panel.toggle_to(ActivePanel::Ai)),
    bind!("key-mixer", Chord::shift("6"), Editor, |state| state
        .active_panel =
        state.active_panel.toggle_to(ActivePanel::Mixer)),
    // Playhead and nudging.
    bind!("key-start", Chord::plain("home"), Editor, |state| state
        .set_playhead(0)),
    bind!("key-end", Chord::plain("end"), Editor, |state| state
        .set_playhead(state.store.sequence.duration_ticks)),
    bind!("key-step-back", Chord::plain("left"), Editor, |state| step(
        state, -1
    )),
    bind!(
        "key-step-back-fast",
        Chord::shift("left"),
        Editor,
        |state| step(state, -10)
    ),
    bind!("key-step-forward", Chord::plain("right"), Editor, |state| {
        step(state, 1)
    }),
    bind!(
        "key-step-forward-fast",
        Chord::shift("right"),
        Editor,
        |state| step(state, 10)
    ),
    // Home. A different set, because Home has no timeline: Ctrl+O opens the rail
    // rather than a file picker.
    bind!("key-home-new", Chord::ctrl("n"), Home, new_project),
    bind!("key-home-open", Chord::ctrl("o"), Home, |state| state
        .set_home_rail(crate::types::HomeRail::Open)),
    bind!("key-home-settings", Chord::ctrl(","), Home, open_settings),
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
