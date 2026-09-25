//! Curve-editor drags: one gesture over the property graph, one edit.
//!
//! The graph could be drawn and hit-tested, and nothing could be moved: the
//! panel built a throwaway drag state on every press, so the press started a
//! drag that no pointer move continued and no release committed. The state that
//! survives between events belongs to [`AppState`], the same way a speed-ramp
//! drag does, so a drag is:
//!
//! 1. `begin_curve_drag` on the press — hit-test handles first, then keyframes,
//! 2. `update_curve_drag` on every move — draft only, nothing reaches the engine,
//! 3. `commit_curve_drag` on the release — one history group, so a keyframe that
//!    moved in time (a remove plus a write) is one undo.
//!
//! Esc cancels at no cost, because a draft has written nothing.
//!
//! Pointer coordinates arrive in window pixels and the geometry is in the
//! graph's own pixels, so the panel reports where it drew the graph
//! ([`AppState::record_curve_frame`]) and every conversion happens here.

use aria_engine::EngineError;
use aria_models::{ClipParam, Easing};

use super::AppState;
use super::curve_geometry::{BezierHandleKind, CurveBounds, CurvePolyline, build_curve_polyline};

/// Hit radius in graph pixels for grabbing keyframe points and handles.
pub const HIT_RADIUS: f32 = 8.0;

/// Samples the drawn polyline carries across the clip's extent. Enough that a
/// bezier segment reads as a curve at the panel's width, and cheap enough to
/// rebuild on every repaint.
pub const CURVE_SAMPLES: usize = 80;

/// The target currently being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CurveDragTarget {
    /// Dragging a keyframe point (time and value).
    Keyframe {
        /// The tick the grabbed keyframe was committed at.
        origin_tick: i64,
        /// The value it was committed with.
        origin_val: f32,
        /// Where the pointer has it now.
        current_tick: i64,
        current_val: f32,
    },
    /// Dragging a bezier tangent handle.
    BezierHandle {
        /// The keyframe the shaped segment leaves.
        segment_tick: i64,
        kind: BezierHandleKind,
        /// The four control coordinates, with the dragged pair updated.
        points: [f32; 4],
    },
}

/// Draft state for an in-flight curve drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveDragDraft {
    pub param: ClipParam,
    /// The axis of a two-dimensional property, or `None` for a scalar.
    pub component: Option<u8>,
    pub target: CurveDragTarget,
}

impl AppState {
    /// The property the graph draws: the chosen one while it is still reachable
    /// on the selection, and otherwise the first the selection offers.
    ///
    /// Falling back rather than holding a stale choice is what keeps the block
    /// drawn after the selection changes to a clip that has no `Opacity` — the
    /// row list is derived per clip, so a remembered property is not always
    /// there.
    pub fn curve_param(&self) -> Option<ClipParam> {
        let rows = self.key_params();
        match self.curve_param {
            Some(chosen) if rows.iter().any(|row| row.param() == chosen) => Some(chosen),
            _ => rows.first().map(|row| row.param()),
        }
    }

    /// Graph another property. Cancels any drag in flight: the draft addresses
    /// the property it started on, so carrying it across would commit a value
    /// to the wrong route.
    pub fn set_curve_param(&mut self, param: ClipParam) {
        self.cancel_curve_drag();
        self.curve_param = Some(param);
        self.curve_axis = 0;
    }

    /// Graph another axis of a two-dimensional property.
    pub fn set_curve_axis(&mut self, axis: u8) {
        self.cancel_curve_drag();
        self.curve_axis = axis;
    }

    /// The axis the graph reads for `param`: `None` for a scalar property, and
    /// the chosen axis for a two-dimensional one.
    ///
    /// A colour and a rect are not graphed as one curve — three channels or
    /// four edges on one value axis is a picker's job. They report `None`, and
    /// the graph draws nothing for them rather than drawing the wrong channel.
    pub fn curve_component(&self, param: ClipParam) -> Option<u8> {
        let spec = &self.key_param(param)?.spec;
        match spec.kind {
            aria_models::ParamKind::Vec2 => Some(self.curve_axis.min(1)),
            aria_models::ParamKind::Scalar
            | aria_models::ParamKind::Color
            | aria_models::ParamKind::Rect => None,
        }
    }

    /// Record where the curve graph was last drawn (x, y, w, h in window
    /// pixels). Reports whether it moved, so the panel notifies on a resize
    /// instead of on every frame — the same rule the speed ramp follows.
    pub fn record_curve_frame(&mut self, frame: [f32; 4]) -> bool {
        let next = Some(frame);
        let moved = next != self.curve_frame;
        self.curve_frame = next;
        moved
    }

