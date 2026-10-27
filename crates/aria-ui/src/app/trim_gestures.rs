//! Slip, slide, roll, and rate stretch: the trims that are not edge drags.
//!
//! Dragging a clip's edge changes where it starts or ends. The four gestures
//! here keep something fixed instead:
//!
//! - **Slip** keeps the placement and moves the take inside it.
//! - **Slide** keeps the clip's length and gives the delta to its neighbours.
//! - **Roll** keeps both clips' outer edges and moves the cut between them.
//! - **Rate stretch** keeps the source window and changes how long it plays.
//!
//! Each is one edit. Each gesture but the slip is built as a list of steps
//! the release sends as one undo entry: a failure halfway rolls the steps
//! before it back rather than leaving the timeline in a state the gesture
//! never asked for, and the frame that draws a held drag runs the same steps.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, Rational};

use crate::app::AppState;
use crate::app::drag_steps::Step;

/// A neighbour of the clip being dragged, in timeline ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbour {
    pub clip: ClipId,
    pub start: i64,
    pub end: i64,
}

impl AppState {
    /// Move the clip's source window by `by` timeline ticks' worth of source.
    ///
    /// One command, so no group: nothing else on the timeline moves.
    pub fn slip_clip(&mut self, clip: ClipId, by: i64) -> Result<(), EngineError> {
        self.snap_guide = None;
        self.apply_command(Command::Edit(EditCommand::SlipClip { clip, by }))?;
        Ok(())
    }

