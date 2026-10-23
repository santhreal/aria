//! Text and model answers that had no way to be asked for.
//!
//! One of the tables [`super::BUILTIN_COMMANDS`] joins. Each row here
//! reaches a capability the editor already shipped and no panel, menu, or key
//! sent:
//!
//! - The font preflight is the only load-time failure signal a font file
//!   produces. `fontdb` drops unparseable bytes silently, so the number of
//!   faces the bytes added is the whole answer, and nothing read it.
//! - The ink report separates "this title draws nothing" from "this title is
//!   behind something", which look identical on the canvas.
//! - The model sweep re-hashes what is installed. `status` only asks whether a
//!   plausible file is there; a truncated copy or a half-restored backup passes
//!   that and then fails deep inside Whisper with a message about tensors.
//!
//! The canvas pick is not a row: clicking the picture is the gesture, and it is
//! wired in [`crate::panels::preview`].

use crate::app::AppState;

use super::host::CommandEntry;

/// Start the model digest sweep, reporting a store that cannot be opened.
///
/// Per-model failures are the job's own detail; this only fails when there is
/// no store to sweep.
fn verify_models(state: &mut AppState) {
    if let Err(err) = state.verify_models() {
        state.report_error(err);
    }
}

pub(super) const TEXT_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-font-check",
        label: "Text: Check a Font File",
        run: AppState::check_font_file,
    },
    CommandEntry {
        id: "pal-title-ink",
        label: "Text: Report the Selected Title's Ink",
        run: AppState::report_title_ink,
    },
    CommandEntry {
        id: "pal-models-verify",
        label: "Transcription: Verify Installed Models",
        run: verify_models,
    },
];
