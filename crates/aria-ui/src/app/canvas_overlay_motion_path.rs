//! Motion paths in the Viewer.
//!
//! A point property with keyframes, Position or Anchor point, draws its path
//! over the picture: one dot per frame from its first keyframe to its last,
//! spaced by speed, the keyframes as squares, and the spatial tangents of a
//! smooth keyframe as handles. Dragging a square moves the keyframe's value,
//! dragging a handle bends the segments on both sides of the keyframe,
//! Alt+click turns a keyframe between corner and smooth, and Ctrl+click on the
//! path adds a keyframe at the frame under the pointer.
//!
//! The model keeps spatial tangents on Position alone, so an Anchor point path
//! is straight between its keyframes and draws no tangent handles.
//!
//! The path in the property's own units is part of the selection's projection
//! ([`crate::app::selected_detail`]), because sampling a curve once per frame
//! of its span is work a repaint must not repeat. Placing it on screen is an
//! affine map per dot and is done per paint, because the view's zoom and pan
//! and a parent's frame at the playhead move it without changing the project.
//!
//! Position is drawn on the canvas, through the parent's frame at the playhead
//! for a child clip. Anchor point is a point inside the layer, so its path is
//! drawn across the layer's quad, where the anchor itself sits.

use std::borrow::Cow;
use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Clip, ClipParam, Easing, Param, ParamValue, ParentFrame, RationalTime, SpatialTangents,
};

use super::{OverlayDrag, Rect};
use crate::app::AppState;
use crate::types::UiClipTransform;

/// The largest tangent component the model accepts, in the property's units.
const MAX_TANGENT: f32 = 4.0;

/// How near, in window pixels, a Ctrl+click lands to a path's dot to add a
/// keyframe there.
pub const PATH_HIT: f32 = 6.0;

/// The closest two drawn dots sit, in window pixels. A slow segment puts many
/// frames on one pixel, and a dot painted under another dot shows nothing.
const DOT_GAP: f32 = 2.0;

/// A property that draws a motion path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathParam {
    Position,
    AnchorPoint,
}

impl PathParam {
    /// Every property that draws a path, in drawing order: the anchor's path
    /// under the position's.
    pub const ALL: [Self; 2] = [Self::AnchorPoint, Self::Position];

    /// The clip property this path draws.
    pub fn param(self) -> ClipParam {
        match self {
            Self::Position => ClipParam::Position,
            Self::AnchorPoint => ClipParam::AnchorPoint,
        }
    }

    /// Whether this path bends through smooth keyframes: Position only, the
    /// one property the model keeps spatial tangents on.
    pub fn bends(self) -> bool {
        matches!(self, Self::Position)
    }

    /// The first part of the identity of every element drawn for this path.
    pub fn id(self) -> &'static str {
        match self {
            Self::Position => "position-path",
            Self::AnchorPoint => "anchor-point-path",
        }
    }
}

/// Which spatial tangent of a keyframe: the one shaping the segment arriving
/// at it, or the one shaping the segment leaving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TangentSide {
    In,
    Out,
}

impl TangentSide {
    pub const ALL: [Self; 2] = [Self::In, Self::Out];

    /// The last part of a tangent handle's identity.
    pub fn id(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }

    /// This side's offset in `tangents`.
    fn of(self, tangents: SpatialTangents) -> [f32; 2] {
        match self {
            Self::In => tangents.in_t,
            Self::Out => tangents.out_t,
        }
    }
}

/// A keyframe on a path: the property, and the keyframe's place in its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathKey {
    pub param: PathParam,
    pub index: usize,
}

/// One keyframe of a path, in the property's own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OwnKey {
    /// The keyframe's tick on the timeline, which is what a command names.
    pub tick: i64,
    /// The keyframe's tick in the property's own time.
    pub local: i64,
    pub value: [f32; 2],
    pub easing: Easing,
    pub tangents: Option<SpatialTangents>,
}

