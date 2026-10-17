//! Path masks drawn and reshaped in the Viewer.
//!
//! A path mask's vertices are layer fractions, `0..1` across the clip's quad.
//! The renderer shifts them by the mask centre and turns them by the mask
//! rotation about `0.5 + centre` in layer pixels
//! (`aria_shapes::PathRaster::rasterize_mask`). [`PathSpace`] runs that mapping
//! forward to place a vertex on screen and backward to read the pointer, so a
//! dot sits where the renderer cuts the picture.
//!
//! The path drawn and edited is the clip's active mask, when that mask is a
//! path. The Pen tool draws a new path one press at a time: a click adds a
//! corner vertex and a click-drag pulls a smooth one. Clicking the first vertex
//! or pressing Enter closes the path and adds it to the clip as a new mask, one
//! `AddClipMask` edit; Escape drops it. On the active path the Pen inserts a
//! vertex on a segment, deletes one with Ctrl+click, and turns one between
//! corner and smooth with Alt+click (an Alt+drag pulls new tangents); a Pen
//! press off it starts a new path. Any tool drags a vertex or a tangent handle.
//! Each reshape is one `SetClipMask` edit, so one undo restores the path it
//! started from.
//!
//! A keyframed path draws and edits as it stands at the playhead. Moving a
//! vertex or a tangent, or turning a vertex, keys the path there. Inserting or
//! deleting a vertex changes every keyframe of the path, so the keyframes keep
//! one vertex count and the shape keeps moving between them.

use std::borrow::Cow;

use aria_engine::EngineError;
use aria_models::{ClipId, MAX_MASK_PATH_POINTS, Mask, MaskKind, ShapePathPoint};

use super::AppState;
use super::canvas_overlay::motion_path::TangentSide;
use crate::surfaces::overlay_gesture::Gesture;
use crate::types::{OverlayMode, PenVariant};

#[path = "mask_path_geometry.rs"]
mod geometry;
use geometry::*;
pub use geometry::{
    MIN_MASK_VERTICES, PathSpace, SEGMENT_HIT, is_smooth, segment_hit, split_segment, toggle_smooth,
};

/// What a press on a mask path holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathHold {
    /// A vertex the pointer moves, with where the press landed and the vertex
    /// as it was, both in path space.
    Vertex {
        index: usize,
        grab: [f32; 2],
        start: ShapePathPoint,
    },
    /// One tangent handle of a vertex.
    Tangent { index: usize, side: TangentSide },
    /// A vertex whose tangents the pointer pulls out, mirrored about its anchor.
    Pull { index: usize },
}

/// A mask path being drawn with the Pen or reshaped.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskPathEdit {
    /// The clip the path belongs to. A selection change drops the edit.
    pub clip: ClipId,
    /// The path as drafted, in path space.
    pub points: Vec<ShapePathPoint>,
    /// A new path the Pen is drawing, which is not on the clip yet.
    pub drawing: bool,
    /// Whether the path closes back to its first vertex. A path being drawn
    /// is open until it is finished.
    pub closed: bool,
    /// What the pointer holds, or `None` between presses.
    pub hold: Option<PathHold>,
    /// Where the press landed, in window pixels.
    pub pressed_at: [f32; 2],
    /// Where the pointer is, in window pixels: the end of the rubber band.
    pub pointer: Option<[f32; 2]>,
    /// Whether the pointer left the click slop since the press.
    pub moved: bool,
    /// An Alt+press: a release that never moved turns the vertex between corner
    /// and smooth.
    pub convert: bool,
    /// The segment a Pen press on the committed path split, as the vertex it
    /// leaves and how far along it: every keyframe of the path gains the vertex
    /// there when the edit commits.
    pub split: Option<(usize, f32)>,
}

/// One vertex of the drawn path, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnVertex {
    pub at: [f32; 2],
    pub handle_in: [f32; 2],
    pub handle_out: [f32; 2],
    pub smooth: bool,
}

impl DrawnVertex {
    /// The tangent handle on `side`, or `None` on a corner.
    pub fn handle(&self, side: TangentSide) -> Option<[f32; 2]> {
        self.smooth.then_some(match side {
            TangentSide::In => self.handle_in,
            TangentSide::Out => self.handle_out,
        })
    }
}

