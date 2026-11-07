//! What a drag on the timeline means: the body drags, the edge drags, and the
//! two edges of a transition's window.
//!
//! Seven of the model's edits were reachable only by dragging, and each one's
//! only mention was a `match` arm inside a gpui mouse-up listener in
//! `panels::timeline::clip::finish`. Nothing could enumerate them, so a tool wired
//! to the wrong verb read exactly like a tool wired to the right one, and two
//! commands (`Slip clip`, `Set transition length`) had no measurable surface at
//! all.
//!
//! A gesture is the tool's answer plus how far the pointer went. The panel owns
//! the pixels, because only the panel knows the zoom: it converts a pointer
//! delta into ticks and hands over a [`Drag`]. Everything after that is here,
//! including the magnet, so what a drag asks for is one table a census presses
//! rather than five listeners.
//!
//! A press that never moved is not here: the threshold is in pixels, so the
//! panel filters it before a gesture exists.

use aria_engine::EngineError;
use aria_models::{ClipId, Project, RationalTime, TrackId};

use crate::app::AppState;
use crate::app::clip_audio::fade_within;
use crate::app::clip_transitions::{transition_ticks_after_drag, transition_ticks_within};
use crate::app::drag_room::RoomKind;
use crate::app::drag_steps::{Plan, Step};
use crate::app::trim_gestures::{Neighbour, roll_steps, slide_steps, stretch_steps};
use crate::snap::SnapCandidate;
use crate::tool_catalog::{BodyDrag, EdgeDrag};
use crate::types::{HeldEdge, UiClip};

/// What the pointer took hold of, and how far it went.
///
/// Ticks rather than pixels: the panel knows the zoom and this does not, and a
/// gesture asserted in pixels would be asserted against a viewport.
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    /// The clip under the pointer when the drag began.
    pub clip: ClipId,
    /// Signed movement of whatever the drag is holding.
    pub delta: i64,
}

/// Where the clip sat before the drag, and what the magnet may do to the edge
/// that moves.
///
/// A transition window carries none of this: it is centered on a cut, so nothing
/// about it is lane-relative and no magnet applies to it.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    /// Where the clip started and ended before the drag.
    pub start: i64,
    pub end: i64,
    /// How near a magnet target has to be to take the moving edge.
    pub threshold: i64,
    /// The magnet, as this drag sees it: Alt suspends it for one gesture while
    /// the toolbar toggle stays on.
    pub magnet: bool,
}

/// One drag the timeline understands.
///
/// The two tool axes are the catalog's own: [`BodyDrag`] and [`EdgeDrag`] are
/// `Option`, and `None` is the pointer tool's plain move or plain trim rather
/// than "no gesture". A new tool axis is a new variant of those enums, and this
/// table stops compiling until it says what the drag does.
#[derive(Debug, Clone, Copy)]
pub enum Gesture {
    /// A drag across the clip's body.
    Body {
        /// What the tool makes of a body drag.
        drag: Option<BodyDrag>,
        /// The lane the drag ends on, which a body drag may have changed. An
        /// edge drag has no such field: an edge never leaves its lane.
        track: TrackId,
        span: Span,
    },
    /// A pointer-tool body drag with Shift: move and close the hole left behind.
    RippleBody { track: TrackId, span: Span },
    /// A drag on one of the clip's own edges.
    Edge {
        /// What the tool makes of an edge drag.
        drag: Option<EdgeDrag>,
        /// The head rather than the tail.
        left: bool,
        span: Span,
    },
    /// A drag on one edge of the blend window at a cut. The window is centered on
    /// the junction, so both edges lengthen it and neither moves the cut.
    Transition {
        /// The right-hand edge rather than the left.
        right: bool,
        /// The window's current length, which the drag is measured against.
        duration: i64,
    },
    /// A drag on one of the two fade handles a clip card draws. The handle sits
    /// where the fade currently ends, so the pointer's travel is the change in
    /// the fade's length.
    Fade {
        /// The fade in rather than the fade out.
        left: bool,
        /// The fade's current length, which the drag is measured against.
        current: i64,
    },
}