impl OwnKey {
    /// Whether the path bends through this keyframe. A corner has no tangents,
    /// or tangents of zero length.
    pub fn smooth(&self) -> bool {
        self.tangents
            .is_some_and(|tangents| tangents != SpatialTangents::ZERO)
    }
}

/// One property's path in its own units.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnPath {
    pub param: PathParam,
    /// The property's value at every frame from its first keyframe to its last.
    pub dots: Arc<[[f32; 2]]>,
    /// The tick of the first dot in the property's own time.
    pub first: i64,
    pub keys: Arc<[OwnKey]>,
}

impl OwnPath {
    /// `param`'s path on `clip`, `None` when it has no keyframes.
    fn of(clip: &Clip, param: PathParam) -> Option<Self> {
        Self::from_curve(clip, param, param.param().motion_path(clip)?)
    }

    fn from_curve(clip: &Clip, param: PathParam, curve: &Param<[f32; 2]>) -> Option<Self> {
        let keys = curve.keyframes();
        let (first, last) = (keys.first()?.tick, keys.last()?.tick);
        Some(Self {
            param,
            dots: (first..=last).map(|tick| curve.sample(tick)).collect(),
            first,
            keys: keys
                .iter()
                .map(|kf| OwnKey {
                    tick: clip.param_timeline_tick(kf.tick),
                    local: kf.tick,
                    value: kf.value,
                    easing: kf.easing,
                    tangents: kf.tangents,
                })
                .collect(),
        })
    }

    /// The dot at `local`, a tick in the property's own time.
    fn dot(&self, local: i64) -> Option<[f32; 2]> {
        let offset = usize::try_from(local.checked_sub(self.first)?).ok()?;
        self.dots.get(offset).copied()
    }
}

/// Every path `clip` draws, in drawing order. Empty on an audio lane, where
/// the clip has no picture to move.
pub(crate) fn paths_of(clip: &Clip, audio_lane: bool) -> Arc<[OwnPath]> {
    if audio_lane {
        return Arc::new([]);
    }
    PathParam::ALL
        .into_iter()
        .filter_map(|param| OwnPath::of(clip, param))
        .collect()
}

/// A keyframe on a path, placed on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnKey {
    pub key: PathKey,
    /// Where the keyframe's square is centred, in window pixels.
    pub at: [f32; 2],
    /// Whether the keyframe sits under the playhead.
    pub current: bool,
    /// Where the incoming tangent handle sits, when the keyframe draws one.
    pub tangent_in: Option<[f32; 2]>,
    /// Where the outgoing tangent handle sits, when the keyframe draws one.
    pub tangent_out: Option<[f32; 2]>,
}

impl DrawnKey {
    /// Where the tangent handle on `side` sits: only a smooth keyframe draws
    /// one, and only on a side with a segment to shape.
    pub fn handle(&self, side: TangentSide) -> Option<[f32; 2]> {
        match side {
            TangentSide::In => self.tangent_in,
            TangentSide::Out => self.tangent_out,
        }
    }
}

/// A path placed on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnPath {
    pub param: PathParam,
    /// The dots, in window pixels, at least `DOT_GAP` apart.
    pub dots: Vec<[f32; 2]>,
    pub keys: Vec<DrawnKey>,
}

/// Where a path's own units land on screen.
#[derive(Debug, Clone, Copy)]
enum Space {
    /// Canvas fractions from the canvas centre, placed through the parent's
    /// frame at the playhead.
    Canvas {
        rect: Rect,
        frame: Option<ParentFrame>,
    },
    /// Fractions of the layer's own quad.
    Layer(Rect),
}

impl Space {
    fn to_window(self, own: [f32; 2]) -> [f32; 2] {
        match self {
            Self::Canvas { rect, frame } => {
                let [x, y] = frame.map_or(own, |frame| frame.place(own));
                rect.at([0.5 + x, 0.5 + y])
            }
            Self::Layer(rect) => rect.at(own),
        }
    }

