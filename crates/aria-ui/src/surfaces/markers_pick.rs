//! The controls pressed in the markers panel: add one at the playhead, step
//! one's colour, delete one.
//!
//! `Remove marker` claimed the panel's delete button and nothing could press it,
//! so the button could have been wired to the wrong id, or deleted, without
//! turning anything red. The three edits the panel offers are declared here and
//! the panel builds each control from one of them.
//!
//! Two neighbours in the same row stay out, and for different reasons:
//!
//! - The row body seeks: it moves the playhead and asks for a frame, which needs
//!   the window. Nothing in the project changes, so there is no command to
//!   reach, and the playhead is written by half the panels in the app: owning it
//!   here would make the escape guard below fail for all of them.
//! - The row's rename control opens a draft rather than writing the marker. The
//!   rename registry owns that, and it drives the field through to the commit,
//!   which is where `Edit marker` is sent from.

use aria_engine::EngineError;

use crate::app::AppState;

/// One control the markers panel draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Drop a marker where the playhead stands.
    AddAtPlayhead,
    /// Step one marker to the next colour in the ring.
    CycleColour(u64),
    /// Delete one marker by id.
    Remove(u64),
}

impl Pick {
    /// The pressed control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::AddAtPlayhead => "add a marker at the playhead",
            Self::CycleColour(_) => "cycle a marker's colour",
            Self::Remove(_) => "remove a marker",
        }
    }
}

/// Press one control on the open project.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::AddAtPlayhead => state.add_marker_at_playhead(),
        Pick::CycleColour(marker) => state.cycle_marker_color(marker),
        Pick::Remove(marker) => state.remove_marker(marker),
    }
}
