//! What Enter does on the dialog that is up.
//!
//! # Why
//!
//! Every dialog was opened by a chord and answered by a click. `Ctrl+E` put the
//! export sheet on screen and Enter did nothing there, because the shell
//! consumed Enter in exactly two places: a focused text field, and the palette.
//! A dialog an operator can open from the keyboard and cannot commit from the
//! keyboard is half a surface, and the half that is missing is the one that
//! does the work.
//!
//! The decision is a row per [`DialogKind`], matched exhaustively. A new dialog
//! does not compile until it says whether Enter commits it, and when it does
//! not, why not. That match is the fail-by-default gate: the census beside it
//! can only check that a reason was written, never that a variant exists.

use super::{AppState, DialogKind};

/// What Enter means to one dialog.
pub enum Primary {
    /// Enter commits the dialog by running this.
    ///
    /// Takes no window on purpose: a primary action is a change to the session,
    /// and anything that needs the window (a file picker, a frame) belongs on a
    /// control the pointer is already on.
    Run(fn(&mut AppState)),
    /// Enter does nothing here, and the reason it does nothing. The reason is
    /// data rather than a comment so the census can insist it was written.
    Nothing(&'static str),
}

/// The primary action of `kind`.
pub fn primary(kind: DialogKind) -> Primary {
    match kind {
        DialogKind::None => Primary::Nothing("no dialog is up, so Enter is the editor's own key"),
        // The sheet's Export button, through the one owner of that press.
        DialogKind::Export => Primary::Run(AppState::export_to_destination),
        // Save is the affirmative answer: the prompt exists so the work
        // survives leaving the project, so the key meaning "yes" keeps it.
        DialogKind::UnsavedChanges => Primary::Run(AppState::save_and_continue),
        DialogKind::SpeedDuration => Primary::Run(AppState::apply_speed_duration),
        DialogKind::AudioGain => Primary::Run(AppState::apply_audio_gain),
        DialogKind::RemoveAttributes => Primary::Run(AppState::apply_remove_attributes),
        DialogKind::KeyframeVelocity => Primary::Run(AppState::apply_keyframe_velocity),
        DialogKind::MarkerEdit => Primary::Run(AppState::apply_marker_edit),
        DialogKind::InstallPlugin => Primary::Nothing(
            "installing runs a stranger's code, so it takes a press on Install, not a stray Enter",
        ),
        DialogKind::Settings => {
            Primary::Nothing("each row is its own control and there is no one setting to commit")
        }
        DialogKind::Palette => {
            Primary::Nothing("the palette answers Enter itself: it runs the highlighted command")
        }
        DialogKind::Jobs => Primary::Nothing("the board reports work that is already running"),
        DialogKind::ProjectReport => Primary::Nothing("the report is read, not answered"),
        DialogKind::About => Primary::Nothing("the card is read, not answered"),
        DialogKind::Plugin(_) => {
            Primary::Nothing("a plugin draws its own controls and the host cannot press them")
        }
    }
}

impl AppState {
    /// Run the primary action of the dialog that is up.
    ///
    /// Returns whether something ran, so the shell can tell a committed dialog
    /// from a key a dialog swallowed while it holds the keyboard.
    pub fn run_dialog_primary(&mut self) -> bool {
        match primary(self.dialog) {
            Primary::Run(run) => {
                run(self);
                true
            }
            Primary::Nothing(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "dialog_primary_tests.rs"]
mod tests;