    /// The own value drawn at window point `[x, y]`. Unclamped, because a
    /// point dragged off the picture is a normal place for a path to go.
    /// `fallback` when the rectangle has no area or the parent's frame
    /// flattens every point onto one.
    fn to_own(self, [x, y]: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
        let (Self::Canvas { rect, .. } | Self::Layer(rect)) = self;
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return fallback;
        }
        let fraction = [(x - rect.x) / rect.w, (y - rect.y) / rect.h];
        match self {
            Self::Canvas { frame, .. } => {
                let world = [fraction[0] - 0.5, fraction[1] - 0.5];
                match frame {
                    Some(frame) => frame.unplace(world).unwrap_or(fallback),
                    None => world,
                }
            }
            Self::Layer(_) => fraction,
        }
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(v: [f32; 2], by: f32) -> [f32; 2] {
    [v[0] * by, v[1] * by]
}

fn length(v: [f32; 2]) -> f32 {
    v[0].hypot(v[1])
}

fn clamped(v: [f32; 2]) -> [f32; 2] {
    v.map(|c| c.clamp(-MAX_TANGENT, MAX_TANGENT))
}

/// The tangents after the handle on `side` is dragged to `offset` from its
/// keyframe: that handle follows the pointer, and the other keeps its length
/// and turns to point the opposite way, so the keyframe stays smooth.
pub fn bend(start: SpatialTangents, side: TangentSide, offset: [f32; 2]) -> SpatialTangents {
    let held = clamped(offset);
    let other = match side {
        TangentSide::In => start.out_t,
        TangentSide::Out => start.in_t,
    };
    let reach = length(held);
    let opposite = if reach > f32::EPSILON {
        clamped(scale(held, -length(other) / reach))
    } else {
        other
    };
    match side {
        TangentSide::In => SpatialTangents {
            in_t: held,
            out_t: opposite,
        },
        TangentSide::Out => SpatialTangents {
            out_t: held,
            in_t: opposite,
        },
    }
}

/// Tangents that make keyframe `index` smooth: both handles along the line
/// from the previous keyframe to the next, each a third of the way to its own
/// neighbour. An end keyframe points its one handle at its neighbour. `None`
/// when the keyframe has no neighbour, or its neighbours give it no direction.
pub fn auto_tangents(keys: &[OwnKey], index: usize) -> Option<SpatialTangents> {
    let here = keys.get(index)?.value;
    let prev = index
        .checked_sub(1)
        .and_then(|before| keys.get(before))
        .map(|key| key.value);
    let next = keys.get(index + 1).map(|key| key.value);
    let (in_t, out_t) = match (prev, next) {
        (Some(prev), Some(next)) => {
            let along = sub(next, prev);
            let span = length(along);
            if span <= f32::EPSILON {
                return None;
            }
            let unit = scale(along, 1.0 / span);
            (
                scale(unit, -length(sub(here, prev)) / 3.0),
                scale(unit, length(sub(next, here)) / 3.0),
            )
        }
        (None, Some(next)) => ([0.0, 0.0], scale(sub(next, here), 1.0 / 3.0)),
        (Some(prev), None) => (scale(sub(prev, here), 1.0 / 3.0), [0.0, 0.0]),
        (None, None) => return None,
    };
    let tangents = SpatialTangents {
        in_t: clamped(in_t),
        out_t: clamped(out_t),
    };
    (tangents != SpatialTangents::ZERO).then_some(tangents)
}

/// Place `own` on screen through `space`, marking the keyframe at timeline
/// tick `playhead`.
fn drawn(own: &OwnPath, space: Space, playhead: i64) -> DrawnPath {
    let mut dots: Vec<[f32; 2]> = Vec::with_capacity(own.dots.len());
    for dot in own.dots.iter() {
        let at = space.to_window(*dot);
        if dots
            .last()
            .is_none_or(|last| length(sub(at, *last)) >= DOT_GAP)
        {
            dots.push(at);
        }
    }
    let last = own.keys.len().saturating_sub(1);
    let keys = own
        .keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let handle = |side: TangentSide, shapes: bool| {
                let offset = side.of(key.tangents?);
                (shapes && length(offset) > f32::EPSILON)
                    .then(|| space.to_window(add(key.value, offset)))
            };
            DrawnKey {
                key: PathKey {
                    param: own.param,
                    index,
                },
                at: space.to_window(key.value),
                current: key.tick == playhead,
                tangent_in: handle(TangentSide::In, index > 0),
                tangent_out: handle(TangentSide::Out, index < last),
            }
        })
        .collect();
    DrawnPath {
        param: own.param,
        dots,
        keys,
    }
}