/// A mask path as the Viewer draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnMaskPath {
    pub vertices: Vec<DrawnVertex>,
    pub closed: bool,
    /// A new path the Pen is drawing.
    pub drawing: bool,
    /// From the last vertex to the pointer, while drawing.
    pub rubber: Option<[[f32; 2]; 2]>,
}

impl AppState {
    /// The selected clip's active mask, when it is a path with vertices.
    fn committed_path_mask(&self) -> Option<&Mask> {
        self.active_model_mask()
            .filter(|mask| mask.kind == MaskKind::Path && mask.has_path())
    }

    /// The active mask's committed path at the playhead and whether it is
    /// closed, when the active mask is a path.
    fn committed_mask_path(&self) -> Option<(Cow<'_, [ShapePathPoint]>, bool)> {
        let tick = self.selected_param_tick()?;
        self.committed_path_mask()
            .map(|mask| (mask.path_at(tick as f64), mask.path_closed))
    }

    /// The space the selected clip's mask path draws in, at the playhead: the
    /// active path's centre and rotation, or unshifted and unturned for a new
    /// path, which is what `Mask::from_path` gives it.
    pub fn mask_path_space(&self) -> Option<PathSpace> {
        if self.mask_path_drawing() {
            return self.new_mask_path_space();
        }
        let layer = self.overlay_frame.layer?;
        let model = self.selected_model()?;
        let tick = model.param_tick(self.playhead.value);
        let (center, rotation) = self
            .committed_path_mask()
            .map_or(([0.0, 0.0], 0.0), |mask| {
                (mask.center.sample(tick), mask.rotation.sample(tick))
            });
        Some(PathSpace::new(layer, center, rotation))
    }

    /// The space a new path draws in: unshifted and unturned.
    fn new_mask_path_space(&self) -> Option<PathSpace> {
        self.selected_clip()?;
        Some(PathSpace::new(self.overlay_frame.layer?, [0.0, 0.0], 0.0))
    }

    /// The edit in flight for the selected clip. An edit left from another
    /// selection is not this clip's.
    fn mask_path_edit(&self) -> Option<&MaskPathEdit> {
        let clip = self.selected_clip()?;
        self.tool_session
            .mask_path
            .as_ref()
            .filter(|edit| edit.clip == clip)
    }

    /// Whether the Viewer draws mask path controls: with the Pen on a framed
    /// clip, or with the Mask handle set over a path mask.
    pub fn mask_path_live(&self) -> bool {
        self.selected_has_framing()
            && self.overlay_frame.layer.is_some()
            && (self.tool.pens()
                || (self.overlay_mode() == OverlayMode::Mask
                    && self.committed_mask_path().is_some()))
    }

    /// Whether the Pen is drawing a new path for the selected clip.
    pub fn mask_path_drawing(&self) -> bool {
        self.mask_path_edit().is_some_and(|edit| edit.drawing)
    }

    /// Whether a press on a mask path is in flight.
    pub fn mask_path_holding(&self) -> bool {
        self.mask_path_edit()
            .is_some_and(|edit| edit.hold.is_some())
    }

    /// The selected clip's mask path as the Viewer draws it: the draft in
    /// flight, else the committed path. `None` with no path to draw.
    pub fn drawn_mask_path(&self) -> Option<DrawnMaskPath> {
        let space = self.mask_path_space()?;
        let (points, closed, drawing, pointer) = match self.mask_path_edit() {
            Some(edit) => (
                Cow::Borrowed(edit.points.as_slice()),
                edit.closed,
                edit.drawing,
                edit.pointer,
            ),
            None => {
                let (points, closed) = self.committed_mask_path()?;
                (points, closed, false, None)
            }
        };
        let vertices: Vec<DrawnVertex> = points
            .iter()
            .map(|vertex| DrawnVertex {
                at: space.to_window(vertex.anchor),
                handle_in: space.to_window(vertex.handle_in),
                handle_out: space.to_window(vertex.handle_out),
                smooth: is_smooth(vertex),
            })
            .collect();
        let rubber = drawing
            .then(|| Some([vertices.last()?.at, pointer?]))
            .flatten();
        Some(DrawnMaskPath {
            vertices,
            closed,
            drawing,
            rubber,
        })
    }

