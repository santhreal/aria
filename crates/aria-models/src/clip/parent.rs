//! Clip parenting: a child's transform read in its parent's frame.
//!
//! A parented clip's position is an offset from its parent's anchor, turned by
//! the parent's rotation and scaled by the parent's scale; its rotation adds to
//! the parent's, and its scale multiplies the parent's per axis. Anchor point
//! and opacity stay the child's own. With the parent at identity the child is
//! placed exactly as it would be with no parent.
//!
//! The composition is per axis: a child of a parent that is both turned and
//! scaled unevenly keeps a rectangular shape rather than shearing.

use super::transform::{AnimatedTransform, ClipTransform, Scale2};
use crate::error::ModelError;
use crate::ids::ClipId;
use crate::param::Param;

/// The longest chain of parents a clip is placed through. A deeper chain is
/// refused when it is made, and a file that carries one is placed through its
/// first this many parents.
pub const MAX_PARENT_DEPTH: usize = 64;

/// The smallest parent scale a child can be placed back out of. A parent
/// flattened below it has no inverse, so a child cannot keep its place when
/// it is parented to it or released from it.
const MIN_PARENT_SCALE: f32 = 1e-6;

/// The frame a parent lends its children at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParentFrame {
    /// The parent's anchor in canvas pixels from the top-left corner.
    pub pivot: [f32; 2],
    /// The parent's clockwise rotation in degrees.
    pub rotation: f32,
    /// The parent's per-axis scale.
    pub scale: Scale2,
    /// The canvas size in pixels the frame is measured on.
    pub canvas: [f32; 2],
}

impl ParentFrame {
    /// The frame of a parent placed at `world` on a canvas of `canvas` pixels.
    pub fn of(world: &ClipTransform, canvas: [f32; 2]) -> Self {
        Self {
            pivot: [
                canvas[0] * (0.5 + world.position[0]),
                canvas[1] * (0.5 + world.position[1]),
            ],
            rotation: world.rotation,
            scale: world.scale,
            canvas,
        }
    }

    /// Whether a child can be placed back out of this frame.
    pub fn invertible(&self) -> bool {
        self.scale.x.abs() >= MIN_PARENT_SCALE && self.scale.y.abs() >= MIN_PARENT_SCALE
    }

    /// Scale then turn `v`, a vector in canvas fractions.
    fn turn(&self, v: [f32; 2]) -> [f32; 2] {
        let x = v[0] * self.canvas[0] * self.scale.x;
        let y = v[1] * self.canvas[1] * self.scale.y;
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        [
            (x * cos - y * sin) / self.canvas[0],
            (x * sin + y * cos) / self.canvas[1],
        ]
    }

    /// [`Self::turn`] undone. `None` when the frame is flattened.
    fn unturn(&self, v: [f32; 2]) -> Option<[f32; 2]> {
        if !self.invertible() {
            return None;
        }
        let x = v[0] * self.canvas[0];
        let y = v[1] * self.canvas[1];
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        Some([
            (x * cos + y * sin) / (self.canvas[0] * self.scale.x),
            (-x * sin + y * cos) / (self.canvas[1] * self.scale.y),
        ])
    }

    /// The canvas position, from the canvas center in canvas fractions, of a
    /// child at `local`.
    pub fn place(&self, local: [f32; 2]) -> [f32; 2] {
        let turned = self.turn(local);
        [
            self.pivot[0] / self.canvas[0] - 0.5 + turned[0],
            self.pivot[1] / self.canvas[1] - 0.5 + turned[1],
        ]
    }

    /// The local position of a child placed at canvas position `world`.
    pub fn unplace(&self, world: [f32; 2]) -> Option<[f32; 2]> {
        self.unturn([
            world[0] + 0.5 - self.pivot[0] / self.canvas[0],
            world[1] + 0.5 - self.pivot[1] / self.canvas[1],
        ])
    }

