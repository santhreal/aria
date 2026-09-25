//! Context and rename targets: what a menu or rename gesture points at.
//!
//! Moved out of `types.rs` to stay under the file ceiling. Each type travels
//! with its own `impl` blocks, and `types.rs` re-exports this module with
//! `pub use`, so every existing `crate::types::Name` path still resolves.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum ContextTarget {
    Clip {
        clip: u64,
        track: u64,
    },
    Track {
        track: u64,
    },
    /// The blend window at the right junction of `clip`.
    Transition {
        clip: u64,
    },
    Preview,
    PoolItem {
        media: u64,
    },
    Recent {
        path: PathBuf,
    },
}

/// What an in-progress inline entry is editing.
///
/// Typing is key-routed like the command palette rather than focused into a
/// GPUI text field, so one code path serves every inline value. Most targets are
/// names; `ShiftFrames` is a signed frame count, routed the same way for the same
/// reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameTarget {
    Marker(u64),
    /// A marker's note. Separate from its name because clearing it is a real
    /// intention: an empty draft here means "no note", not "cancel".
    MarkerComment(u64),
    Track(u64),
    /// One clip's card name. An empty draft is a value here: it gives the card
    /// back the name its source provides.
    Clip(u64),
    Project,
    /// The placeholder hint on the selected clip's template slot.
    SlotLabel,
    /// The selected title's text content.
    ClipText,
    /// A signed frame offset to shift the selection by, across every lane it
    /// spans.
    ShiftFrames,
}

impl RenameTarget {
    /// Prompt shown in front of the draft.
    pub fn label(self) -> &'static str {
        match self {
            RenameTarget::Marker(_) => "Rename marker",
            RenameTarget::MarkerComment(_) => "Marker note",
            RenameTarget::Track(_) => "Rename track",
            RenameTarget::Clip(_) => "Rename clip",
            RenameTarget::Project => "Rename project",
            RenameTarget::SlotLabel => "Slot hint",
            RenameTarget::ClipText => "Title text",
            RenameTarget::ShiftFrames => "Shift by (frames)",
        }
    }

    /// Whether an empty draft is a value rather than a cancel.
    ///
    /// A name cannot be blank, so an empty name draft means the operator changed
    /// their mind. A note can be blank, and clearing one is the only way to take
    /// a stale note off a marker.
    pub fn allows_empty(self) -> bool {
        matches!(self, RenameTarget::MarkerComment(_) | RenameTarget::Clip(_))
    }
}
