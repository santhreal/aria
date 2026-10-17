//! What an operator presses in the Align panel: an edge to line the selection
//! up on, or an axis to spread it along.
//!
//! The Align To selector stays out. It chooses what the next press aligns
//! against and writes nothing to the project, so there is no command to reach;
//! the panel sets it on the state directly, and an align press reads it there.

use aria_engine::EngineError;

use crate::app::AppState;
use crate::app::align::{AlignAxis, AlignEdge};

/// One button the Align panel draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Line every selected clip's bounds up on this edge of the target.
    Align(AlignEdge),
    /// Space the selected clips' centres evenly along this axis.
    Distribute(AlignAxis),
}

impl Pick {
    /// What the operator pressed, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Align(_) => "the selection aligned on an edge",
            Self::Distribute(_) => "the selection distributed along an axis",
        }
    }
}

/// Press one button on the selection.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::Align(edge) => state.align_selected(edge),
        Pick::Distribute(axis) => state.distribute_selected(axis),
    }
}
