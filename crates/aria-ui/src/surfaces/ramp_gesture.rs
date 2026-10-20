//! The speed graph's own gestures: press, drag, release, and a right-click that
//! drops a point.
//!
//! # Why
//!
//! The ramp canvas kept its behaviour in four gpui listeners. Each one held a
//! decision nothing else could see: a press grabs a nearby point or adds one
//! where it landed, a move is inert unless a point is grabbed, a release that
//! moved nothing writes nothing, and a right-click drops the point under the
//! pointer or does nothing. Four rules, four closures, no way to enumerate them,
//! so a canvas wired to the wrong call read the same as one wired to the right
//! call.
//!
//! The gesture carries pointer positions rather than ticks, the way the preview
//! overlay's does: the graph frame the pointer is mapped through is recorded by
//! the panel as it draws, and mapping it here keeps the whole rule in one place
//! instead of splitting it across the listener and the state.
//!
//! [`apply`] answers whether the window has anything new to draw, so the panel
//! asks for a repaint from the outcome rather than deciding for itself.

use aria_engine::EngineError;

use crate::app::AppState;

/// One gesture on the speed graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// A left press on the graph.
    Press { at: [f32; 2] },
    /// A pointer move, which is only a drag while a point is grabbed.
    Drag { at: [f32; 2] },
    /// The release that writes the point where it was dropped.
    Release,
    /// A right press, which drops the point under it.
    Drop { at: [f32; 2] },
}

impl Gesture {
    /// The gesture, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Press { .. } => "a press on the speed graph",
            Self::Drag { .. } => "a drag across the speed graph",
            Self::Release => "the release that lands a dragged ramp point",
            Self::Drop { .. } => "a right press on a ramp point",
        }
    }
}

/// The tick and speed under a pointer position, if the graph was drawn.
///
/// Above [`apply`] so the mapping is not read as one of the calls this registry
/// owns: a panel may still ask where a pointer is, it may not write a ramp.
fn under(state: &AppState, at: [f32; 2]) -> Option<(i64, f32)> {
    state.ramp_at_pointer(at[0], at[1])
}

/// Whether a point is currently grabbed.
fn grabbed(state: &AppState) -> bool {
    state.ramp_dragging().is_some()
}

/// The ramp point at `tick`, if the curve has one there.
fn point_at(state: &AppState, tick: i64) -> Option<i64> {
    crate::app::clip_speed::ramp_point_at(&state.selected_ramp_curve(), tick)
}

/// Run one gesture. `true` means the graph changed and the window should draw.
pub fn apply(state: &mut AppState, gesture: Gesture) -> Result<bool, EngineError> {
    match gesture {
        Gesture::Press { at } => {
            let Some((tick, value)) = under(state, at) else {
                return Ok(false);
            };
            // A press grabs a nearby point, or adds one where it landed: an
            // empty graph has nothing to grab, and the first press on it should
            // still put a point down.
            if state.begin_ramp_drag(tick) {
                return Ok(true);
            }
            state.set_selected_ramp_point(tick, value)?;
            Ok(true)
        }
        Gesture::Drag { at } => {
            if !grabbed(state) {
                return Ok(false);
            }
            let Some((tick, value)) = under(state, at) else {
                return Ok(false);
            };
            Ok(state.update_ramp_drag(tick, value))
        }
        Gesture::Release => {
            if !grabbed(state) {
                return Ok(false);
            }
            state.commit_ramp_drag()?;
            Ok(true)
        }
        Gesture::Drop { at } => {
            let Some((tick, _)) = under(state, at) else {
                return Ok(false);
            };
            let Some(point) = point_at(state, tick) else {
                return Ok(false);
            };
            state.remove_selected_ramp_point(point)?;
            Ok(true)
        }
    }
}