    /// Whether a Pen press at window point `at` belongs to the mask path: on
    /// the picture, and not a Ctrl+click the motion path takes.
    pub fn pen_takes_press(&self, at: [f32; 2], ctrl: bool) -> bool {
        if !self.tool.pens() || !self.mask_path_live() {
            return false;
        }
        if ctrl && self.can_add_key_on_path(ctrl, at) {
            return false;
        }
        self.overlay_frame.canvas.is_some_and(|canvas| {
            at[0] >= canvas.x
                && at[1] >= canvas.y
                && at[0] <= canvas.x + canvas.w
                && at[1] <= canvas.y + canvas.h
        })
    }

    /// The gesture a press on vertex `index` makes, from the tool, its
    /// variant, and the modifiers held: the Pen deletes with Ctrl (or the
    /// Delete Vertex tool) and converts with Alt (or the Convert Vertex tool);
    /// everything else drags.
    pub fn mask_vertex_gesture(&self, index: usize, ctrl: bool, alt: bool) -> Gesture {
        if !self.tool.pens() {
            return Gesture::MaskVertex(index);
        }
        match self.pen_variant() {
            PenVariant::DeleteVertex => Gesture::MaskVertexDelete(index),
            PenVariant::ConvertVertex => Gesture::MaskVertexConvert(index),
            PenVariant::Standard | PenVariant::AddVertex if ctrl => {
                Gesture::MaskVertexDelete(index)
            }
            PenVariant::Standard | PenVariant::AddVertex if alt => {
                Gesture::MaskVertexConvert(index)
            }
            PenVariant::Standard | PenVariant::AddVertex => Gesture::MaskVertex(index),
        }
    }

    /// Start an edit of the committed path, or carry on the draft.
    fn open_mask_path_edit(&mut self, at: [f32; 2]) -> Option<&mut MaskPathEdit> {
        let clip = self.selected_clip()?;
        if self.mask_path_edit().is_none() {
            let (points, closed) = self.committed_mask_path()?;
            let points = points.into_owned();
            self.tool_session.mask_path = Some(MaskPathEdit {
                clip,
                points,
                drawing: false,
                closed,
                hold: None,
                pressed_at: at,
                pointer: Some(at),
                moved: false,
                convert: false,
                split: None,
            });
        }
        let edit = self.tool_session.mask_path.as_mut()?;
        edit.pressed_at = at;
        edit.pointer = Some(at);
        edit.moved = false;
        edit.convert = false;
        Some(edit)
    }

    /// A Pen press on the picture at window point `at`: the next vertex of the
    /// path being drawn, a vertex inserted on a segment of the active path, or
    /// the first vertex of a new path anywhere else.
    pub fn press_mask_path(&mut self, at: [f32; 2]) -> Result<(), EngineError> {
        let (Some(space), Some(clip)) = (self.mask_path_space(), self.selected_clip()) else {
            return Ok(());
        };
        if self.mask_path_drawing() {
            let to = space.to_path(at);
            let Some(edit) = self.open_mask_path_edit(at) else {
                return Ok(());
            };
            if edit.points.len() >= MAX_MASK_PATH_POINTS {
                self.status_message = Some(format!(
                    "A mask path holds at most {MAX_MASK_PATH_POINTS} vertices"
                ));
                return Ok(());
            }
            edit.points.push(ShapePathPoint::corner(to));
            edit.hold = Some(PathHold::Pull {
                index: edit.points.len() - 1,
            });
            return Ok(());
        }
        let hit = self.committed_mask_path().and_then(|(points, closed)| {
            let hit = segment_hit(&points, closed, space, at)?;
            Some((points, hit))
        });
        let Some((points, (from, t))) = hit else {
            // Only the Pen starts a path; the vertex tools edit one that exists.
            if self.pen_variant() == PenVariant::Standard
                && let Some(space) = self.new_mask_path_space()
            {
                self.tool_session.mask_path = Some(MaskPathEdit {
                    clip,
                    points: vec![ShapePathPoint::corner(space.to_path(at))],
                    drawing: true,
                    closed: false,
                    hold: Some(PathHold::Pull { index: 0 }),
                    pressed_at: at,
                    pointer: Some(at),
                    moved: false,
                    convert: false,
                    split: None,
                });
            }
            return Ok(());
        };
        let mut points = points.into_owned();
        let to = space.to_path(at);
        if points.len() >= MAX_MASK_PATH_POINTS {
            self.status_message = Some(format!(
                "A mask path holds at most {MAX_MASK_PATH_POINTS} vertices"
            ));
            return Ok(());
        }
        let index = split_segment(&mut points, from, t);
        let start = points[index];
        if let Some(edit) = self.open_mask_path_edit(at) {
            edit.points = points;
            edit.split = Some((from, t));
            edit.hold = Some(PathHold::Vertex {
                index,
                grab: to,
                start,
            });
        }
        Ok(())
    }