impl AppState {
    /// Where `param`'s own units land on screen, `None` before the picture
    /// has been painted or, for the anchor, while the layer has no quad.
    fn path_space(&self, param: PathParam) -> Option<Space> {
        match param {
            PathParam::Position => Some(Space::Canvas {
                rect: self.overlay_frame.canvas?,
                frame: self.selected_parent_frame(),
            }),
            PathParam::AnchorPoint => self.overlay_frame.layer.map(Space::Layer),
        }
    }

    /// The selected clip's paths in its own units, from the projection.
    fn own_paths(&self) -> &[OwnPath] {
        match &self.selected {
            Some(detail) => &detail.paths,
            None => &[],
        }
    }

    /// `param`'s path on the selected clip.
    fn own_path(&self, param: PathParam) -> Option<&OwnPath> {
        self.own_paths().iter().find(|path| path.param == param)
    }

    /// The keyframe `key` names on the selected clip.
    fn own_key(&self, key: PathKey) -> Option<OwnKey> {
        self.own_path(key.param)?.keys.get(key.index).copied()
    }

    /// Whether the drag in flight holds a motion path keyframe or tangent.
    pub fn path_dragging(&self) -> bool {
        matches!(
            self.overlay_drag,
            Some(OverlayDrag::PathKey { .. } | OverlayDrag::PathTangent { .. })
        )
    }

    /// The keyframe a path drag holds, with the value and tangents it is
    /// building.
    fn path_draft(&self) -> Option<(PathKey, [f32; 2], Option<SpatialTangents>)> {
        match self.overlay_drag? {
            OverlayDrag::PathKey { key, draft, .. } => {
                Some((key, draft, self.own_key(key)?.tangents))
            }
            OverlayDrag::PathTangent { key, draft, .. } => {
                Some((key, self.own_key(key)?.value, Some(draft)))
            }
            _ => None,
        }
    }

    /// `param`'s curve on the selected clip with the drafted keyframe written
    /// into it, `None` while no path drag holds one of its keyframes.
    fn drafted_curve(&self, param: PathParam) -> Option<Param<[f32; 2]>> {
        let (key, value, tangents) = self.path_draft()?;
        if key.param != param {
            return None;
        }
        let own = self.own_key(key)?;
        let mut curve = param.param().motion_path(self.selected_model()?)?.clone();
        curve.set_keyframe_with_tangents(own.local, value, own.easing, tangents);
        Some(curve)
    }

    /// The selected clip's transform at the playhead with a path drag's draft
    /// sampled into it, `None` while no path drag is in flight or the
    /// playhead is off the clip.
    pub(super) fn path_drafted_transform(&self) -> Option<UiClipTransform> {
        let (key, ..) = self.path_draft()?;
        let curve = self.drafted_curve(key.param)?;
        let model = self.selected_model()?;
        let playhead = self.playhead.value;
        if playhead < model.timeline.start.value || playhead >= model.timeline.end_tick() {
            return None;
        }
        let at = curve.sample(model.param_tick(playhead));
        let mut drafted = self.selected_pose()?;
        match key.param {
            PathParam::Position => [drafted.position_x, drafted.position_y] = at,
            PathParam::AnchorPoint => [drafted.anchor_x, drafted.anchor_y] = at,
        }
        Some(drafted)
    }