impl Gesture {
    /// The gesture, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Body { drag: None, .. } => "a clip dragged along its track",
            Self::RippleBody { .. } => "a clip rippled along its track, closing the hole it left",
            Self::Body {
                drag: Some(BodyDrag::Slip),
                ..
            } => "a take slipped inside its clip",
            Self::Body {
                drag: Some(BodyDrag::Slide),
                ..
            } => "a clip slid into its neighbors",
            Self::Edge { drag: None, .. } => "a clip edge trimmed",
            Self::Edge {
                drag: Some(EdgeDrag::Roll),
                ..
            } => "a cut rolled between two clips",
            Self::Edge {
                drag: Some(EdgeDrag::Rate),
                ..
            } => "a clip stretched by its edge",
            Self::Edge {
                drag: Some(EdgeDrag::Ripple),
                ..
            } => "an edge rippled, taking the track with it",
            Self::Transition { .. } => "a transition's window dragged by an edge",
            Self::Fade { left: true, .. } => "a fade-in handle dragged on a clip card",
            Self::Fade { left: false, .. } => "a fade-out handle dragged on a clip card",
        }
    }

    /// The lane the drag carries its clip to: a move's or a ripple move's. A
    /// slip and a slide keep the clip on the lane it started on.
    pub fn lane(&self) -> Option<TrackId> {
        match *self {
            Self::Body {
                drag: None, track, ..
            }
            | Self::RippleBody { track, .. } => Some(track),
            Self::Body {
                drag: Some(BodyDrag::Slip | BodyDrag::Slide),
                ..
            }
            | Self::Edge { .. }
            | Self::Transition { .. }
            | Self::Fade { .. } => None,
        }
    }

    /// What bounds the drag's travel: an edit the model refuses past a
    /// limit. A move, a ripple move, a slip, a transition window and a fade
    /// are held by their own edits.
    pub(crate) fn room(&self) -> Option<RoomKind> {
        match *self {
            Self::Edge {
                drag: None, left, ..
            } => Some(RoomKind::Trim { left }),
            Self::Edge {
                drag: Some(EdgeDrag::Ripple),
                left,
                ..
            } => Some(RoomKind::Ripple { left }),
            Self::Edge {
                drag: Some(EdgeDrag::Roll),
                left,
                ..
            } => Some(RoomKind::Roll { left }),
            Self::Edge {
                drag: Some(EdgeDrag::Rate),
                left,
                ..
            } => Some(RoomKind::Stretch { left }),
            Self::Body {
                drag: Some(BodyDrag::Slide),
                ..
            } => Some(RoomKind::Slide),
            Self::Body {
                drag: None | Some(BodyDrag::Slip),
                ..
            }
            | Self::RippleBody { .. }
            | Self::Transition { .. }
            | Self::Fade { .. } => None,
        }
    }

    /// The undo step the release leaves, for a drag that sends several edits;
    /// `None` for one that sends one edit, which the edit names.
    pub(crate) fn step_name(&self) -> Option<&'static str> {
        match *self {
            Self::Edge {
                drag: Some(drag), ..
            } => Some(drag.step_name()),
            Self::Body {
                drag: Some(drag), ..
            } => drag.step_name(),
            Self::Edge { drag: None, .. }
            | Self::Body { drag: None, .. }
            | Self::RippleBody { .. }
            | Self::Transition { .. }
            | Self::Fade { .. } => None,
        }
    }
}

/// Where `desired` lands once the magnet has had its say, and the magnet it
/// lands on. A drag that has not moved stays where it is: the magnet pulls
/// what the pointer carries, and a press carries nothing yet.
///
/// The guide line the magnet draws belongs to the drag, not to the gesture: it
/// is written while the pointer moves and every verb below drops it, so a
/// gesture that arrives on release has nothing left to clear.
fn snapped(
    state: &AppState,
    drag: &Drag,
    span: &Span,
    desired: i64,
) -> (i64, Option<SnapCandidate>) {
    if drag.delta == 0 {
        return (desired.max(0), None);
    }
    state.snap_at(
        desired.max(0),
        Some(drag.clip.raw()),
        span.threshold,
        !span.magnet,
    )
}

/// Whether a clip card offers the fade drag: an unlocked clip that plays
/// sound. A fade is a sound fade, so a generated clip, which plays none, has
/// no fade to drag. The panel adds the pixel condition, a bar wide enough to
/// hold both handles.
pub(crate) fn offers_fade(clip: &UiClip) -> bool {
    !clip.locked && clip.generator.is_none()
}

/// Where a drag a room holds lets go.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    /// How far the release takes what the drag holds, inside the room.
    pub travel: i64,
    /// Whether the room holds the drag short of where the pointer and the
    /// magnet took it.
    pub stopped: bool,
    /// The magnet what the drag holds lands on.
    pub guide: Option<SnapCandidate>,
}