    /// A press on vertex `index` that drags it. On the path being drawn, the
    /// first vertex closes the path once it has [`MIN_MASK_VERTICES`].
    pub fn press_mask_vertex(&mut self, index: usize, at: [f32; 2]) -> Result<(), EngineError> {
        if self.mask_path_drawing()
            && index == 0
            && self
                .mask_path_edit()
                .is_some_and(|edit| edit.points.len() >= MIN_MASK_VERTICES)
        {
            return self.finish_mask_path();
        }
        let Some(space) = self.mask_path_space() else {
            return Ok(());
        };
        let grab = space.to_path(at);
        let Some(edit) = self.open_mask_path_edit(at) else {
            return Ok(());
        };
        let Some(start) = edit.points.get(index).copied() else {
            return Ok(());
        };
        edit.hold = Some(PathHold::Vertex { index, grab, start });
        Ok(())
    }

    /// A press on one tangent handle of vertex `index`.
    pub fn press_mask_tangent(&mut self, index: usize, side: TangentSide, at: [f32; 2]) {
        let Some(edit) = self.open_mask_path_edit(at) else {
            return;
        };
        if edit.points.get(index).is_some_and(is_smooth) {
            edit.hold = Some(PathHold::Tangent { index, side });
        }
    }

    /// An Alt+press on vertex `index`: a click turns it between corner and
    /// smooth, and a drag pulls new tangents out of it.
    pub fn press_mask_convert(&mut self, index: usize, at: [f32; 2]) {
        let Some(edit) = self.open_mask_path_edit(at) else {
            return;
        };
        if index < edit.points.len() {
            edit.hold = Some(PathHold::Pull { index });
            edit.convert = true;
        }
    }

    /// A Ctrl+click with the Pen on vertex `index`: the vertex goes from the
    /// path and from every keyframe of it, unless the committed path would keep
    /// fewer than [`MIN_MASK_VERTICES`].
    pub fn delete_mask_vertex(&mut self, index: usize) -> Result<(), EngineError> {
        if self.mask_path_drawing() {
            if let Some(edit) = self.tool_session.mask_path.as_mut()
                && index < edit.points.len()
            {
                edit.points.remove(index);
            }
            if self
                .tool_session
                .mask_path
                .as_ref()
                .is_some_and(|edit| edit.points.is_empty())
            {
                self.tool_session.mask_path = None;
            }
            return Ok(());
        }
        let Some((count, closed)) = self
            .committed_mask_path()
            .map(|(points, closed)| (points.len(), closed))
        else {
            return Ok(());
        };
        if index >= count {
            return Ok(());
        }
        if count <= MIN_MASK_VERTICES {
            self.status_message = Some(format!(
                "A mask path keeps at least {MIN_MASK_VERTICES} vertices"
            ));
            return Ok(());
        }
        self.reshape_selected_mask_path(closed, |mask, _| {
            mask.edit_every_path(|points| {
                if index < points.len() && points.len() > MIN_MASK_VERTICES {
                    points.remove(index);
                }
            });
        })
    }