    /// The selected clip's motion paths as the Viewer draws them, the one a
    /// drag holds as the drag has moved it. Empty with nothing selected, on an
    /// audio lane, before the first paint, or with no keyframed point.
    pub fn motion_paths(&self) -> Vec<DrawnPath> {
        let Some(model) = self.selected_model() else {
            return Vec::new();
        };
        self.own_paths()
            .iter()
            .filter_map(|own| {
                let space = self.path_space(own.param)?;
                let own = match self.drafted_curve(own.param) {
                    Some(curve) => Cow::Owned(OwnPath::from_curve(model, own.param, &curve)?),
                    None => Cow::Borrowed(own),
                };
                Some(drawn(&own, space, self.playhead.value))
            })
            .collect()
    }

    /// The frame on a drawn path nearest window point `at`, within
    /// [`PATH_HIT`], as the property and the tick in its own time.
    pub fn path_hit(&self, at: [f32; 2]) -> Option<(PathParam, i64)> {
        let mut best: Option<(f32, PathParam, i64)> = None;
        for path in self.own_paths() {
            let Some(space) = self.path_space(path.param) else {
                continue;
            };
            for (offset, dot) in (path.first..).zip(path.dots.iter()) {
                let distance = length(sub(space.to_window(*dot), at));
                if distance <= PATH_HIT && best.is_none_or(|(nearest, ..)| distance < nearest) {
                    best = Some((distance, path.param, offset));
                }
            }
        }
        best.map(|(_, param, local)| (param, local))
    }