    /// The child's transform on the canvas.
    pub fn compose(&self, local: &ClipTransform) -> ClipTransform {
        ClipTransform {
            position: self.place(local.position),
            anchor_point: local.anchor_point,
            scale: Scale2 {
                x: self.scale.x * local.scale.x,
                y: self.scale.y * local.scale.y,
            },
            rotation: self.rotation + local.rotation,
            opacity: local.opacity,
        }
    }

    /// The child's transform in this frame, given where it is on the canvas.
    /// `None` when the frame is flattened.
    pub fn decompose(&self, world: &ClipTransform) -> Option<ClipTransform> {
        Some(ClipTransform {
            position: self.unplace(world.position)?,
            anchor_point: world.anchor_point,
            scale: Scale2 {
                x: world.scale.x / self.scale.x,
                y: world.scale.y / self.scale.y,
            },
            rotation: world.rotation - self.rotation,
            opacity: world.opacity,
        })
    }
}

/// How a clip takes a new parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentKeep {
    /// The child stays where it is on the canvas: its position, scale, and
    /// rotation are rewritten into the new frame.
    #[default]
    Place,
    /// The child snaps to the parent: its position, scale, and rotation are
    /// reset, keyframes included, so it sits on the parent's anchor at the
    /// parent's scale and rotation.
    Snap,
}

impl AnimatedTransform {
    /// Rewrite every position, scale, and rotation value, keyframes and
    /// spatial tangents included, from its pose in `from` to the same pose in
    /// `to`. `None` is the canvas itself.
    ///
    /// `parent` is the clip the flattened frame belongs to, for the error.
    pub fn reparent(
        &mut self,
        from: Option<&ParentFrame>,
        to: Option<&ParentFrame>,
        parent: ClipId,
    ) -> Result<(), ModelError> {
        if to.is_some_and(|frame| !frame.invertible()) {
            return Err(ModelError::ParentFlattened(parent));
        }
        let place = |p: [f32; 2]| from.map_or(p, |frame| frame.place(p));
        let unplace = |p: [f32; 2]| to.map_or(Some(p), |frame| frame.unplace(p));
        let turn = |v: [f32; 2]| from.map_or(v, |frame| frame.turn(v));
        let unturn = |v: [f32; 2]| to.map_or(Some(v), |frame| frame.unturn(v));
        let flat = || ModelError::ParentFlattened(parent);
        match &mut self.position {
            Param::Constant(value) => *value = unplace(place(*value)).ok_or_else(flat)?,
            Param::Keyframed { keyframes } => {
                for keyframe in keyframes {
                    keyframe.value = unplace(place(keyframe.value)).ok_or_else(flat)?;
                    if let Some(tangents) = &mut keyframe.tangents {
                        tangents.out_t = unturn(turn(tangents.out_t)).ok_or_else(flat)?;
                        tangents.in_t = unturn(turn(tangents.in_t)).ok_or_else(flat)?;
                    }
                }
            }
        }
        let (from_scale, to_scale) = (
            from.map_or(Scale2::ONE, |frame| frame.scale),
            to.map_or(Scale2::ONE, |frame| frame.scale),
        );
        self.scale.map_values(|scale| Scale2 {
            x: scale.x * from_scale.x / to_scale.x,
            y: scale.y * from_scale.y / to_scale.y,
        });
        let turned =
            from.map_or(0.0, |frame| frame.rotation) - to.map_or(0.0, |frame| frame.rotation);
        self.rotation.map_values(|rotation| rotation + turned);
        Ok(())
    }

    /// Sit on the parent's anchor at the parent's scale and rotation: position
    /// `[0, 0]`, scale 1, rotation 0, every keyframe of the three dropped.
    pub fn snap_to_parent(&mut self) {
        self.position = Param::Constant([0.0, 0.0]);
        self.scale = Param::Constant(Scale2::ONE);
        self.rotation = Param::Constant(0.0);
    }
}

#[cfg(test)]
#[path = "parent_tests.rs"]
mod tests;