    /// Carry what the pointer holds to window point `at`. `false` when no
    /// press on a mask path is in flight.
    pub(super) fn carry_mask_path(&mut self, at: [f32; 2]) -> bool {
        let Some(space) = self.mask_path_space() else {
            return false;
        };
        let Some(edit) = self.tool_session.mask_path.as_mut() else {
            return false;
        };
        let Some(hold) = edit.hold else {
            return false;
        };
        edit.pointer = Some(at);
        edit.moved |= length(sub(at, edit.pressed_at)) > CLICK_SLOP;
        let to = space.to_path(at);
        match hold {
            PathHold::Vertex { index, grab, start } => {
                let shift = sub(to, grab);
                if let Some(vertex) = edit.points.get_mut(index) {
                    *vertex = ShapePathPoint {
                        anchor: add(start.anchor, shift),
                        handle_in: add(start.handle_in, shift),
                        handle_out: add(start.handle_out, shift),
                    };
                }
            }
            PathHold::Tangent { index, side } => {
                if let Some(vertex) = edit.points.get_mut(index) {
                    set_handle(vertex, side, to);
                }
            }
            PathHold::Pull { index } => {
                if edit.moved
                    && let Some(vertex) = edit.points.get_mut(index)
                {
                    pull(vertex, to);
                }
            }
        }
        true
    }

    /// Follow the pointer with the rubber band while a path is being drawn.
    /// Returns whether the Viewer needs a repaint.
    pub fn hover_mask_path(&mut self, at: [f32; 2]) -> bool {
        if !self.mask_path_drawing() {
            return false;
        }
        match self.tool_session.mask_path.as_mut() {
            Some(edit) if edit.pointer != Some(at) => {
                edit.pointer = Some(at);
                true
            }
            _ => false,
        }
    }

    /// Let go of the mask path. An Alt+click turns the vertex it pressed
    /// between corner and smooth. The path being drawn keeps its draft; a
    /// reshaped path commits as one edit, or nothing when it did not change.
    pub(super) fn release_mask_path(&mut self) -> Result<(), EngineError> {
        let selected = self.selected_clip();
        let Some(edit) = self.tool_session.mask_path.as_mut() else {
            return Ok(());
        };
        if Some(edit.clip) != selected {
            self.tool_session.mask_path = None;
            return Ok(());
        }
        let Some(hold) = edit.hold.take() else {
            return Ok(());
        };
        if edit.convert
            && !edit.moved
            && let PathHold::Pull { index } = hold
        {
            toggle_smooth(&mut edit.points, index);
        }
        if edit.drawing {
            return Ok(());
        }
        let Some(edit) = self.tool_session.mask_path.take() else {
            return Ok(());
        };
        let changed = self
            .committed_mask_path()
            .is_some_and(|(committed, _)| *committed != *edit.points);
        if !changed {
            return Ok(());
        }
        let MaskPathEdit {
            points,
            closed,
            split,
            ..
        } = edit;
        self.reshape_selected_mask_path(closed, |mask, tick| {
            if let Some((from, t)) = split {
                // The vertex every other keyframe gains where the Pen split the
                // path at the playhead, so the keyframes keep one vertex count.
                mask.edit_every_path(|path| {
                    if from < path.len() {
                        split_segment(path, from, t);
                    }
                });
            }
            mask.set_path_at(tick, points);
        })
    }

    /// Close the path being drawn and add it to the clip as a new mask, as one
    /// edit. A path with fewer than [`MIN_MASK_VERTICES`] encloses nothing and
    /// stays a draft.
    pub fn finish_mask_path(&mut self) -> Result<(), EngineError> {
        let Some(count) = self
            .mask_path_edit()
            .filter(|edit| edit.drawing)
            .map(|edit| edit.points.len())
        else {
            return Ok(());
        };
        if count < MIN_MASK_VERTICES {
            self.status_message = Some(format!(
                "A mask path needs at least {MIN_MASK_VERTICES} vertices"
            ));
            return Ok(());
        }
        let Some(edit) = self.tool_session.mask_path.take() else {
            return Ok(());
        };
        self.add_selected_mask_path(edit.points)?;
        self.status_message = Some("Mask path closed".into());
        Ok(())
    }

    /// Drop the path being drawn and any press in flight.
    pub fn cancel_mask_path(&mut self) {
        self.tool_session.mask_path = None;
    }
}

#[cfg(test)]
#[path = "mask_path_tests.rs"]
pub(super) mod tests;
