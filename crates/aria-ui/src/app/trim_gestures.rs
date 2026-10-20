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
//! Each is one edit. Slide and roll issue two commands, so they run inside an
//! engine history group: one undo puts the pair back, and a failure halfway
//! rolls the first command back rather than leaving the timeline in a state the
//! operator never asked for.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, Rational, RationalTime, TimeRange};

use crate::app::AppState;

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
        if delta == 0 {
            self.snap_guide = None;
            return Ok(());
        }
        self.snap_guide = None;
        let rate = self.playhead.rate;
        // The first command lands before the second; a gesture left half-applied
        // is worse than the gesture not happening.
        self.one_step(|this| this.slide_steps(clip, start, end, delta, before, after, rate))
    }

    // The neighbour on the giving side is trimmed before the clip moves, so the
    // move never lands on ticks another clip still holds.
    #[allow(clippy::too_many_arguments)]
    fn slide_steps(
        &mut self,
        clip: ClipId,
        start: i64,
        end: i64,
        delta: i64,
        before: Option<Neighbour>,
        after: Option<Neighbour>,
        rate: Rational,
    ) -> Result<(), EngineError> {
        if delta > 0 {
            if let Some(next) = after {
                self.trim_to(next.clip, next.start + delta, next.end, rate)?;
            }
            self.move_to(clip, start + delta, rate)?;
            if let Some(prev) = before {
                self.trim_to(prev.clip, prev.start, prev.end + delta, rate)?;
            }
        } else {
            if let Some(prev) = before {
                self.trim_to(prev.clip, prev.start, prev.end + delta, rate)?;
            }
            self.move_to(clip, start + delta, rate)?;
            if let Some(next) = after {
                self.trim_to(next.clip, next.start + delta, next.end, rate)?;
            }
        }
        let _ = end;
        Ok(())
    }

    /// Move the cut between two abutting clips by `delta` ticks: the left clip's
    /// end and the right clip's start are the same instant, so both change.
    pub fn roll_cut(
        &mut self,
        left: Neighbour,
        right: Neighbour,
        delta: i64,
    ) -> Result<(), EngineError> {
        let delta = roll_room(left, right, delta);
        if delta == 0 {
            self.snap_guide = None;
            return Ok(());
        }
        self.snap_guide = None;
        let rate = self.playhead.rate;
        // Shrink first: growing into ticks the other clip still holds would be
        // refused as an overlap.
        self.one_step(|this| {
            if delta > 0 {
                this.trim_to(right.clip, right.start + delta, right.end, rate)
                    .and_then(|()| this.trim_to(left.clip, left.start, left.end + delta, rate))
            } else {
                this.trim_to(left.clip, left.start, left.end + delta, rate)
                    .and_then(|()| this.trim_to(right.clip, right.start + delta, right.end, rate))
            }
        })
    }

    /// Retime a clip so it occupies `new_duration` ticks while keeping its
    /// source window: the speed follows from the two lengths.
    ///
    /// This is the rate-stretch gesture, and it is a speed edit rather than a
    /// trim: a trim would drop source, while this plays the same source faster
    /// or slower. A clip that is already retimed is stretched from where it is,
    /// not from 1x, so two drags compose the way the operator sees them.
    pub fn rate_stretch(
        &mut self,
        clip: ClipId,
        old_duration: i64,
        new_duration: i64,
    ) -> Result<(), EngineError> {
        let Some(ui) = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|item| item.id == clip.raw())
        else {
            return Ok(());
        };
        let current = Rational::new(ui.speed_num.max(1), ui.speed_den.max(1));
        let reversed = ui.reversed;
        let Some(speed) = stretch_speed(current, old_duration, new_duration) else {
            self.snap_guide = None;
            return Ok(());
        };
        self.snap_guide = None;
        self.apply_command(Command::Edit(EditCommand::SetClipSpeed {
            clip,
            speed,
            reversed,
        }))?;
        Ok(())
    }

    /// Trim an edge and move the rest of the lane by the same amount, so the
    /// trim leaves no hole and needs no cleanup.
    ///
    /// `delta` is the signed movement of the dragged edge. The order of the two
    /// commands follows the direction: the lane is opened before an edge grows
    /// into it and closed after an edge shrinks away from it, because a shift
    /// onto ticks another clip still holds is refused.
    pub fn ripple_trim(
        &mut self,
        clip: ClipId,
        track: aria_models::TrackId,
        start: i64,
        end: i64,
        left: bool,
        delta: i64,
    ) -> Result<(), EngineError> {
        self.snap_guide = None;
        if delta == 0 {
            return Ok(());
        }
        let rate = self.playhead.rate;
        let tracks = self.linked_tracks_of(clip, track);
        self.one_step(|this| {
            if left {
                if delta > 0 {
                    // The head moves right, then the clip and everything after it
                    // slides back to where the head used to be.
                    this.trim_to(clip, start + delta, end, rate)?;
                    for lane in tracks {
                        this.shift_after(lane, start + delta, -delta, rate)?;
                    }
                    Ok(())
                } else {
                    for lane in tracks {
                        this.shift_after(lane, start, -delta, rate)?;
                    }
                    this.trim_to(clip, start, end - delta, rate)
                }
            } else if delta > 0 {
                for lane in tracks {
                    this.shift_after(lane, end, delta, rate)?;
                }
                this.trim_to(clip, start, end + delta, rate)
            } else {
                this.trim_to(clip, start, end + delta, rate)?;
                for lane in tracks {
                    this.shift_after(lane, end, delta, rate)?;
                }
                Ok(())
            }
        })
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
                let delta = drag.delta;
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
                let (before, after) = self.abutting(drag.clip);
                let actual = slide_room(span.start, span.end, drag.delta, before, after);
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
                Some(crate::types::TrimFeedback {
                    tool_label: label.into(),
                    clip_name: ui.name.clone(),
                    delta_ticks: drag.delta,
                    delta_label: signed_tc(drag.delta).into(),
                    detail_label: format!("At {}", tc(span.start + drag.delta)).into(),
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

    fn shift_after(
        &mut self,
        track: aria_models::TrackId,
        from: i64,
        delta: i64,
        rate: Rational,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::ShiftClips {
            track,
            from: RationalTime::new(from.max(0), rate),
            delta: RationalTime::new(delta, rate),
        }))?;
        Ok(())
    }

    fn trim_to(
        &mut self,
        clip: ClipId,
        start: i64,
        end: i64,
        rate: Rational,
    ) -> Result<(), EngineError> {
        let start = start.max(0);
        let end = end.max(start + 1);
        self.apply_command(Command::Edit(
            self.trim_command(clip, TimeRange::at_rate(start, end - start, rate)),
        ))?;
        Ok(())
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

    fn move_to(&mut self, clip: ClipId, start: i64, rate: Rational) -> Result<(), EngineError> {
        let Some(track) = self.track_of(clip) else {
            return Err(EngineError::Refused(
                "that clip is no longer on the timeline".into(),
            ));
        };
        self.apply_command(Command::Edit(self.move_command(
            clip,
            track,
            RationalTime::new(start.max(0), rate),
        )))?;
        Ok(())
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