    /// Mapping bounds for the selected clip's property graph, in graph pixels.
    ///
    /// The tick axis is the clip's own extent. The value axis is the property's
    /// declared range where it has one, and otherwise the span its keyframes
    /// actually cover, padded so a flat curve is not a line on the floor — a
    /// fixed 0..1 box would put every position, rotation, or gain keyframe off
    /// the top of the graph.
    pub fn curve_bounds(&self, param: ClipParam, component: Option<u8>) -> Option<CurveBounds> {
        let [_, _, width, height] = self.curve_frame?;
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        let start = clip.timeline.start.value;
        let end = clip.timeline.end_tick();
        let (mut min_value, mut max_value) = match (spec.range.min, spec.range.max) {
            (Some(min), Some(max)) => (min, max),
            _ => {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                // The property's own keyed ticks, in the time its parameters
                // are keyed in: converting the projected absolute ticks back
                // would round-trip a speed ramp's mapping for no reason.
                for (local_tick, _) in spec.keyframes(clip) {
                    let Some(value) = super::curve_geometry::sample_param_scalar(
                        spec,
                        clip,
                        component,
                        local_tick as f64,
                    ) else {
                        continue;
                    };
                    lo = lo.min(value);
                    hi = hi.max(value);
                }
                if !lo.is_finite() || !hi.is_finite() {
                    (0.0, 1.0)
                } else {
                    let pad = ((hi - lo).abs() * 0.1).max(0.5);
                    (lo - pad, hi + pad)
                }
            }
        };
        if let Some(min) = spec.range.min {
            min_value = min_value.max(min);
        }
        if let Some(max) = spec.range.max {
            max_value = max_value.min(max);
        }
        Some(CurveBounds::new(
            start, end, min_value, max_value, width, height,
        ))
    }

    /// The polyline the graph draws, in graph pixels. `None` before the panel
    /// has been drawn once, and for a property carrying no keyframes.
    pub fn curve_polyline(&self, param: ClipParam, component: Option<u8>) -> Option<CurvePolyline> {
        let bounds = self.curve_bounds(param, component)?;
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        build_curve_polyline(spec, clip, component, bounds, CURVE_SAMPLES)
    }

    /// The drag in flight, if any.
    pub fn curve_dragging(&self) -> Option<CurveDragDraft> {
        self.curve_drag
    }

    /// A window pointer position in graph pixels, or `None` when the graph has
    /// not been drawn.
    fn curve_pointer(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let [left, top, _, _] = self.curve_frame?;
        Some((x - left, y - top))
    }

    /// Grab a bezier handle or a keyframe under the pointer. False when the
    /// press landed on neither, so the panel can fall through to its own
    /// meaning for a press on empty graph.
    ///
    /// Handles win over keyframes: a handle tip sits within a few pixels of the
    /// keyframe it belongs to at small curvature, and the handle is the smaller
    /// target.
    pub fn begin_curve_drag(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        x: f32,
        y: f32,
    ) -> bool {
        let Some((px, py)) = self.curve_pointer(x, y) else {
            return false;
        };
        let Some(poly) = self.curve_polyline(param, component) else {
            return false;
        };
        for handle in &poly.handles {
            if !within_hit(px, py, handle.handle_x, handle.handle_y) {
                continue;
            }
            let Some(key) = poly.key_points.get(handle.segment_index) else {
                continue;
            };
            self.curve_drag = Some(CurveDragDraft {
                param,
                component,
                target: CurveDragTarget::BezierHandle {
                    segment_tick: key.tick,
                    kind: handle.kind,
                    // A held keyframe has no tangents to shape; grabbing its
                    // handle starts from the linear ones.
                    points: key
                        .easing
                        .control_points()
                        .or_else(|| Easing::Linear.control_points())
                        .unwrap_or_default(),
                },
            });
            return true;
        }
        for key in &poly.key_points {
            if !within_hit(px, py, key.pixel_x, key.pixel_y) {
                continue;
            }
            self.curve_drag = Some(CurveDragDraft {
                param,
                component,
                target: CurveDragTarget::Keyframe {
                    origin_tick: key.tick,
                    origin_val: key.value,
                    current_tick: key.tick,
                    current_val: key.value,
                },
            });
            return true;
        }
        false
    }

