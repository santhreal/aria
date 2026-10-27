//! The time area beside a property's value in Effect Controls.
//!
//! A strip spans the selected clip's range and draws the property's keyframes
//! and the playhead at their places in it. A press on a keyframe selects it, or
//! adds it to the selection with Shift, and holds it for a drag: the drag moves
//! every selected keyframe, snapping to the playhead and to the clip's other
//! keyframes unless Alt is held, and the release writes the move as one undo
//! step. A press anywhere else moves the playhead there and scrubs while the
//! button stays down. Escape drops a keyframe drag unwritten and returns a
//! scrubbed playhead to where the press found it.
//!
//! The press is held on the state rather than on the strip, so a drag keeps
//! going when the pointer leaves the strip; the shell routes the pointer. A
//! keyframe drag is the timeline's drag (`begin_timeline_keyframe_drag`), so a
//! strip and the timeline's property row move keyframes the same way and each
//! draws the other's drag while it is in flight.

use aria_models::{ClipId, ClipParam};

use super::AppState;
use super::property_rows::{KeyframeIconShape, SelectedKeyframe};

/// Pointer distance, in pixels, within which a press lands on a keyframe.
pub const KEY_HIT_PX: f32 = 5.0;

/// Pointer distance, in pixels, within which a dragged keyframe snaps.
pub const KEY_SNAP_PX: f32 = 6.0;

/// One keyframe as a strip draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripMark {
    /// Where it sits in absolute timeline ticks, moved by a drag in flight.
    pub tick: i64,
    pub shape: KeyframeIconShape,
    pub selected: bool,
}

/// One property's time area: the clip's range, the playhead, and the
/// property's keyframes.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyStrip {
    pub clip: ClipId,
    pub param: ClipParam,
    /// The clip's first tick.
    pub start: i64,
    /// The tick after the clip's last frame.
    pub end: i64,
    pub playhead: i64,
    /// Empty, and unallocated, on a property with no keyframes.
    pub marks: Vec<StripMark>,
}

impl KeyStrip {
    /// Where `tick` sits across the strip: `0` at the clip's start, `1` at its
    /// end.
    pub fn fraction(&self, tick: i64) -> f32 {
        let span = (self.end - self.start).max(1);
        ((tick - self.start) as f32 / span as f32).clamp(0.0, 1.0)
    }

    /// Where the playhead sits across the strip, or `None` when it is off the
    /// clip.
    pub fn playhead_fraction(&self) -> Option<f32> {
        (self.start..self.end)
            .contains(&self.playhead)
            .then(|| self.fraction(self.playhead))
    }

    /// The frame at `fraction` across the strip, held inside the clip.
    pub fn tick_at(&self, fraction: f32) -> i64 {
        tick_across(self.start, self.end, fraction)
    }

    /// The keyframe nearest `fraction`, when it lies within `slop` of it.
    pub fn mark_near(&self, fraction: f32, slop: f32) -> Option<i64> {
        self.marks
            .iter()
            .map(|mark| (mark.tick, (self.fraction(mark.tick) - fraction).abs()))
            .filter(|(_, gap)| *gap <= slop)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(tick, _)| tick)
    }
}

/// The frame at `fraction` across `start..end`, held inside it. A fraction that
/// is not a number lands on the start.
fn tick_across(start: i64, end: i64, fraction: f32) -> i64 {
    let last = (end - 1).max(start);
    let fraction = if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    };
    let span = (end - start).max(1) as f32;
    (start + (fraction * span).round() as i64).clamp(start, last)
}

/// A press held on a strip until the button comes up.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StripPress {
    /// The strip's left edge and width, in window pixels, when it was pressed.
    left: f32,
    width: f32,
    start: i64,
    end: i64,
    hold: StripHold,
}

#[derive(Debug, Clone, Copy)]
enum StripHold {
    /// A keyframe drag, measured from the x the press landed on.
    Key { from_x: f32 },
    /// A scrub that Escape returns to `from`; `at` is where it is now.
    Scrub { from: i64, at: i64 },
}

