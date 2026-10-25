//! Align and Distribute: line the selected clips' bounds up against the frame,
//! the selection, or the key clip, or spread them evenly.
//!
//! Bounds are each clip's transformed layer bounds at the playhead, measured by
//! the resolver and the renderer the way a composite places the layer: a title
//! aligns by its ink, a rotated clip by the box around its turned quad, and a
//! parented clip where its parents put it. A move is a write to the clip's
//! Position, through its parents' frame: a keyframe under the playhead when
//! Position is animated, a constant otherwise. One press is one undo step.
//!
//! A clip is left out when it draws nothing at the playhead, when its lane is
//! locked, and when a driver computes its Position, because a written value
//! would not move it.

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineError};
use aria_models::{
    ClipId, ClipParam, Easing, ParamValue, ParentFrame, RationalTime, SpatialTangents,
};

use crate::app::AppState;
use crate::app::canvas_overlay::moved_on_canvas;

/// A shift below this many canvas pixels is no move.
const STILL_PX: f32 = 0.01;

/// What the Align buttons line the selection up against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignTo {
    /// The box around every selected clip.
    #[default]
    Selection,
    /// The sequence frame.
    Frame,
    /// The first selected clip drawing at the playhead, which stays put.
    FirstClip,
}

impl AlignTo {
    /// Every choice, in the order the panel draws them.
    pub const ALL: [Self; 3] = [Self::Selection, Self::Frame, Self::FirstClip];

    /// The selector's label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Selection => "Selection",
            Self::Frame => "Frame",
            Self::FirstClip => "First clip",
        }
    }
}

/// The edge or centre line an Align button moves each clip's bounds onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignEdge {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

impl AlignEdge {
    /// Every edge, in the order the panel draws them.
    pub const ALL: [Self; 6] = [
        Self::Left,
        Self::CenterX,
        Self::Right,
        Self::Top,
        Self::CenterY,
        Self::Bottom,
    ];

    /// The button's label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::CenterX => "Center",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::CenterY => "Middle",
            Self::Bottom => "Bottom",
        }
    }

    /// The axis the edge lies across: `0` for x, `1` for y.
    const fn axis(self) -> usize {
        match self {
            Self::Left | Self::CenterX | Self::Right => 0,
            Self::Top | Self::CenterY | Self::Bottom => 1,
        }
    }

    /// Where this edge of `bounds` lies along its axis.
    fn line(self, bounds: Bounds) -> f32 {
        let axis = self.axis();
        match self {
            Self::Left | Self::Top => bounds.min[axis],
            Self::CenterX | Self::CenterY => bounds.center(axis),
            Self::Right | Self::Bottom => bounds.max[axis],
        }
    }
}

/// The axis a Distribute button spreads clip centres along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignAxis {
    Horizontal,
    Vertical,
}

impl AlignAxis {
    /// Both axes, in the order the panel draws them.
    pub const ALL: [Self; 2] = [Self::Horizontal, Self::Vertical];

    /// The button's label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Across",
            Self::Vertical => "Down",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Horizontal => 0,
            Self::Vertical => 1,
        }
    }
}

/// How many selected clips Distribute needs: two fixed ends and one between.
pub const DISTRIBUTE_MIN: usize = 3;

/// An axis-aligned box in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Bounds {
    /// The box around a `size` quad centred on `center` and turned `rotation`
    /// radians about that centre, as the compositor turns it.
    pub(crate) fn of_quad(center: [f32; 2], size: [f32; 2], rotation: f32) -> Self {
        let (sin, cos) = rotation.sin_cos();
        let half = [
            ((size[0] * cos).abs() + (size[1] * sin).abs()) * 0.5,
            ((size[0] * sin).abs() + (size[1] * cos).abs()) * 0.5,
        ];
        Self {
            min: [center[0] - half[0], center[1] - half[1]],
            max: [center[0] + half[0], center[1] + half[1]],
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    fn center(self, axis: usize) -> f32 {
        (self.min[axis] + self.max[axis]) * 0.5
    }
}

/// One selected clip drawing at the playhead: where it is and how a move is
/// written.
struct Placed {
    clip: ClipId,
    bounds: Bounds,
    /// Its own Position at the playhead, in canvas fractions from the centre.
    own: [f32; 2],
    frame: Option<ParentFrame>,
    /// Position is animated, so a move is a keyframe under the playhead.
    keyed: bool,
    /// The curve and motion-path handles of a keyframe already under the
    /// playhead, kept by the rewrite.
    easing: Easing,
    tangents: Option<SpatialTangents>,
}

/// What one press asks for.
#[derive(Clone, Copy, Debug)]
enum Move {
    Align(AlignEdge, AlignTo),
    Distribute(AlignAxis),
}

/// Why a press wrote nothing, for the status line.
#[derive(Debug, PartialEq, Eq)]
enum Inert {
    NoneDrawing,
    TooFewToDistribute,
    InPlace,
}

impl Inert {
    fn message(&self) -> &'static str {
        match self {
            Self::NoneDrawing => {
                "No selected clip draws at the playhead on an unlocked lane with an undriven Position"
            }
            Self::TooFewToDistribute => {
                "Distribute needs three selected clips drawing at the playhead"
            }
            Self::InPlace => "The selection is already aligned",
        }
    }
}