    /// Move the in-flight draft to a new pointer position. Reports whether
    /// anything changed, so a move that lands on the same tick and value costs
    /// no repaint.
    pub fn update_curve_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(draft) = self.curve_drag else {
            return false;
        };
        let Some((px, py)) = self.curve_pointer(x, y) else {
            return false;
        };
        let Some(poly) = self.curve_polyline(draft.param, draft.component) else {
            return false;
        };
        let mut next = draft;
        let changed = match &mut next.target {
            CurveDragTarget::Keyframe {
                current_tick,
                current_val,
                ..
            } => {
                let (tick, value) = poly.bounds.from_pixel(px, py);
                if !value.is_finite() {
                    return false;
                }
                let moved = *current_tick != tick || (*current_val - value).abs() >= 1e-4;
                *current_tick = tick;
                *current_val = value;
                moved
            }
            CurveDragTarget::BezierHandle {
                segment_tick,
                kind,
                points,
            } => {
                let Some(idx) = poly
                    .key_points
                    .iter()
                    .position(|key| key.tick == *segment_tick)
                else {
                    return false;
                };
                let Some(k1) = poly.key_points.get(idx + 1) else {
                    return false;
                };
                let k0 = &poly.key_points[idx];
                let dx = k1.pixel_x - k0.pixel_x;
                let dy = k1.pixel_y - k0.pixel_y;
                if dx.abs() < 1e-3 {
                    return false;
                }
                // The control box is the segment's own bounding box: u along
                // time, v up the value axis, which is why v inverts the pixel
                // delta. A degenerate (flat) segment has no value axis to read,
                // so the handle stays mid-box rather than dividing by nothing.
                let u = ((px - k0.pixel_x) / dx).clamp(0.0, 1.0);
                let v = if dy.abs() > 1e-3 {
                    1.0 - (py - k0.pixel_y) / dy
                } else {
                    0.5
                };
                let slot = match kind {
                    BezierHandleKind::Control1 => 0,
                    BezierHandleKind::Control2 => 2,
                };
                let moved = (points[slot] - u).abs() > 1e-4 || (points[slot + 1] - v).abs() > 1e-4;
                points[slot] = u;
                points[slot + 1] = v;
                moved
            }
        };
        self.curve_drag = Some(next);
        changed
    }

    /// Commit the drag as one history entry. A release that moved nothing
    /// writes nothing, so a click on a keyframe is not an undo step.
    pub fn commit_curve_drag(&mut self) -> Result<(), EngineError> {
        let Some(draft) = self.curve_drag.take() else {
            return Ok(());
        };
        match draft.target {
            CurveDragTarget::Keyframe {
                origin_tick,
                origin_val,
                current_tick,
                current_val,
            } => {
                if origin_tick == current_tick && (origin_val - current_val).abs() < 1e-4 {
                    return Ok(());
                }
                // The segment's shape belongs to the keyframe, not to the tick
                // it sat on: read it before the move so a drag in time does not
                // silently flatten the curve leaving it.
                let easing = self
                    .keyframe_easing_at(draft.param, origin_tick)
                    .unwrap_or(Easing::Linear);
                if origin_tick == current_tick {
                    return self.set_keyframe_value_at(
                        draft.param,
                        draft.component,
                        current_tick,
                        current_val,
                        easing,
                    );
                }
                // Two commands, one gesture: group them so one undo puts the
                // keyframe back where it was.
                self.engine().lock().begin_group();
                let moved = self
                    .remove_keyframe_at(draft.param, origin_tick)
                    .and_then(|()| {
                        self.set_keyframe_value_at(
                            draft.param,
                            draft.component,
                            current_tick,
                            current_val,
                            easing,
                        )
                    });
                match moved {
                    Ok(()) => {
                        self.engine().lock().commit_group();
                        Ok(())
                    }
                    Err(err) => {
                        self.engine().lock().rollback_group();
                        self.sync_from_engine();
                        Err(err)
                    }
                }
            }
            CurveDragTarget::BezierHandle {
                segment_tick,
                points,
                ..
            } => self.set_keyframe_easing(draft.param, segment_tick, Easing::Bezier { points }),
        }
    }

    /// Abandon the drag at no cost: a draft has written nothing.
    pub fn cancel_curve_drag(&mut self) {
        self.curve_drag = None;
    }
}

/// Whether the pointer is within [`HIT_RADIUS`] of a target, in graph pixels.
fn within_hit(px: f32, py: f32, target_x: f32, target_y: f32) -> bool {
    let dx = px - target_x;
    let dy = py - target_y;
    dx * dx + dy * dy <= HIT_RADIUS * HIT_RADIUS
}