/// Where an edge drag or a slide lets go: the edge, or the clip's head,
/// moved by the drag, after the magnet, held inside the room the gesture
/// gives it. `None` for a drag no room holds. The release sends it, the
/// readout states it and the held frame draws it.
///
/// The room is the floor, not the sequence start: a ripple trim grows the head
/// of a clip at zero by pushing the lane, so its edge may pass zero.
pub(crate) fn held(state: &AppState, gesture: Gesture, drag: &Drag) -> Option<Held> {
    let kind = gesture.room()?;
    let (from, (to, guide)) = match gesture {
        Gesture::Edge { left, span, .. } => {
            let from = if left { span.start } else { span.end };
            let desired = from + drag.delta;
            let to = if desired < 0 {
                (desired, None)
            } else {
                snapped(state, drag, &span, desired)
            };
            (from, to)
        }
        Gesture::Body { span, .. } => (
            span.start,
            snapped(state, drag, &span, span.start + drag.delta),
        ),
        Gesture::RippleBody { .. } | Gesture::Transition { .. } | Gesture::Fade { .. } => {
            return None;
        }
    };
    let wanted = to - from;
    let travel = state.drag_room(kind, drag.clip).hold(wanted);
    Some(Held {
        travel,
        stopped: travel != wanted,
        guide: guide.filter(|_| travel == wanted),
    })
}

/// Where a body drag lets the clip's head go: its start moved by the drag,
/// after the magnet, and the magnet it lands on. The release sends it and the
/// held frame draws it.
pub(crate) fn landing(state: &AppState, drag: &Drag, span: &Span) -> (i64, Option<SnapCandidate>) {
    snapped(state, drag, span, span.start + drag.delta)
}

/// What a slip, a transition window drag or a fade drag sets on release,
/// inside the bounds its edit keeps. The release sends it, the readout states
/// it and the held frame draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Setting {
    /// The take moves inside the clip by this many of its own source ticks.
    Slip(i64),
    /// The window at the clip's right junction takes this many ticks.
    Transition(i64),
    /// The fade in, or the fade out, takes this many ticks.
    Fade { left: bool, ticks: i64 },
}

/// What `gesture` sets on release for the pointer's `drag`, read off
/// `project`, the project as the drag found it. `None` for a drag that sets
/// nothing this way, a move, an edge drag or a slide, and for a clip `project`
/// does not hold.
///
/// A slip of a clip that plays no take moves it by nothing, and the release
/// still sends it, so the model states why the clip does not slip.
pub(crate) fn setting(
    state: &AppState,
    project: &Project,
    gesture: Gesture,
    drag: &Drag,
) -> Option<Setting> {
    match gesture {
        Gesture::Body {
            drag: Some(BodyDrag::Slip),
            ..
        } => {
            let travel = RationalTime::new(drag.delta, project.timeline().frame_rate);
            let by = project.clip(drag.clip)?.slip_for(travel).unwrap_or(0);
            Some(Setting::Slip(by))
        }
        // A junction with no window to bound keeps the length the drag asks
        // for, and the release says there is no transition there.
        Gesture::Transition { right, duration } => {
            let alignment = state
                .clip_transition_alignment(drag.clip)
                .unwrap_or_default();
            let ticks = transition_ticks_after_drag(duration, drag.delta, right, alignment);
            Some(Setting::Transition(
                transition_ticks_within(project, drag.clip, ticks).unwrap_or(ticks),
            ))
        }
        // A fade in grows to the right and a fade out grows to the left, so
        // one pointer direction lengthens one and shortens the other.
        Gesture::Fade { left, current } => {
            let desired = if left {
                current + drag.delta
            } else {
                current - drag.delta
            };
            let ticks = fade_within(state.ui_clip_ref(drag.clip)?, desired);
            Some(Setting::Fade { left, ticks })
        }
        Gesture::Body {
            drag: None | Some(BodyDrag::Slide),
            ..
        }
        | Gesture::RippleBody { .. }
        | Gesture::Edge { .. } => None,
    }
}

