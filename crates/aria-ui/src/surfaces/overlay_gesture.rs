//! What a drag on the preview's picture means: move it, scale it from a corner,
//! pull a crop edge, move, size and turn a mask, draw and reshape a mask path,
//! move a clip type handle, or reshape a motion path.
//!
//! The three handle sets were three `cx.listener` closures calling three
//! different `begin_*_drag` methods, and which set was live was decided by a
//! `match` on the overlay mode inside the panel. `Transform` was one of them, so
//! the command that moves the picture on screen had no surface a census could
//! press: a corner wired to the crop verb is invisible by inspection, because it
//! draws in the right place and edits the wrong thing.
//!
//! A gesture names the set and the handle. The panel places the dots, because
//! only the panel knows where they are on screen, and hands the pointer position
//! straight through; the arithmetic was already in
//! [`crate::app::canvas_overlay`] and stays there. What this adds is one table
//! naming every draggable thing on the canvas, and one function that says which
//! verb each one runs.
//!
//! A drag is three steps: the press names the handle, the moves draft it, and
//! the release is the only one that edits. That is what makes a gesture one undo
//! entry rather than one per frame, and why the draft is shown through the
//! engine's overrides instead of the project.
//!
//! The handle-set picker is not here. Choosing which set is live changes no
//! project state, and the sets are mutually exclusive, so which one a press
//! means is the mode's answer and never geometry: a crop corner and a scale
//! corner land on the same pixel when the crop is full.

use aria_engine::EngineError;

use crate::app::AppState;
use crate::app::canvas_overlay::motion_path::{PathKey, TangentSide};
use crate::app::canvas_overlay::{CropHandle, MaskHandle, TransformHandle};
use crate::types::{ActivePanel, OverlayMode};

/// One draggable thing on the picture.
///
/// The handle enums are the catalogs the panel loops over, so a fourth crop
/// handle or a second mask control is drawn and driven without a list here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// The picture itself: pressing it and dragging moves the layer.
    Move,
    /// One of the four corners, which scales the layer.
    Scale(TransformHandle),
    /// One edge or corner of the crop window.
    Crop(CropHandle),
    /// The mask's centre, its size, or its rotation.
    Mask(MaskHandle),
    /// The selected clip's clip type handle at this index, which moves a vec2
    /// parameter of the type's generator.
    TypeHandle(usize),
    /// A motion path keyframe's square, which moves the keyframe's value.
    PathKey(PathKey),
    /// One tangent handle of a smooth motion path keyframe, which bends the
    /// path through the keyframe.
    PathTangent(PathKey, TangentSide),
    /// An Alt+click on a motion path keyframe, which turns it between corner
    /// and smooth. The press is the edit.
    PathCorner(PathKey),
    /// A Ctrl+click on a motion path, which adds a keyframe at the frame under
    /// the pointer. The press is the edit.
    PathAdd,
    /// A mask path vertex, which moves with its tangents.
    MaskVertex(usize),
    /// One tangent handle of a smooth mask path vertex, which bends the path
    /// through the vertex.
    MaskTangent(usize, TangentSide),
    /// A Ctrl+click with the Pen on a mask path vertex, which deletes it. The
    /// press is the edit.
    MaskVertexDelete(usize),
    /// An Alt+press with the Pen on a mask path vertex: a click turns it
    /// between corner and smooth, and a drag pulls new tangents out of it.
    MaskVertexConvert(usize),
    /// A Pen press on the picture: the next vertex of a path being drawn, a
    /// vertex inserted on a segment of the mask path, or the first vertex of a
    /// new path.
    MaskPathPen,
}

