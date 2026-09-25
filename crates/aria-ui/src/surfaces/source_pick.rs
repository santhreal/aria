//! What an operator presses on the source monitor's bar: the two marks, the
//! clear, and the two edits that land the marked source on the timeline.
//!
//! Three commands claimed this bar and nothing could press it. One of them
//! claimed a control that does not exist: there is no fit-to-fill button. A fit
//! is what Insert and Overwrite do when the *sequence* also carries a marked
//! range, because then the destination has a length of its own and the source is
//! retimed to fill it. The rows are declared here, the bar is built from them,
//! and the census presses each one with and without that range.
//!
//! The scrub bar is not a row: dragging it moves the monitor's own playhead
//! through [`AppState::scrub_source`], which is a pointer gesture over a painted
//! rectangle rather than a control with a label. Nothing in the project changes,
//! so there is no command to reach.

use aria_engine::EngineError;

use crate::app::AppState;
use crate::types::EditMode;

/// One control on the source monitor's bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Mark the source in at the monitor's own tick.
    MarkIn,
    /// Mark the source out one frame past the monitor's own tick.
    MarkOut,
    /// Drop both source marks.
    ClearMarks,
    /// Land the marked source range, pushing what follows to the right.
    Insert,
    /// Land the marked source range over whatever is already there.
    Overwrite,
}

impl Pick {
    /// What the operator pressed, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::MarkIn => "mark the source in",
            Self::MarkOut => "mark the source out",
            Self::ClearMarks => "clear the source marks",
            Self::Insert => "insert the marked source",
            Self::Overwrite => "overwrite with the marked source",
        }
    }
}

/// Press one control on the open monitor.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        // The marks are the monitor's own view of the source: they name where a
        // later edit starts and stops, and no project holds them.
        Pick::MarkIn => {
            state.set_source_in();
            Ok(())
        }
        Pick::MarkOut => {
            state.set_source_out();
            Ok(())
        }
        Pick::ClearMarks => {
            state.clear_source_marks();
            Ok(())
        }
        Pick::Insert => state.cut_source_in(EditMode::Ripple),
        Pick::Overwrite => state.cut_source_in(EditMode::Overwrite),
    }
}