    /// Take hold of a path keyframe's square, pressed at window point `at`.
    /// Inert when the keyframe is gone.
    pub fn begin_path_key_drag(&mut self, key: PathKey, at: [f32; 2]) {
        let Some(own) = self.own_key(key) else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::PathKey {
            key,
            start: own.value,
            grab: at,
            draft: own.value,
        });
    }

    /// Take hold of one tangent handle of a path keyframe. Inert on a corner,
    /// which draws none.
    pub fn begin_path_tangent_drag(&mut self, key: PathKey, side: TangentSide) {
        let Some(start) = self.own_key(key).and_then(|own| own.tangents) else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::PathTangent {
            key,
            side,
            start,
            draft: start,
        });
    }

    /// Carry a path drag to window point `to`. `false` when the drag in flight
    /// is not on a path.
    pub(super) fn drag_path_to(&mut self, to: [f32; 2]) -> bool {
        match self.overlay_drag {
            Some(OverlayDrag::PathKey {
                key, start, grab, ..
            }) => {
                if let Some(space) = self.path_space(key.param) {
                    let from = space.to_window(start);
                    let draft = space.to_own(add(from, sub(to, grab)), start);
                    self.overlay_drag = Some(OverlayDrag::PathKey {
                        key,
                        start,
                        grab,
                        draft,
                    });
                }
            }
            Some(OverlayDrag::PathTangent {
                key, side, start, ..
            }) => {
                if let (Some(space), Some(own)) = (self.path_space(key.param), self.own_key(key)) {
                    let held = add(own.value, side.of(start));
                    let offset = sub(space.to_own(to, held), own.value);
                    self.overlay_drag = Some(OverlayDrag::PathTangent {
                        key,
                        side,
                        start,
                        draft: bend(start, side, offset),
                    });
                }
            }
            _ => return false,
        }
        // The picture follows the draft, the way a transform drag's does: the
        // project sees one edit when the pointer comes up.
        if let (Some(clip), Some(drafted)) = (self.selected_clip(), self.path_drafted_transform()) {
            self.engine
                .lock()
                .set_transform_override(Some((clip, drafted.into())));
        }
        true
    }

    /// Commit a path drag as one keyframe edit. A press that never moved is a
    /// click and edits nothing.
    pub(super) fn commit_path_drag(&mut self, drag: OverlayDrag) -> Result<(), EngineError> {
        match drag {
            OverlayDrag::PathKey {
                key, start, draft, ..
            } if draft != start => {
                let tangents = self.own_key(key).and_then(|own| own.tangents);
                self.write_path_key(key, draft, tangents)
            }
            OverlayDrag::PathTangent {
                key, start, draft, ..
            } if draft != start => match self.own_key(key) {
                Some(own) => self.write_path_key(key, own.value, Some(draft)),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// Rewrite the keyframe `key` names with `value` and `tangents`, keeping
    /// its easing.
    fn write_path_key(
        &mut self,
        key: PathKey,
        value: [f32; 2],
        tangents: Option<SpatialTangents>,
    ) -> Result<(), EngineError> {
        let (Some(clip), Some(model), Some(own)) = (
            self.selected_clip(),
            self.selected_model(),
            self.own_key(key),
        ) else {
            return Ok(());
        };
        let at = RationalTime::new(own.tick, model.timeline.start.rate);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param: key.param.param(),
            at,
            value: ParamValue::Vec2(value),
            easing: own.easing,
            tangents,
        }))?;
        Ok(())
    }

    /// Alt+click on a path keyframe: a smooth keyframe becomes a corner, and a
    /// corner becomes smooth with handles along its neighbours. Inert on a
    /// keyframe with no neighbour to point at, and on a path that never bends.
    pub fn toggle_path_key_smooth(&mut self, key: PathKey) -> Result<(), EngineError> {
        if !key.param.bends() {
            return Ok(());
        }
        let Some(path) = self.own_path(key.param) else {
            return Ok(());
        };
        let keys = Arc::clone(&path.keys);
        let Some(own) = keys.get(key.index) else {
            return Ok(());
        };
        let tangents = if own.smooth() {
            None
        } else {
            match auto_tangents(&keys, key.index) {
                Some(tangents) => Some(tangents),
                None => return Ok(()),
            }
        };
        self.write_path_key(key, own.value, tangents)
    }

    /// Ctrl+click on a path: add a keyframe at the frame of the dot under
    /// window point `at`, holding the value the path has there, so the path
    /// still runs through that point. Beside a smooth keyframe the new one is
    /// smooth as well.
    ///
    /// `false` when no dot is under the pointer, so the press goes on to
    /// whatever lies beneath. A dot that is already a keyframe, or that the
    /// clip's extent no longer covers, takes the press and edits nothing.
    pub fn add_key_on_path(&mut self, at: [f32; 2]) -> Result<bool, EngineError> {
        let Some((param, local)) = self.path_hit(at) else {
            return Ok(false);
        };
        let (Some(clip), Some(model), Some(path)) = (
            self.selected_clip(),
            self.selected_model(),
            self.own_path(param),
        ) else {
            return Ok(false);
        };
        let tick = model.param_timeline_tick(local);
        let inside = tick >= model.timeline.start.value && tick < model.timeline.end_tick();
        let Some(value) = path.dot(local) else {
            return Ok(true);
        };
        if !inside || path.keys.iter().any(|key| key.local == local) {
            return Ok(true);
        }
        let after = path.keys.partition_point(|key| key.local < local);
        let before = after.checked_sub(1).and_then(|index| path.keys.get(index));
        let curved = param.bends()
            && (before.is_some_and(OwnKey::smooth)
                || path.keys.get(after).is_some_and(OwnKey::smooth));
        let easing = before.map_or(Easing::Linear, |key| key.easing);
        let tangents = if curved {
            let mut keys = path.keys.to_vec();
            keys.insert(
                after,
                OwnKey {
                    tick,
                    local,
                    value,
                    easing,
                    tangents: None,
                },
            );
            auto_tangents(&keys, after)
        } else {
            None
        };
        let at = RationalTime::new(tick, model.timeline.start.rate);
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param: param.param(),
            at,
            value: ParamValue::Vec2(value),
            easing,
            tangents,
        }))?;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "canvas_overlay_motion_path_tests.rs"]
mod tests;
