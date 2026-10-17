//! Editor-route bindings for timeline property reveal, keyframe interpolation,
//! and the graph editor.
//!
//! Joined into [`super::bindings::SHIPPED_BINDINGS`] alongside the edit, tool,
//! and reach tables. Chords follow `docs/spec/keyboard.md`.

use super::bind;
use super::{Binding, Chord};
use crate::app::property_rows::RevealAction;

/// Property reveal (`Alt+P/S/R/T/A/E/M/L/U`, ``Alt+` ``), additive `Shift+Alt`
/// reveal toggles, keyframe easing (`F9`, `Shift+F9`, `Ctrl+Shift+F9`,
/// `Ctrl+Alt+H`), and the timeline graph editor (`Shift+F3`).
pub(super) const REVEAL_ROWS: &[Binding] = &[
    bind!("key-reveal-position", Chord::alt("p"), Editor, |state| {
        state.reveal_properties(RevealAction::Position, false)
    }),
    bind!(
        "key-reveal-position-add",
        Chord::shift_alt("p"),
        Editor,
        |state| state.reveal_properties(RevealAction::Position, true)
    ),
    bind!("key-reveal-scale", Chord::alt("s"), Editor, |state| state
        .reveal_properties(RevealAction::Scale, false)),
    bind!(
        "key-reveal-scale-add",
        Chord::shift_alt("s"),
        Editor,
        |state| state.reveal_properties(RevealAction::Scale, true)
    ),
    bind!("key-reveal-rotation", Chord::alt("r"), Editor, |state| {
        state.reveal_properties(RevealAction::Rotation, false)
    }),
    bind!(
        "key-reveal-rotation-add",
        Chord::shift_alt("r"),
        Editor,
        |state| state.reveal_properties(RevealAction::Rotation, true)
    ),
    bind!("key-reveal-opacity", Chord::alt("t"), Editor, |state| state
        .reveal_properties(RevealAction::Opacity, false)),
    bind!(
        "key-reveal-opacity-add",
        Chord::shift_alt("t"),
        Editor,
        |state| state.reveal_properties(RevealAction::Opacity, true)
    ),
    bind!("key-reveal-anchor", Chord::alt("a"), Editor, |state| state
        .reveal_properties(RevealAction::Anchor, false)),
    bind!(
        "key-reveal-anchor-add",
        Chord::shift_alt("a"),
        Editor,
        |state| state.reveal_properties(RevealAction::Anchor, true)
    ),
    bind!("key-reveal-effects", Chord::alt("e"), Editor, |state| state
        .reveal_properties(RevealAction::Effects, false)),
    bind!(
        "key-reveal-effects-add",
        Chord::shift_alt("e"),
        Editor,
        |state| state.reveal_properties(RevealAction::Effects, true)
    ),
    bind!("key-reveal-masks", Chord::alt("m"), Editor, |state| state
        .reveal_properties(RevealAction::Masks, false)),
    bind!(
        "key-reveal-masks-add",
        Chord::shift_alt("m"),
        Editor,
        |state| state.reveal_properties(RevealAction::Masks, true)
    ),
    bind!("key-reveal-audio", Chord::alt("l"), Editor, |state| state
        .reveal_properties(RevealAction::Audio, false)),
    bind!(
        "key-reveal-audio-add",
        Chord::shift_alt("l"),
        Editor,
        |state| state.reveal_properties(RevealAction::Audio, true)
    ),
    bind!("key-reveal-animated", Chord::alt("u"), Editor, |state| {
        state.reveal_properties(RevealAction::AnimatedOrModified, false)
    }),
    bind!(
        "key-reveal-animated-add",
        Chord::shift_alt("u"),
        Editor,
        |state| state.reveal_properties(RevealAction::AnimatedOrModified, true)
    ),
    bind!("key-reveal-close", Chord::alt("`"), Editor, |state| state
        .reveal_properties(RevealAction::CloseAll, false)),
    bind!("key-easy-ease", Chord::plain("f9"), Editor, |state| {
        if let Err(err) = state.easy_ease_selected_or_playhead() {
            state.report_error(&err);
        }
    }),
    bind!("key-easy-ease-in", Chord::shift("f9"), Editor, |state| {
        if let Err(err) = state.easy_ease_in_selected_or_playhead() {
            state.report_error(&err);
        }
    }),
    bind!(
        "key-easy-ease-out",
        Chord::ctrl_shift("f9"),
        Editor,
        |state| {
            if let Err(err) = state.easy_ease_out_selected_or_playhead() {
                state.report_error(&err);
            }
        }
    ),
    bind!("key-toggle-hold", Chord::ctrl_alt("h"), Editor, |state| {
        if let Err(err) = state.toggle_hold_selected_or_playhead() {
            state.report_error(&err);
        }
    }),
    bind!("key-graph-editor", Chord::shift("f3"), Editor, |state| {
        state.toggle_timeline_graph_editor()
    }),
];