/// The edits an edge drag or a slide of `clip` sends when what it holds
/// travels `travel` ticks, in the order the release sends them. Empty for no
/// travel, for a roll with no clip abutting the edge, and for a drag no room
/// holds.
pub(crate) fn steps(state: &AppState, gesture: Gesture, clip: ClipId, travel: i64) -> Vec<Step> {
    if travel == 0 {
        return Vec::new();
    }
    match gesture {
        Gesture::Body {
            drag: Some(BodyDrag::Slide),
            span,
            ..
        } => {
            let (before, after) = state.abutting(clip);
            slide_steps(clip, span.start, travel, before, after)
        }
        Gesture::Edge {
            drag: None,
            left,
            span,
        } => vec![if left {
            Step::Trim {
                clip,
                start: span.start + travel,
                end: span.end,
            }
        } else {
            Step::Trim {
                clip,
                start: span.start,
                end: span.end + travel,
            }
        }],
        Gesture::Edge {
            drag: Some(EdgeDrag::Roll),
            left,
            span,
        } => {
            let (before, after) = state.abutting(clip);
            let me = Neighbour {
                clip,
                start: span.start,
                end: span.end,
            };
            // Only an abutting neighbor has a shared cut to roll. Without one
            // the gesture does nothing, rather than silently trimming.
            let pair = if left {
                before.map(|prev| (prev, me))
            } else {
                after.map(|next| (me, next))
            };
            pair.map_or_else(Vec::new, |(cut_left, cut_right)| {
                roll_steps(cut_left, cut_right, travel)
            })
        }
        // The edge dragged moves and the other stays, so the new length is
        // measured against the edge that did not move.
        Gesture::Edge {
            drag: Some(EdgeDrag::Rate),
            left,
            span,
        } => {
            let length = if left {
                span.end - span.start - travel
            } else {
                span.end - span.start + travel
            };
            stretch_steps(clip, span.start, span.end, length, left)
        }
        Gesture::Edge {
            drag: Some(EdgeDrag::Ripple),
            left,
            span,
        } => state.track_of(clip).map_or_else(Vec::new, |track| {
            state.ripple_steps(clip, track, span.start, span.end, left, travel)
        }),
        Gesture::Body {
            drag: None | Some(BodyDrag::Slip),
            ..
        }
        | Gesture::RippleBody { .. }
        | Gesture::Transition { .. }
        | Gesture::Fade { .. } => Vec::new(),
    }
}

/// What an edge drag or a slide held at `held` sends on release, and the
/// edge it holds.
pub(crate) fn plan(state: &AppState, gesture: Gesture, drag: &Drag, held: Held) -> Plan {
    Plan {
        steps: steps(state, gesture, drag.clip, held.travel),
        edge: match gesture {
            Gesture::Edge { left, .. } => Some(HeldEdge {
                clip: drag.clip.raw(),
                left,
                stopped: held.stopped,
            }),
            Gesture::Body { .. }
            | Gesture::RippleBody { .. }
            | Gesture::Transition { .. }
            | Gesture::Fade { .. } => None,
        },
    }
}

/// Run one gesture. The magnet is applied here, so every drag that snaps snaps
/// the same way.
pub fn apply(state: &mut AppState, gesture: Gesture, drag: Drag) -> Result<(), EngineError> {
    match gesture {
        Gesture::Body {
            drag: None,
            track,
            span,
        } => {
            let (to, _) = landing(state, &drag, &span);
            state.move_clip(drag.clip, track, to)
        }
        Gesture::RippleBody { track, span } => {
            let (to, _) = landing(state, &drag, &span);
            state.ripple_move_clip(drag.clip, track, to)
        }
        Gesture::Body {
            drag: Some(BodyDrag::Slip),
            ..
        }
        | Gesture::Transition { .. }
        | Gesture::Fade { .. } => {
            let set = {
                let engine = state.engine().lock();
                setting(state, engine.project(), gesture, &drag)
            };
            match set {
                Some(Setting::Slip(by)) => state.slip_clip(drag.clip, by),
                Some(Setting::Transition(ticks)) => {
                    state.set_clip_transition_ticks(drag.clip, ticks)
                }
                Some(Setting::Fade { left: true, ticks }) => {
                    state.set_clip_fade_in(drag.clip.raw(), ticks)
                }
                Some(Setting::Fade { left: false, ticks }) => {
                    state.set_clip_fade_out(drag.clip.raw(), ticks)
                }
                None => Ok(()),
            }
        }
        Gesture::Body {
            drag: Some(BodyDrag::Slide),
            ..
        }
        | Gesture::Edge { .. } => {
            let travel = held(state, gesture, &drag).map_or(0, |held| held.travel);
            state.run_steps(
                gesture.step_name(),
                &steps(state, gesture, drag.clip, travel),
            )
        }
    }
}
