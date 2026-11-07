//! Every drag the timeline offers, on every clip of every dressed state, both
//! ways, near and far: a drag the engine refuses states why, never the value
//! copy.
//!
//! # Why
//!
//! Closes, for a timeline drag, the class of a structural refusal shown as the
//! value copy, "That value isn't accepted here. Enter another value", whose fix
//! names a field a drag does not have. The drags are the panel's own: a body
//! drag with each tool onto every lane, a ripple move onto every lane, each
//! edge with each tool, each fade handle from its current length on a bar that
//! draws one, and each edge of a blend window from its current length, on
//! every clip a press can take hold of (an unlocked one). A drag that edits is
//! undone before the next.
//!
//! A fade handle the bar draws sets its fade: a handle drawn on a clip with no
//! sound, as a generated clip's was, offers a drag that is always refused.
//!
//! What this does not catch: a state no fixture dresses, a drag whose span the
//! panel computes differently from the card's, a bar too narrow for its fade
//! handles, and a refusal worded by the UI with the value copy without going
//! through `describe_engine_error`.

use aria_engine::EngineError;
use aria_models::{ClipId, ModelError, TrackId};

use super::room::FAR;
use super::{STEP, THRESHOLD};
use crate::app::AppState;
use crate::app::describe_engine_error;
use crate::app::fixtures;
use crate::app::menu_reach_tests::history_labels;
use crate::surfaces::timeline_gesture::{self, Drag, Gesture, Span};
use crate::tool_catalog::{BodyDrag, EdgeDrag};

/// The tool a body drag runs under: the pointer's plain move and each body tool.
const BODY: [Option<BodyDrag>; 3] = [None, Some(BodyDrag::Slip), Some(BodyDrag::Slide)];

/// The tool an edge drag runs under: the pointer's plain trim and each edge tool.
const EDGE: [Option<EdgeDrag>; 4] = [
    None,
    Some(EdgeDrag::Roll),
    Some(EdgeDrag::Rate),
    Some(EdgeDrag::Ripple),
];

/// How far each drag goes, both ways: one step, and past any room.
const DELTAS: [i64; 4] = [-FAR, -STEP, STEP, FAR];

/// The dressed states, each a project with at least one clip on the timeline.
fn states() -> [(&'static str, fn() -> AppState); 8] {
    [
        ("a still", fixtures::a_clip),
        ("a video with sound", || fixtures::with_video().0),
        ("an audio clip", || fixtures::with_audio().0),
        ("a title", fixtures::a_title),
        ("a linked pair", fixtures::linked_pair),
        ("a stacked pair", || fixtures::stacked_pair().0),
        ("a blended junction", || fixtures::blended_junction().0),
        ("a parented pair", || fixtures::parented_pair().0),
    ]
}

/// Every drag a press can start on the clips of `state`, with the clip it holds.
fn drags(state: &AppState) -> Vec<(Gesture, ClipId)> {
    let lanes: Vec<TrackId> = state
        .store
        .sequence
        .tracks
        .iter()
        .map(|lane| TrackId::from_raw(lane.id))
        .collect();
    let mut drags = Vec::new();
    for card in state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .filter(|card| !card.locked)
    {
        let clip = ClipId::from_raw(card.id);
        let span = Span {
            start: card.timeline_start_ticks,
            end: card.timeline_end_ticks,
            threshold: THRESHOLD,
            magnet: false,
        };
        for &track in &lanes {
            for drag in BODY {
                drags.push((Gesture::Body { drag, track, span }, clip));
            }
            drags.push((Gesture::RippleBody { track, span }, clip));
        }
        for drag in EDGE {
            for left in [true, false] {
                drags.push((Gesture::Edge { drag, left, span }, clip));
            }
        }
        if timeline_gesture::offers_fade(card) {
            for (left, current) in [(true, card.fade_in_ticks), (false, card.fade_out_ticks)] {
                let current = current.max(0);
                drags.push((Gesture::Fade { left, current }, clip));
            }
        }
        if let Some(blend) = &card.transition {
            for right in [true, false] {
                drags.push((
                    Gesture::Transition {
                        right,
                        duration: blend.duration_ticks,
                    },
                    clip,
                ));
            }
        }
    }
    drags
}

#[test]
fn no_timeline_drag_answers_with_the_value_copy() {
    // The copy every model refusal of a value reads.
    let copy = describe_engine_error(&EngineError::Model(ModelError::InvalidParam(String::new())));
    let mut sent = 0;
    let mut wrong = Vec::new();
    let mut fades_refused = Vec::new();
    for (name, dress) in states() {
        let mut state = dress();
        for (gesture, clip) in drags(&state) {
            for delta in DELTAS {
                sent += 1;
                let steps = history_labels(&state).len();
                let applied = timeline_gesture::apply(&mut state, gesture, Drag { clip, delta });
                if let Err(err) = &applied {
                    if describe_engine_error(err) == copy {
                        wrong.push(format!("{gesture:?} by {delta} on {name}: {err}"));
                    }
                    if matches!(gesture, Gesture::Fade { .. }) {
                        fades_refused.push(format!("{gesture:?} by {delta} on {name}: {err}"));
                    }
                }
                if history_labels(&state).len() > steps {
                    assert!(state.undo(), "{gesture:?} by {delta} on {name}: not undone");
                }
                state.sync_from_engine();
            }
        }
    }
    assert!(sent > 0, "the census sent no drag");
    assert!(
        wrong.is_empty(),
        "these drags answered with the value copy:\n{}",
        wrong.join("\n")
    );
    assert!(
        fades_refused.is_empty(),
        "these fade handles are drawn on a clip whose fade is refused:\n{}",
        fades_refused.join("\n")
    );
}
