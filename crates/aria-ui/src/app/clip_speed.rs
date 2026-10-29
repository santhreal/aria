//! Speed-ramp commands: the preset catalog and the ramp's own keyframes.
//!
//! `SetSpeedCurve` had no operator path, so only flat speed was reachable and the
//! `speed_preset_catalog` was never browsed. A ramp is a normalized curve over
//! `0..=SPEED_CURVE_SCALE`, independent of the clip's length, so the setters here
//! address it by fraction of the clip rather than by tick: that is what survives a
//! trim.
//!
//! The engine re-derives the clip's timeline duration from the ramp, and refuses a
//! ramp that would overlap a neighbour, so a refusal here is a real overlap and is
//! reported rather than swallowed.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Easing, Keyframe, MAX_SPEED, MIN_SPEED, Param, SPEED_CURVE_SCALE, SpeedPresetSpec,
    speed_preset, speed_preset_catalog, speed_preset_id,
};

use super::AppState;

/// A ramp point under the pointer: where it started, and where the drag has it
/// now. A draft — nothing reaches the engine until the release, so one drag is
/// one edit and Esc costs nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RampDrag {
    /// The tick the grabbed point was committed at.
    pub origin: i64,
    /// Where the pointer has it now. Equal to `origin` for the end points.
    pub tick: i64,
    /// The multiplier the pointer has it at, already clamped.
    pub value: f32,
}

/// The three points a preset-free ramp is seeded with: start, middle, end. A
/// hand-built ramp starts here and grows by adding points on the graph.
pub(crate) const RAMP_POINTS: [(&str, &str, i64); 3] = [
    ("ramp-in", "Start", 0),
    ("ramp-mid", "Middle", SPEED_CURVE_SCALE / 2),
    ("ramp-out", "End", SPEED_CURVE_SCALE),
];

/// Fewest points a ramp can hold: a curve needs two to have a shape, so removing
/// the second-to-last point is refused rather than collapsing the ramp silently.
pub(crate) const RAMP_MIN_POINTS: usize = 2;

/// How close, in normalized ticks, a click has to land to grab an existing point
/// rather than add one. Scaled from the graph being roughly 240px wide.
pub(crate) const RAMP_GRAB_TICKS: i64 = 24;

/// Every built-in ramp, for a preset row.
pub(crate) fn speed_presets() -> &'static [SpeedPresetSpec] {
    speed_preset_catalog()
}

/// A graph x fraction as a normalized curve tick.
pub(crate) fn ramp_frac_to_tick(frac: f32) -> i64 {
    let clamped = frac.clamp(0.0, 1.0);
    (clamped * SPEED_CURVE_SCALE as f32).round() as i64
}

/// A normalized curve tick as a graph x fraction.
pub(crate) fn ramp_tick_to_frac(tick: i64) -> f32 {
    tick.clamp(0, SPEED_CURVE_SCALE) as f32 / SPEED_CURVE_SCALE as f32
}

