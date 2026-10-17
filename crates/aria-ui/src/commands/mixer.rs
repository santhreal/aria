//! Palette rows for the mixer: opening it, and adding the lane it mixes into.
//!
//! One of the groups [`super::BUILTIN_COMMANDS`] joins. Routing itself is not
//! here on purpose: a send names a source lane and a destination bus, and a
//! palette row that guessed either would be a control that acts on something
//! the operator did not point at. The mixer strip is where a route is chosen;
//! these rows are how the mixer is reached and how it gets a bus to route to.

use crate::types::ActivePanel;

use super::host::CommandEntry;

pub(super) const MIXER_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-mixer-open",
        label: "Mixer: open the mixer",
        run: |state| {
            state.focus_pane(ActivePanel::Mixer);
            state.status_message = Some("Faders, outputs and sends, one strip per lane".into());
        },
    },
    CommandEntry {
        id: "pal-mixer-add-bus",
        label: "Mixer: add a submix bus",
        run: |state| {
            if let Err(err) = state.add_submix_lane() {
                state.report_error(&err);
                return;
            }
            state.focus_pane(ActivePanel::Mixer);
        },
    },
];
