//! Where a drag is pointing, as the frame that draws it reads it.
//!
//! A GPUI drop listener is not handed the pointer, so the tick under it has to
//! be captured while the drag moves. [`crate::shell::Shell`] keeps the reading
//! the drop will commit; this is the same reading kept for the frame that draws
//! it, in the shape [`crate::motion`] keeps hover fades in — view state, read by
//! the renderer and never by an edit.
//!
//! It sits outside `panels` because both ends of a drag read it: the timeline
//! draws the line, and the shell forgets the hint on the first frame with no
//! drag in flight, so a drag released off the panel leaves nothing behind.

use std::cell::Cell;

/// The row a drag is over, and the tick inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropHint {
    /// Over a lane: the clip lands on this track.
    Lane { track: u64, tick: i64 },
    /// Over an edge strip: the clip lands on a lane that does not exist yet.
    Edge { top: bool, tick: i64 },
}

thread_local! {
    static HINT: Cell<Option<DropHint>> = Cell::new(None);
}

/// Record where the drag points. `true` when that is somewhere new, which is the
/// only case a repaint is owed.
pub(crate) fn set(hint: DropHint) -> bool {
    HINT.with(|cell| {
        let changed = cell.get() != Some(hint);
        cell.set(Some(hint));
        changed
    })
}

/// Forget the drag. Called on the drop that consumes it, and by the timeline's
/// own frame driver once no drag is in flight, so a drag released off the panel
/// cannot leave a line behind.
pub(crate) fn clear() {
    HINT.with(|cell| cell.set(None));
}

/// The tick a lane is being dropped on, when the drag is over that lane.
pub(crate) fn lane_tick(track: u64) -> Option<i64> {
    HINT.with(|cell| match cell.get() {
        Some(DropHint::Lane { track: id, tick }) if id == track => Some(tick),
        _ => None,
    })
}

/// The tick an edge strip is being dropped on, when the drag is over that strip.
pub(crate) fn edge_tick(top: bool) -> Option<i64> {
    HINT.with(|cell| match cell.get() {
        Some(DropHint::Edge { top: side, tick }) if side == top => Some(tick),
        _ => None,
    })
}