impl AppState {
    /// The time area of `param` on the selected clip, or `None` when nothing is
    /// selected or the clip cannot animate `param`.
    pub fn key_strip(&self, param: ClipParam) -> Option<KeyStrip> {
        let detail = self.selected.as_ref()?;
        let clip = &detail.model;
        let row = detail.params.iter().find(|row| row.spec.param == param)?;
        let (id, start, end) = (clip.id, clip.timeline.start.value, clip.timeline.end_tick());
        let marks = if row.marks.is_empty() {
            Vec::new()
        } else {
            self.with_prop_rows(|rows| {
                let drag = rows.time_drag.as_ref();
                row.marks
                    .iter()
                    .map(|mark| {
                        let dragged = drag.filter(|drag| {
                            drag.origins
                                .iter()
                                .any(|(c, p, t, ..)| *c == id && *p == param && *t == mark.tick)
                        });
                        StripMark {
                            tick: dragged.map_or(mark.tick, |drag| {
                                (mark.tick + drag.delta_ticks).clamp(start, end)
                            }),
                            shape: mark.shape,
                            selected: rows.selected_keyframes.contains(&SelectedKeyframe {
                                clip: id,
                                param,
                                tick: mark.tick,
                            }),
                        }
                    })
                    .collect()
            })
        };
        Some(KeyStrip {
            clip: id,
            param,
            start,
            end,
            playhead: self.playhead.value,
            marks,
        })
    }

    /// A press at window x `x` on the strip of `param`, which spans
    /// `left..left + width`. On a keyframe it selects it, or adds it to the
    /// selection with `additive`, and holds it for a drag; anywhere else it
    /// moves the playhead there and holds a scrub. Returns whether the press
    /// was taken: a strip that has not been laid out, or a property the
    /// selected clip cannot animate, takes none.
    pub fn press_key_strip(
        &mut self,
        param: ClipParam,
        x: f32,
        left: f32,
        width: f32,
        additive: bool,
    ) -> bool {
        if width.is_nan() || width <= 0.0 || !x.is_finite() || !left.is_finite() {
            return false;
        }
        let Some(strip) = self.key_strip(param) else {
            return false;
        };
        let fraction = (x - left) / width;
        let hold = match strip.mark_near(fraction, KEY_HIT_PX / width) {
            Some(tick) => {
                self.begin_timeline_keyframe_drag(strip.clip, param, tick, additive);
                StripHold::Key { from_x: x }
            }
            None => {
                let from = self.playhead.value;
                let at = strip.tick_at(fraction);
                if at != from {
                    self.scrub_playhead(at);
                }
                StripHold::Scrub { from, at }
            }
        };
        self.with_prop_rows_mut(|rows| {
            rows.strip_press = Some(StripPress {
                left,
                width,
                start: strip.start,
                end: strip.end,
                hold,
            });
        });
        true
    }

    /// The pointer moved to window x `x` with a strip held. A keyframe drag
    /// snaps unless `alt` is held. Returns whether anything moved.
    pub fn drag_key_strip(&mut self, x: f32, alt: bool) -> bool {
        let Some(press) = self.with_prop_rows(|rows| rows.strip_press) else {
            return false;
        };
        if !x.is_finite() {
            return false;
        }
        match press.hold {
            StripHold::Key { from_x } => {
                let per_px = (press.end - press.start).max(1) as f32 / press.width;
                let raw = ((x - from_x) * per_px).round() as i64;
                let threshold = (KEY_SNAP_PX * per_px).round() as i64;
                self.update_timeline_keyframe_drag(raw, threshold, alt)
            }
            StripHold::Scrub { from, at } => {
                let tick = tick_across(press.start, press.end, (x - press.left) / press.width);
                if tick == at {
                    return false;
                }
                self.scrub_playhead(tick);
                self.with_prop_rows_mut(|rows| {
                    if let Some(press) = rows.strip_press.as_mut() {
                        press.hold = StripHold::Scrub { from, at: tick };
                    }
                });
                true
            }
        }
    }

    /// Let go of a held strip: a keyframe drag writes its move as one undo
    /// step, and a scrub leaves the playhead where it is. Returns whether a
    /// strip was held.
    pub fn release_key_strip(&mut self) -> bool {
        let Some(press) = self.with_prop_rows_mut(|rows| rows.strip_press.take()) else {
            return false;
        };
        if let StripHold::Key { .. } = press.hold
            && let Err(err) = self.commit_timeline_keyframe_drag()
        {
            self.report_error(&err);
        }
        true
    }

    /// Escape with a strip held: a keyframe drag is dropped with nothing
    /// written, and a scrub returns the playhead to where the press found it.
    /// Returns whether a strip was held.
    pub fn cancel_key_strip(&mut self) -> bool {
        let Some(press) = self.with_prop_rows_mut(|rows| rows.strip_press.take()) else {
            return false;
        };
        match press.hold {
            StripHold::Key { .. } => self.cancel_timeline_keyframe_drag(),
            StripHold::Scrub { from, at } => {
                if from != at {
                    self.scrub_playhead(from);
                }
            }
        }
        true
    }

    /// Whether a press on a strip is held.
    pub fn key_strip_held(&self) -> bool {
        self.with_prop_rows(|rows| rows.strip_press.is_some())
    }
}
