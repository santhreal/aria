//! A drag reads the session at its press, and no move of it does.
//!
//! The preview worker holds the session for the length of each frame it
//! decodes. A move that read the session waited out the decode in flight and
//! drew the clip, its room and its readout that much late: the room of an
//! edge drag was read again whenever the drag turned into another, the clips
//! a move takes once per drag, and the project its held clips are built from
//! once per drag, each behind its own lock.
//!
//! Each census drag is pressed, then, with the session held on another
//! thread, carried far both ways and turned into every other drag on its
//! clip. A move that reads the session waits for the holder, whose own wait
//! runs out first.
//!
//! The release and a key that trims an edge read the session again; neither
//! is a move. A revision that lands mid-drag is read again as well, on the
//! next move.

use std::sync::Arc;

use super::super::junction;
use super::{FAR, at_the_start, between_two, drags, in_open_lane};
use crate::surfaces::timeline_gesture::Drag;
use crate::test_support::while_held;

#[test]
fn a_held_drag_reads_the_session_only_at_its_press() {
    for dress in [junction, between_two, in_open_lane, at_the_start] {
        let mut dressed = dress();
        let all = drags(&dressed);
        let state = &mut dressed.state;
        for &(pressed, on) in &all {
            state.hold_drag(pressed, Drag { clip: on, delta: 0 });
            let engine = Arc::clone(&state.engine);
            let waited = while_held(&engine, || {
                for &(gesture, clip) in all.iter().filter(|&&(_, clip)| clip == on) {
                    for delta in [-FAR, -1, 1, FAR] {
                        state.hold_drag(gesture, Drag { clip, delta });
                    }
                }
            });
            state.end_drag();
            assert!(!waited, "a move of {pressed:?} waited for the session");
        }
    }
}