/// A speed multiplier as a graph y fraction, 0 at the bottom.
///
/// Logarithmic, because the bounds span 0.05× to 100× and a linear axis would put
/// every useful speed in the bottom two percent of the graph. 1× sits where
/// `log(1)` falls between the bounds.
pub(crate) fn ramp_value_to_frac(value: f32) -> f32 {
    let lo = MIN_SPEED.ln();
    let hi = MAX_SPEED.ln();
    ((value.clamp(MIN_SPEED, MAX_SPEED).ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// A graph y fraction as a speed multiplier. Inverse of [`ramp_value_to_frac`].
pub(crate) fn ramp_frac_to_value(frac: f32) -> f32 {
    let lo = MIN_SPEED.ln();
    let hi = MAX_SPEED.ln();
    (lo + frac.clamp(0.0, 1.0) * (hi - lo))
        .exp()
        .clamp(MIN_SPEED, MAX_SPEED)
}

/// The point a click at `tick` grabs, if one is within [`RAMP_GRAB_TICKS`].
///
/// Nearest wins, so two points closer together than the tolerance are still both
/// reachable from their own side.
pub(crate) fn ramp_point_at(points: &[(i64, f32)], tick: i64) -> Option<i64> {
    points
        .iter()
        .map(|(point, _)| *point)
        .filter(|point| (point - tick).abs() <= RAMP_GRAB_TICKS)
        .min_by_key(|point| (point - tick).abs())
}

impl AppState {
    /// Whether the selected clip can carry a ramp at all. The model refuses one on
    /// a generated clip, so the surface must not offer it there.
    pub fn selected_can_ramp(&self) -> bool {
        self.selected_ui_clip()
            .map(|clip| clip.media_id.is_some() && clip.generator.is_none())
            .unwrap_or(false)
    }

    /// The selected clip's ramp. A flat clip carries a constant unit curve, so an
    /// "is there a ramp" question is `is_animated`, not `is_some`.
    fn selected_curve(&self) -> Option<Param<f32>> {
        Some(self.selected_model()?.speed_curve.clone())
    }

    /// Whether the selected clip carries a speed ramp rather than flat speed.
    pub fn selected_has_ramp(&self) -> bool {
        self.selected_curve()
            .map(|curve| curve.is_animated())
            .unwrap_or(false)
    }

    /// The id of the preset the selected clip's ramp matches, if any. A
    /// hand-edited ramp matches nothing, which is how a preset row shows that it
    /// is no longer the active shape.
    pub fn selected_speed_preset(&self) -> Option<&'static str> {
        let curve = self.selected_curve()?;
        if !curve.is_animated() {
            return None;
        }
        speed_preset_id(&curve)
    }

    /// The ramp's multiplier at each of `RAMP_POINTS`, for the readout. Empty
    /// when the clip has flat speed.
    pub fn selected_ramp_values(&self) -> Vec<(&'static str, &'static str, f32)> {
        let Some(curve) = self.selected_curve().filter(Param::is_animated) else {
            return Vec::new();
        };
        RAMP_POINTS
            .iter()
            .map(|(id, label, tick)| (*id, *label, curve.sample(*tick)))
            .collect()
    }

    /// Apply a catalog ramp. Inert on an unknown id and on a clip that cannot
    /// carry one.
    pub fn set_selected_speed_preset(&mut self, preset: &str) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.selected_can_ramp() {
            return Ok(());
        }
        let Some(curve) = speed_preset(preset) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
            clip,
            curve: Some(curve),
        }))?;
        Ok(())
    }

    /// Clear the ramp back to flat speed. Inert when there is none.
    pub fn clear_selected_speed_curve(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.selected_has_ramp() {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
            clip,
            curve: None,
        }))?;
        Ok(())
    }

    /// Set the ramp's multiplier at one normalized point, clamped to the model's
    /// speed bounds. A flat clip gains a three-point ramp seeded at its current
    /// speed, so the first drag does not jump.
    pub fn set_selected_ramp_point(&mut self, tick: i64, value: f32) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        if !self.selected_can_ramp() || !value.is_finite() {
            return Ok(());
        }
        if !(0..=SPEED_CURVE_SCALE).contains(&tick) {
            return Ok(());
        }
        let value = value.clamp(MIN_SPEED, MAX_SPEED);
        let mut curve = match self.selected_curve() {
            Some(curve) if curve.is_animated() => curve,
            // A flat clip starts from a unit ramp so the untouched points stay at
            // 1× and only the dragged one moves.
            _ => Param::Keyframed {
                keyframes: RAMP_POINTS
                    .iter()
                    .map(|(_, _, point)| Keyframe::new(*point, 1.0, Easing::Linear))
                    .collect(),
            },
        };
        curve.set_keyframe(tick, value, Easing::Linear);
        self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
            clip,
            curve: Some(curve),
        }))?;
        Ok(())
    }

    /// Nudge the multiplier at one normalized point.
    pub fn nudge_selected_ramp_point(&mut self, tick: i64, step: f32) -> Result<(), EngineError> {
        let current = self
            .selected_curve()
            .filter(Param::is_animated)
            .map(|curve| curve.sample(tick))
            .unwrap_or(1.0);
        self.set_selected_ramp_point(tick, current + step)
    }

    /// The ramp's real keyframes in tick order, with a drag draft applied. Empty
    /// when the clip has flat speed and nothing is being dragged.
    ///
    /// This is what the graph draws: sampled values would hide where the points
    /// actually sit.
    pub fn selected_ramp_curve(&self) -> Vec<(i64, f32)> {
        let mut points: Vec<(i64, f32)> = self
            .selected_curve()
            .filter(Param::is_animated)
            .map(|curve| {
                curve
                    .keyframes()
                    .iter()
                    .map(|frame| (frame.tick, frame.value))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(drag) = self.ramp_drag {
            points.retain(|(tick, _)| *tick != drag.origin && *tick != drag.tick);
            points.push((drag.tick, drag.value));
            points.sort_by_key(|(tick, _)| *tick);
        }
        points
    }

    /// The point being dragged, if any.
    pub fn ramp_dragging(&self) -> Option<RampDrag> {
        self.ramp_drag
    }

    /// Grab the point at `tick`. False when no point sits there, so a press on
    /// empty graph can fall through to adding one.
    pub fn begin_ramp_drag(&mut self, tick: i64) -> bool {
        if !self.selected_can_ramp() {
            return false;
        }
        let points = self.selected_ramp_curve();
        let Some(origin) = ramp_point_at(&points, tick) else {
            return false;
        };
        let value = points
            .iter()
            .find(|(point, _)| *point == origin)
            .map(|(_, value)| *value)
            .unwrap_or(1.0);
        self.ramp_drag = Some(RampDrag {
            origin,
            tick: origin,
            value,
        });
        true
    }

    /// Move the dragged point. Draft only — nothing is committed until release,
    /// so a drag is one edit. Returns whether the draft moved.
    ///
    /// The first and last point keep their tick: a ramp spans the whole clip, and
    /// dragging an end inwards would leave the curve undefined past it.
    pub fn update_ramp_drag(&mut self, tick: i64, value: f32) -> bool {
        let Some(mut drag) = self.ramp_drag else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let pinned = drag.origin == 0 || drag.origin == SPEED_CURVE_SCALE;
        let tick = if pinned {
            drag.origin
        } else {
            tick.clamp(1, SPEED_CURVE_SCALE - 1)
        };
        let value = value.clamp(MIN_SPEED, MAX_SPEED);
        if drag.tick == tick && (drag.value - value).abs() < f32::EPSILON {
            return false;
        }
        drag.tick = tick;
        drag.value = value;
        self.ramp_drag = Some(drag);
        true
    }

    /// Commit the dragged point. A release that moved nothing writes nothing.
    pub fn commit_ramp_drag(&mut self) -> Result<(), EngineError> {
        let Some(drag) = self.ramp_drag.take() else {
            return Ok(());
        };
        let committed = self
            .selected_ramp_curve()
            .into_iter()
            .find(|(tick, _)| *tick == drag.origin);
        if let Some((_, value)) = committed
            && drag.tick == drag.origin
            && (value - drag.value).abs() < f32::EPSILON
        {
            return Ok(());
        }
        if drag.tick != drag.origin {
            self.remove_selected_ramp_point(drag.origin)?;
        }
        self.set_selected_ramp_point(drag.tick, drag.value)
    }

    /// Abandon the drag at no cost.
    pub fn cancel_ramp_drag(&mut self) {
        self.ramp_drag = None;
    }

    /// Drop one ramp point. Inert when no point sits at `tick`; refused with a
    /// message when the ramp is down to `RAMP_MIN_POINTS`, because a curve with
    /// one point is a constant wearing a ramp's clothes.
    pub fn remove_selected_ramp_point(&mut self, tick: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(mut curve) = self.selected_curve().filter(Param::is_animated) else {
            return Ok(());
        };
        if !curve.keyframes().iter().any(|frame| frame.tick == tick) {
            return Ok(());
        }
        if curve.keyframes().len() <= RAMP_MIN_POINTS {
            self.status_message = Some("A ramp needs two points; clear it instead".into());
            return Ok(());
        }
        curve.remove_keyframe(tick);
        self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
            clip,
            curve: Some(curve),
        }))?;
        Ok(())
    }

    /// Timeline ticks the selected clip would occupy under the ramp as drawn,
    /// draft included. `None` on a clip that cannot be retimed.
    ///
    /// The model owns the rule, so the readout cannot drift from the commit.
    pub fn selected_ramp_length_ticks(&self) -> Option<i64> {
        let points = self.selected_ramp_curve();
        let curve = if points.is_empty() {
            Param::Constant(1.0)
        } else {
            Param::Keyframed {
                keyframes: points
                    .into_iter()
                    .map(|(tick, value)| Keyframe::new(tick, value, Easing::Linear))
                    .collect(),
            }
        };
        self.selected_model()?
            .duration_under(&curve, self.store.sequence.frame_rate)
    }

    /// Record the graph rectangle the Speed tab painted, in window pixels
    /// `[x, y, w, h]`. Returns whether it moved, so the panel notifies once on a
    /// resize instead of every frame.
    pub fn record_ramp_frame(&mut self, frame: [f32; 4]) -> bool {
        let next = Some(frame);
        let moved = next != self.ramp_frame;
        self.ramp_frame = next;
        moved
    }

    /// A pointer position in window pixels as a point on the ramp graph: a
    /// normalized tick and a multiplier. `None` before the graph has been
    /// measured or when it has no area.
    ///
    /// Fast is up, so the y axis is inverted here rather than at every call site.
    pub fn ramp_at_pointer(&self, x: f32, y: f32) -> Option<(i64, f32)> {
        let [left, top, width, height] = self.ramp_frame?;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let tick = ramp_frac_to_tick((x - left) / width);
        let value = ramp_frac_to_value(1.0 - (y - top) / height);
        Some((tick, value))
    }
}