impl Gesture {
    /// The grabbed handle, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Move => "the picture dragged to a new position",
            Self::Scale(_) => "a corner dragged to scale the picture",
            Self::Crop(_) => "a crop edge dragged",
            Self::Mask(_) => "a mask handle dragged",
            Self::TypeHandle(_) => "a clip type handle dragged",
            Self::PathKey(_) => "a motion path keyframe dragged to a new value",
            Self::PathTangent(..) => "a motion path tangent handle dragged to bend the path",
            Self::PathCorner(_) => "a motion path keyframe turned between corner and smooth",
            Self::PathAdd => "a motion path clicked to add a keyframe",
            Self::MaskVertex(_) => "a mask path vertex dragged",
            Self::MaskTangent(..) => "a mask path tangent handle dragged to bend the path",
            Self::MaskVertexDelete(_) => {
                "a mask path vertex Ctrl+clicked with the Pen to delete it"
            }
            Self::MaskVertexConvert(_) => "a mask path vertex Alt+dragged to pull new tangents",
            Self::MaskPathPen => "a mask path segment pressed with the Pen to insert a vertex",
        }
    }

    /// The handle set this gesture belongs to, which is the mode that draws it.
    pub fn mode(&self) -> OverlayMode {
        match self {
            Self::Move
            | Self::Scale(_)
            | Self::TypeHandle(_)
            | Self::PathKey(_)
            | Self::PathTangent(..)
            | Self::PathCorner(_)
            | Self::PathAdd => OverlayMode::Transform,
            Self::Crop(_) => OverlayMode::Crop,
            Self::Mask(_)
            | Self::MaskVertex(_)
            | Self::MaskTangent(..)
            | Self::MaskVertexDelete(_)
            | Self::MaskVertexConvert(_)
            | Self::MaskPathPen => OverlayMode::Mask,
        }
    }
}

/// One step of a drag.
///
/// Only the press carries a gesture: after it, the state holds what was taken
/// hold of, which is why a pointer move can be routed without the panel
/// remembering which dot started it.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// Take hold, at a pointer position in window pixels.
    ///
    /// The position is what the transform drags measure their delta from. A crop
    /// or mask handle is already at a known place in the layer, so for those the
    /// press carries nothing and the first move positions it.
    Grab(Gesture, [f32; 2]),
    /// Carry whatever is held to a pointer position. Inert with no drag in
    /// flight, which is the state the panel is in between gestures.
    Carry([f32; 2]),
    /// Let go: the draft becomes one edit, or nothing when the press never
    /// moved, because that press was a click on the picture.
    Release,
}

/// Run one step of a drag on the picture.
pub fn apply(state: &mut AppState, step: Step) -> Result<(), EngineError> {
    match step {
        Step::Grab(Gesture::Move, at) => state.begin_transform_drag(None, at),
        Step::Grab(Gesture::Scale(handle), at) => state.begin_transform_drag(Some(handle), at),
        Step::Grab(Gesture::Crop(handle), _) => state.begin_crop_drag(handle),
        Step::Grab(Gesture::Mask(handle), _) => state.begin_mask_drag(handle),
        Step::Grab(Gesture::TypeHandle(index), _) => state.begin_type_handle_drag(index),
        Step::Grab(Gesture::PathKey(key), at) => state.begin_path_key_drag(key, at),
        Step::Grab(Gesture::PathTangent(key, side), _) => state.begin_path_tangent_drag(key, side),
        Step::Grab(Gesture::PathCorner(key), _) => return state.toggle_path_key_smooth(key),
        Step::Grab(Gesture::PathAdd, at) => return state.add_key_on_path(at).map(|_| ()),
        Step::Grab(Gesture::MaskVertex(index), at) => return state.press_mask_vertex(index, at),
        Step::Grab(Gesture::MaskTangent(index, side), at) => {
            state.press_mask_tangent(index, side, at)
        }
        Step::Grab(Gesture::MaskVertexDelete(index), _) => return state.delete_mask_vertex(index),
        Step::Grab(Gesture::MaskVertexConvert(index), at) => state.press_mask_convert(index, at),
        Step::Grab(Gesture::MaskPathPen, at) => return state.press_mask_path(at),
        Step::Carry(at) => state.drag_overlay_to(at),
        Step::Release => return state.end_overlay_drag(),
    }
    Ok(())
}

/// Run one step of a drag on the picture, as the panel's presses and the
/// shell's carry and release do. A press makes the Viewer the active panel,
/// and a refusal is reported on the status line.
pub fn run(state: &mut AppState, step: Step) {
    if matches!(step, Step::Grab(..)) {
        state.active_panel = ActivePanel::Preview;
    }
    if let Err(err) = apply(state, step) {
        state.report_error(&err);
    }
}
