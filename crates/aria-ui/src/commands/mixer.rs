//! The palette row that adds a submix bus for the mixer to route into.
//!
//! One of the groups [`super::BUILTIN_COMMANDS`] joins. Routing itself is not
//! here: a send names a source lane and a destination bus, and a palette row
//! that guessed either would act on something the pointer was not on. The
//! mixer strip is where a route is chosen; Window > Audio Track Mixer opens the
//! mixer, and this row gives it a bus to route to.

use crate::types::ActivePanel;

use super::host::CommandEntry;

pub(super) const MIXER_ROWS: &[CommandEntry] = &[CommandEntry {
    id: "pal-mixer-add-bus",
    label: "Mixer: Add a Submix Bus",
    run: |state| {
        if let Err(err) = state.add_submix_lane() {
            state.report_error(&err);
            return;
        }
        state.focus_pane(ActivePanel::Mixer);
    },
}];