/// Every clip of `selection` that draws at `at`, in selection order, and the
/// canvas size in pixels.
fn placements(
    engine: &mut Engine,
    selection: &[ClipId],
    at: RationalTime,
) -> (Vec<Placed>, [f32; 2]) {
    let Ok(scene) = aria_render::resolve(engine.project(), at) else {
        return (Vec::new(), [0.0; 2]);
    };
    let canvas = [scene.width as f32, scene.height as f32];
    let mut out = Vec::with_capacity(selection.len());
    for &clip in selection {
        let Some(layer) = scene.layers.iter().find(|layer| layer.clip == Some(clip)) else {
            continue;
        };
        if !layer.source.spec().paints {
            continue;
        }
        let Some((center, size)) = engine.layer_quad(layer, canvas) else {
            continue;
        };
        let project = engine.project();
        let Some(model) = project.clip(clip) else {
            continue;
        };
        let timeline = project.timeline();
        let locked = timeline
            .track_of(clip)
            .and_then(|track| timeline.track(track))
            .is_some_and(|track| track.locked);
        if locked || model.driver(ClipParam::Position).is_some() {
            continue;
        }
        let local = model.param_tick(at.value);
        let position = &model.transform.position;
        let key = position.keyframes().iter().find(|key| key.tick == local);
        let frame = model.parent.and_then(|_| {
            aria_render::parent_frame(
                project,
                model,
                at.value as f64,
                timeline.frame_rate,
                canvas,
                None,
            )
        });
        out.push(Placed {
            clip,
            bounds: Bounds::of_quad(center, size, layer.rotation),
            own: position.sample(local),
            frame,
            keyed: position.is_animated(),
            easing: key.map_or(Easing::Linear, |key| key.easing),
            tangents: key.and_then(|key| key.tangents),
        });
    }
    (out, canvas)
}

/// The box Align lines clips up against. A lone clip aligns to the frame,
/// because the selection and the key clip are that clip.
fn target(to: AlignTo, placed: &[Placed], canvas: [f32; 2]) -> Bounds {
    let frame = Bounds {
        min: [0.0; 2],
        max: canvas,
    };
    match (to, placed) {
        (_, [] | [_]) | (AlignTo::Frame, _) => frame,
        (AlignTo::Selection, _) => placed
            .iter()
            .map(|placed| placed.bounds)
            .reduce(Bounds::union)
            .unwrap_or(frame),
        (AlignTo::FirstClip, [first, ..]) => first.bounds,
    }
}

/// The canvas-pixel shift that puts each clip's `edge` on the same edge of
/// `target`.
fn align_shifts(edge: AlignEdge, placed: &[Placed], target: Bounds) -> Vec<[f32; 2]> {
    let axis = edge.axis();
    let line = edge.line(target);
    placed
        .iter()
        .map(|placed| {
            let mut by = [0.0; 2];
            by[axis] = line - edge.line(placed.bounds);
            by
        })
        .collect()
}