    /// The clips that touch `clip` on its own lane, in timeline order.
    ///
    /// Only an abutting clip counts. A gap on one side is room the slide can use
    /// without asking anyone for it, and the clip on the far side of a gap must
    /// not be trimmed to pay for a move that never reached it.
    pub fn abutting(&self, clip: ClipId) -> (Option<Neighbour>, Option<Neighbour>) {
        let raw = clip.raw();
        let Some(track) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.clips.iter().any(|item| item.id == raw))
        else {
            return (None, None);
        };
        let Some(me) = track.clips.iter().find(|item| item.id == raw) else {
            return (None, None);
        };
        let at = |edge: i64, pick: fn(&crate::types::UiClip) -> i64| {
            track
                .clips
                .iter()
                .find(|item| item.id != raw && pick(item) == edge)
                .map(|item| Neighbour {
                    clip: ClipId::from_raw(item.id),
                    start: item.timeline_start_ticks,
                    end: item.timeline_end_ticks,
                })
        };
        (
            at(me.timeline_start_ticks, |item| item.timeline_end_ticks),
            at(me.timeline_end_ticks, |item| item.timeline_start_ticks),
        )
    }

    /// Move `clip` by `delta` ticks and give the delta to its neighbours: the
    /// clip keeps its length and the sequence keeps its total length.
    ///
    /// A slide with no room is refused before anything is applied rather than
    /// half-done: `delta` is clamped to what the neighbours can give, and a
    /// clamp to zero is a no-op.
    pub fn slide_clip(
        &mut self,
        clip: ClipId,
        start: i64,
        end: i64,
        delta: i64,
        before: Option<Neighbour>,
        after: Option<Neighbour>,
    ) -> Result<(), EngineError> {
        let delta = slide_room(start, end, delta, before, after);
        self.run_steps(&slide_steps(clip, start, delta, before, after))
    }

    /// Move the cut between two abutting clips by `delta` ticks: the left clip's
    /// end and the right clip's start are the same instant, so both change.
    pub fn roll_cut(
        &mut self,
        left: Neighbour,
        right: Neighbour,
        delta: i64,
    ) -> Result<(), EngineError> {
        self.run_steps(&roll_steps(left, right, roll_room(left, right, delta)))
    }

    /// Retime a clip so it occupies `new_duration` ticks while keeping its
    /// source window: the speed follows from the take and the length.
    ///
    /// This is the rate-stretch gesture, and it is a speed edit rather than a
    /// trim: a trim would drop source, while this plays the same source faster
    /// or slower. The edge dragged moves and the other stays.
    pub fn rate_stretch(
        &mut self,
        clip: ClipId,
        new_duration: i64,
        left: bool,
    ) -> Result<(), EngineError> {
        let steps = self.ui_clip_ref(clip).map_or_else(Vec::new, |ui| {
            stretch_steps(
                clip,
                ui.timeline_start_ticks,
                ui.timeline_end_ticks,
                new_duration,
                left,
            )
        });
        self.run_steps(&steps)
    }

    /// Trim an edge and move the rest of the lane by the same amount, so the
    /// trim leaves no hole and needs no cleanup. `delta` is the signed
    /// movement of the dragged edge.
    pub fn ripple_trim(
        &mut self,
        clip: ClipId,
        track: aria_models::TrackId,
        start: i64,
        end: i64,
        left: bool,
        delta: i64,
    ) -> Result<(), EngineError> {
        let steps = self.ripple_steps(clip, track, start, end, left, delta);
        self.run_steps(&steps)
    }

    /// The edits a ripple trim of `clip`'s head (`left`) or tail by `delta`
    /// sends: the edge trimmed, and the lane the clip is on moved by the same
    /// amount from the edge on, with every lane holding a clip linked to it
    /// while linked selection is on. A lane is opened before an edge grows
    /// into it and closed after an edge shrinks away from it, because a shift
    /// onto ticks another clip still holds is refused.
    pub(crate) fn ripple_steps(
        &self,
        clip: ClipId,
        track: aria_models::TrackId,
        start: i64,
        end: i64,
        left: bool,
        delta: i64,
    ) -> Vec<Step> {
        if delta == 0 {
            return Vec::new();
        }
        let lanes = self.linked_tracks_of(clip, track);
        let shifts = |from: i64, by: i64| {
            lanes
                .iter()
                .map(move |&lane| Step::Shift { lane, from, by })
        };
        let mut steps = Vec::with_capacity(lanes.len() + 1);
        match (left, delta > 0) {
            // The head moves right, then the clip and everything after it
            // slides back to where the head was.
            (true, true) => {
                steps.push(Step::Trim {
                    clip,
                    start: start + delta,
                    end,
                });
                steps.extend(shifts(start + delta, -delta));
            }
            (true, false) => {
                steps.extend(shifts(start, -delta));
                steps.push(Step::Trim {
                    clip,
                    start,
                    end: end - delta,
                });
            }
            (false, true) => {
                steps.extend(shifts(end, delta));
                steps.push(Step::Trim {
                    clip,
                    start,
                    end: end + delta,
                });
            }
            (false, false) => {
                steps.push(Step::Trim {
                    clip,
                    start,
                    end: end + delta,
                });
                steps.extend(shifts(end, delta));
            }
        }
        steps
    }

    /// Every track holding `clip` or one of its linked companions, starting with `primary`.
    fn linked_tracks_of(
        &self,
        clip: ClipId,
        primary: aria_models::TrackId,
    ) -> Vec<aria_models::TrackId> {
        let mut out = vec![primary];
        // With linked selection off a ripple trim moves only the trimmed lane.
        let Some(link) = self
            .ui_clip_ref(clip)
            .and_then(|ui| ui.linked_to)
            .filter(|_| self.linked_selection)
        else {
            return out;
        };
        for lane in &self.store.sequence.tracks {
            if lane.locked {
                continue;
            }
            if lane.clips.iter().any(|item| item.linked_to == Some(link)) {
                let id = aria_models::TrackId::from_raw(lane.id);
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Compute live on-screen trim feedback for an in-flight timeline gesture.
    pub fn compute_trim_feedback(
        &self,
        gesture: crate::surfaces::timeline_gesture::Gesture,
        drag: crate::surfaces::timeline_gesture::Drag,
    ) -> Option<crate::types::TrimFeedback> {
        use crate::surfaces::timeline_gesture::Gesture;
        use crate::tool_catalog::{BodyDrag, EdgeDrag};
        let ui = self.ui_clip_ref(drag.clip)?;
        let rate = self.store.sequence.frame_rate;
        let display = self.store.sequence.time_display;
        let tc = |ticks: i64| crate::timecode::format_time(ticks.max(0), rate, display);
        let signed_tc = |delta: i64| {
            let sign = if delta >= 0 { "+" } else { "-" };
            let body = crate::timecode::format_time(delta.abs(), rate, display);
            format!("{sign}{body} ({sign}{}f)", delta.abs())
        };
        match gesture {
            Gesture::Edge {
                drag: edge_drag,
                left,
                span,
            } => {
                // The edge the release lands, after the magnet and the room.
                let delta = crate::surfaces::timeline_gesture::held(self, gesture, &drag)?.travel;
                let (tool_label, detail) = match edge_drag {
                    Some(EdgeDrag::Ripple) => {
                        let new_dur = if left {
                            (span.end - (span.start + delta)).max(1)
                        } else {
                            (span.end + delta - span.start).max(1)
                        };
                        ("Ripple Trim", format!("Dur {}", tc(new_dur)))
                    }
                    Some(EdgeDrag::Roll) => {
                        let cut = if left {
                            span.start + delta
                        } else {
                            span.end + delta
                        };
                        ("Roll Edit", format!("Cut {}", tc(cut)))
                    }
                    Some(EdgeDrag::Rate) => {
                        let old_dur = (span.end - span.start).max(1);
                        let new_dur = if left {
                            (span.end - (span.start + delta)).max(1)
                        } else {
                            (span.end + delta - span.start).max(1)
                        };
                        let current = Rational::new(ui.speed_num.max(1), ui.speed_den.max(1));
                        let pct = stretch_speed(current, old_dur, new_dur)
                            .map(|s| s.as_f64() * 100.0)
                            .unwrap_or_else(|| current.as_f64() * 100.0);
                        ("Rate Stretch", format!("{pct:.1}% · Dur {}", tc(new_dur)))
                    }
                    None => {
                        let edge_tc = if left {
                            format!("In {}", tc(span.start + delta))
                        } else {
                            format!("Out {}", tc(span.end + delta))
                        };
                        ("Trim", edge_tc)
                    }
                };
                Some(crate::types::TrimFeedback {
                    tool_label: tool_label.into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: delta,
                    delta_label: signed_tc(delta).into(),
                    detail_label: detail.into(),
                })
            }
            Gesture::Body {
                drag: Some(BodyDrag::Slip),
                ..
            } => {
                let shift = -drag.delta;
                let new_in = (ui.source_start_ticks + shift).max(0);
                let span = (ui.timeline_end_ticks - ui.timeline_start_ticks).max(1);
                Some(crate::types::TrimFeedback {
                    tool_label: "Slip".into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: shift,
                    delta_label: signed_tc(shift).into(),
                    detail_label: format!("In {} · Out {}", tc(new_in), tc(new_in + span)).into(),
                })
            }
            Gesture::Body {
                drag: Some(BodyDrag::Slide),
                span,
                ..
            } => {
                let actual = crate::surfaces::timeline_gesture::held(self, gesture, &drag)
                    .map_or(0, |held| held.travel);
                Some(crate::types::TrimFeedback {
                    tool_label: "Slide".into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: actual,
                    delta_label: signed_tc(actual).into(),
                    detail_label: format!(
                        "{} -> {}",
                        tc(span.start + actual),
                        tc(span.end + actual)
                    )
                    .into(),
                })
            }
            Gesture::Body {
                drag: None, span, ..
            }
            | Gesture::RippleBody { span, .. } => {
                let label = if matches!(gesture, Gesture::RippleBody { .. }) {
                    "Ripple Move"
                } else {
                    "Move"
                };
                // The tick the release sends, so the readout and the drawn
                // card agree with the magnet.
                let (to, _) = crate::surfaces::timeline_gesture::landing(self, &drag, &span);
                let delta = to - span.start;
                Some(crate::types::TrimFeedback {
                    tool_label: label.into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: delta,
                    delta_label: signed_tc(delta).into(),
                    detail_label: format!("At {}", tc(to)).into(),
                })
            }
            Gesture::Transition { duration, right } => {
                let alignment = ui
                    .transition
                    .as_ref()
                    .map(|t| t.alignment)
                    .unwrap_or_default();
                let next = crate::app::clip_transitions::transition_ticks_after_drag(
                    duration, drag.delta, right, alignment,
                );
                Some(crate::types::TrimFeedback {
                    tool_label: "Transition".into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: next - duration,
                    delta_label: signed_tc(next - duration).into(),
                    detail_label: format!("Len {}", tc(next)).into(),
                })
            }
            Gesture::Fade { left, current } => {
                let next = if left {
                    (current + drag.delta).max(0)
                } else {
                    (current - drag.delta).max(0)
                };
                Some(crate::types::TrimFeedback {
                    tool_label: if left { "Fade In" } else { "Fade Out" }.into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: next - current,
                    delta_label: signed_tc(next - current).into(),
                    detail_label: format!("Len {}", tc(next)).into(),
                })
            }
        }
    }

    /// The lane a clip is on, as the store sees it. `None` once the clip is gone,
    /// which a gesture reads as "there is nothing to do here".
    pub fn track_of(&self, clip: ClipId) -> Option<aria_models::TrackId> {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|track| track.clips.iter().any(|item| item.id == clip.raw()))
            .map(|track| aria_models::TrackId::from_raw(track.id))
    }
}

/// The edits a slide of `clip`, which starts at `start`, by `delta` sends:
/// the neighbour on the giving side trimmed first, so the move never lands
/// on ticks another clip still holds, then the clip moved, then the
/// neighbour on the other side grown into the ticks the clip left.
pub(crate) fn slide_steps(
    clip: ClipId,
    start: i64,
    delta: i64,
    before: Option<Neighbour>,
    after: Option<Neighbour>,
) -> Vec<Step> {
    if delta == 0 {
        return Vec::new();
    }
    let prev = before.map(|prev| Step::Trim {
        clip: prev.clip,
        start: prev.start,
        end: prev.end + delta,
    });
    let next = after.map(|next| Step::Trim {
        clip: next.clip,
        start: next.start + delta,
        end: next.end,
    });
    let (first, last) = if delta > 0 {
        (next, prev)
    } else {
        (prev, next)
    };
    first
        .into_iter()
        .chain([Step::Move {
            clip,
            start: start + delta,
        }])
        .chain(last)
        .collect()
}

/// The edits a roll of the cut between `left` and `right` by `delta` sends:
/// the clip the cut moves into shrinks first, because growing into ticks the
/// other clip still holds is refused.
pub(crate) fn roll_steps(left: Neighbour, right: Neighbour, delta: i64) -> Vec<Step> {
    let grows = Step::Trim {
        clip: left.clip,
        start: left.start,
        end: left.end + delta,
    };
    let shrinks = Step::Trim {
        clip: right.clip,
        start: right.start + delta,
        end: right.end,
    };
    match delta.signum() {
        0 => Vec::new(),
        1 => vec![shrinks, grows],
        _ => vec![grows, shrinks],
    }
}

/// The edits a rate stretch of `clip`, on `start..end`, to `length` ticks
/// sends. A stretch by the head (`left`) keeps where the clip ends, so it
/// moves the clip as well: before it grows and after it shrinks, so the clip
/// never covers ticks another clip holds.
pub(crate) fn stretch_steps(
    clip: ClipId,
    start: i64,
    end: i64,
    length: i64,
    left: bool,
) -> Vec<Step> {
    let retime = Step::Retime {
        clip,
        duration: length,
    };
    let moved = Step::Move {
        clip,
        start: end - length,
    };
    match (left, length.cmp(&(end - start))) {
        (_, std::cmp::Ordering::Equal) => Vec::new(),
        (false, _) => vec![retime],
        (true, std::cmp::Ordering::Greater) => vec![moved, retime],
        (true, std::cmp::Ordering::Less) => vec![retime, moved],
    }
}

/// How much of `delta` the neighbours can absorb.
///
/// A neighbour must keep at least one tick, and a side with no neighbour is
/// unbounded in that direction: sliding into empty space is a plain move.
pub fn slide_room(
    start: i64,
    end: i64,
    delta: i64,
    before: Option<Neighbour>,
    after: Option<Neighbour>,
) -> i64 {
    if delta == 0 || end <= start {
        return 0;
    }
    if delta > 0 {
        // Moving right eats the next clip's head; it must keep a tick.
        let room = match after {
            Some(next) => (next.end - 1 - next.start).max(0),
            None => delta,
        };
        delta.min(room)
    } else {
        let room = match before {
            Some(prev) => (prev.end - 1 - prev.start).max(0),
            None => -delta,
        };
        delta.max(-room)
    }
}

/// How far the cut between two abutting clips can move: each side keeps a tick.
pub fn roll_room(left: Neighbour, right: Neighbour, delta: i64) -> i64 {
    if delta > 0 {
        delta.min((right.end - 1 - right.start).max(0))
    } else {
        delta.max(-((left.end - 1 - left.start).max(0)))
    }
}

/// The speed that makes a clip currently running at `current` and `old` ticks
/// long occupy `new` ticks instead.
///
/// `None` when there is nothing to do or the request is degenerate: a zero-tick
/// result would ask the model for an empty clip, and a speed the model would
/// refuse is better not sent.
pub fn stretch_speed(current: Rational, old: i64, new: i64) -> Option<Rational> {
    if old <= 0 || new <= 0 || old == new {
        return None;
    }
    // Occupying less timeline means playing faster: the factor is old / new,
    // applied on top of whatever the clip already runs at.
    let num = i64::from(current.num.max(1)) * old;
    let den = i64::from(current.den.max(1)) * new;
    // A ratio too extreme for `i32` terms is a dragged pixel away from
    // meaningless; the gesture does nothing rather than sending a wrong
    // speed.
    Rational::from_ratio(num, den)
}