/// The canvas-pixel shift that spaces the clips' centres evenly along `axis`
/// between the two outermost, which stay put. Clips with equal centres keep
/// their selection order.
fn distribute_shifts(axis: AlignAxis, placed: &[Placed]) -> Vec<[f32; 2]> {
    let axis = axis.index();
    let mut shifts = vec![[0.0; 2]; placed.len()];
    let mut order: Vec<usize> = (0..placed.len()).collect();
    order.sort_by(|&a, &b| {
        placed[a]
            .bounds
            .center(axis)
            .total_cmp(&placed[b].bounds.center(axis))
    });
    let (Some(&first), Some(&last)) = (order.first(), order.last()) else {
        return shifts;
    };
    if order.len() < DISTRIBUTE_MIN {
        return shifts;
    }
    let start = placed[first].bounds.center(axis);
    let step = (placed[last].bounds.center(axis) - start) / (order.len() - 1) as f32;
    for (rank, &index) in order.iter().enumerate() {
        shifts[index][axis] = start + step * rank as f32 - placed[index].bounds.center(axis);
    }
    shifts
}

/// The Position writes one press makes, or why it makes none.
fn plan(
    engine: &mut Engine,
    selection: &[ClipId],
    at: RationalTime,
    how: Move,
) -> Result<Vec<EditCommand>, Inert> {
    let (placed, canvas) = placements(engine, selection, at);
    if placed.is_empty() || canvas[0] <= 0.0 || canvas[1] <= 0.0 {
        return Err(Inert::NoneDrawing);
    }
    let shifts = match how {
        Move::Align(edge, to) => align_shifts(edge, &placed, target(to, &placed, canvas)),
        Move::Distribute(_) if placed.len() < DISTRIBUTE_MIN => {
            return Err(Inert::TooFewToDistribute);
        }
        Move::Distribute(axis) => distribute_shifts(axis, &placed),
    };
    let writes: Vec<EditCommand> = placed
        .iter()
        .zip(shifts)
        .filter(|(_, by)| by[0].abs() > STILL_PX || by[1].abs() > STILL_PX)
        .filter_map(|(placed, by)| {
            let moved = moved_on_canvas(
                placed.frame,
                placed.own,
                [by[0] / canvas[0], by[1] / canvas[1]],
            );
            // A flattened parent places every child on one point, so no
            // Position moves the clip and the write would change nothing.
            (moved != placed.own).then(|| write(placed, moved, at))
        })
        .collect();
    if writes.is_empty() {
        return Err(Inert::InPlace);
    }
    Ok(writes)
}

/// The Position write that moves `placed` to `to`.
fn write(placed: &Placed, to: [f32; 2], at: RationalTime) -> EditCommand {
    let value = ParamValue::Vec2(to);
    if placed.keyed {
        EditCommand::SetParamKeyframe {
            clip: placed.clip,
            param: ClipParam::Position,
            at,
            value,
            easing: placed.easing,
            tangents: placed.tangents,
        }
    } else {
        EditCommand::SetParamConstant {
            clip: placed.clip,
            param: ClipParam::Position,
            value,
        }
    }
}

impl AppState {
    /// What the Align buttons line the selection up against.
    pub fn align_to(&self) -> AlignTo {
        self.align_to
    }

    /// Choose what the Align buttons line the selection up against. A view
    /// setting: nothing in the project changes.
    pub fn set_align_to(&mut self, to: AlignTo) {
        self.align_to = to;
    }

    /// Move the selected clips so `edge` of each one's bounds lies on `edge` of
    /// the [`AlignTo`] target. One undo step; inert with a status line when
    /// nothing would move.
    pub fn align_selected(&mut self, edge: AlignEdge) -> Result<(), EngineError> {
        let how = Move::Align(edge, self.align_to);
        self.move_selected(how)
    }

    /// Space the selected clips' centres evenly along `axis` between the two
    /// outermost. Needs [`DISTRIBUTE_MIN`] clips drawing at the playhead.
    pub fn distribute_selected(&mut self, axis: AlignAxis) -> Result<(), EngineError> {
        self.move_selected(Move::Distribute(axis))
    }

    fn move_selected(&mut self, how: Move) -> Result<(), EngineError> {
        let at = self.playhead;
        let planned = {
            let mut engine = self.engine.lock();
            plan(&mut engine, &self.selection, at, how)
        };
        match planned {
            Ok(writes) => self.one_step(|state| {
                for command in writes {
                    state.apply_command(Command::Edit(command))?;
                }
                Ok(())
            }),
            Err(inert) => {
                self.status_message = Some(inert.message().into());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
#[path = "align_tests.rs"]
mod tests;
