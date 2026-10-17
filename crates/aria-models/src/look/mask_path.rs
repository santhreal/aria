//! A path mask's vertices over time.
//!
//! [`Mask::path`] holds one path or keyframes of whole paths. A keyframe holds
//! every vertex, so between two keyframes the shape changes by moving each
//! vertex and both of its handles along the segment's easing. Two keyframes
//! with different vertex counts have no vertex-to-vertex match, so the earlier
//! path holds until the later keyframe.
//!
//! Inserting or deleting a vertex changes every keyframe the same way
//! ([`Mask::edit_every_path`]), which keeps the counts in step. Moving a vertex
//! or a handle changes the path at one time only ([`Mask::set_path_at`]): the
//! keyframe at that time when the path is keyframed, the path itself when it is
//! not.

use std::borrow::Cow;

use super::Mask;
use crate::clip::ShapePathPoint;
use crate::param::{Easing, Keyframe, Lerp, Param};

impl Mask {
    /// The path at clip-relative `tick`. Before the first keyframe the first
    /// path holds, after the last the last. Borrowed wherever one stored path
    /// holds; built only between two keyframes.
    pub fn path_at(&self, tick: f64) -> Cow<'_, [ShapePathPoint]> {
        match segment(&self.path, tick) {
            (from, Some(to), t) => {
                Cow::Owned(from.iter().zip(to).map(|(a, b)| between(a, b, t)).collect())
            }
            (from, None, _) => Cow::Borrowed(from),
        }
    }

    /// Whether the mask has a path with vertices at any time.
    pub fn has_path(&self) -> bool {
        match &self.path {
            Param::Constant(points) => !points.is_empty(),
            Param::Keyframed { keyframes } => keyframes.iter().any(|key| !key.value.is_empty()),
        }
    }

    /// Write `points` as the path at clip-relative `tick`: the keyframe there
    /// when the path is keyframed, which keeps its easing, or the path itself
    /// when it is not.
    pub fn set_path_at(&mut self, tick: i64, points: Vec<ShapePathPoint>) {
        match &mut self.path {
            Param::Constant(path) => *path = points,
            Param::Keyframed { keyframes } => {
                match keyframes.binary_search_by_key(&tick, |key| key.tick) {
                    Ok(index) => keyframes[index].value = points,
                    Err(index) => {
                        keyframes.insert(index, Keyframe::new(tick, points, Easing::Linear));
                    }
                }
            }
        }
    }

    /// Keyframe the path at clip-relative `tick` with the path shown there. A
    /// path that is not keyframed becomes a curve of that one keyframe; a
    /// keyframe already at `tick` is kept.
    pub fn key_path(&mut self, tick: i64) {
        if self.path.keyframe_at(tick).is_some() {
            return;
        }
        let shown = self.path_at(tick as f64).into_owned();
        if self.path.is_animated() {
            self.set_path_at(tick, shown);
        } else {
            self.path = Param::Keyframed {
                keyframes: vec![Keyframe::new(tick, shown, Easing::Linear)],
            };
        }
    }

    /// Remove the path keyframe at clip-relative `tick`. Removing the last one
    /// leaves its path as the mask's path. `false` when no keyframe sits there.
    pub fn unkey_path(&mut self, tick: i64) -> bool {
        let Param::Keyframed { keyframes } = &mut self.path else {
            return false;
        };
        let Ok(index) = keyframes.binary_search_by_key(&tick, |key| key.tick) else {
            return false;
        };
        let removed = keyframes.remove(index);
        if keyframes.is_empty() {
            self.path = Param::Constant(removed.value);
        }
        true
    }

    /// Drop every path keyframe and keep the path shown at clip-relative
    /// `tick`.
    pub fn hold_path(&mut self, tick: i64) {
        if self.path.is_animated() {
            self.path = Param::Constant(self.path_at(tick as f64).into_owned());
        }
    }

    /// Set the easing of the segment leaving the path keyframe at clip-relative
    /// `tick`. `false` when no keyframe sits there or it already eases so.
    pub fn set_path_easing(&mut self, tick: i64, easing: Easing) -> bool {
        let Param::Keyframed { keyframes } = &mut self.path else {
            return false;
        };
        match keyframes.binary_search_by_key(&tick, |key| key.tick) {
            Ok(index) if keyframes[index].easing != easing => {
                keyframes[index].easing = easing;
                true
            }
            _ => false,
        }
    }

    /// Move the path keyframe at clip-relative `from` to `to`, with its path
    /// and its easing. A keyframe already at `to` gives way to it. `false`
    /// when no keyframe sits at `from`, or `to` is `from`.
    pub fn move_path_key(&mut self, from: i64, to: i64) -> bool {
        let Param::Keyframed { keyframes } = &mut self.path else {
            return false;
        };
        if from == to {
            return false;
        }
        let Ok(index) = keyframes.binary_search_by_key(&from, |key| key.tick) else {
            return false;
        };
        let mut moved = keyframes.remove(index);
        moved.tick = to;
        match keyframes.binary_search_by_key(&to, |key| key.tick) {
            Ok(at) => keyframes[at] = moved,
            Err(at) => keyframes.insert(at, moved),
        }
        true
    }

    /// Apply `edit` to the path and to every path keyframe: the change a vertex
    /// insert or delete makes, which every keyframe needs so the vertex counts
    /// stay in step.
    pub fn edit_every_path(&mut self, mut edit: impl FnMut(&mut Vec<ShapePathPoint>)) {
        match &mut self.path {
            Param::Constant(points) => edit(points),
            Param::Keyframed { keyframes } => {
                for key in keyframes {
                    edit(&mut key.value);
                }
            }
        }
    }
}

/// The two paths `tick` falls between and the eased progress from the first to
/// the second, or the one path that holds there with no second.
fn segment(
    path: &Param<Vec<ShapePathPoint>>,
    tick: f64,
) -> (&[ShapePathPoint], Option<&[ShapePathPoint]>, f32) {
    let keyframes = match path {
        Param::Constant(points) => return (points, None, 0.0),
        Param::Keyframed { keyframes } => keyframes,
    };
    let (Some(first), Some(last)) = (keyframes.first(), keyframes.last()) else {
        return (&[], None, 0.0);
    };
    if tick <= first.tick as f64 {
        return (&first.value, None, 0.0);
    }
    if tick >= last.tick as f64 {
        return (&last.value, None, 0.0);
    }
    // `first.tick < tick < last.tick`, so the segment is two keyframes inside
    // the curve.
    let next = keyframes.partition_point(|key| key.tick as f64 <= tick);
    let (from, to) = (&keyframes[next - 1], &keyframes[next]);
    if from.value.len() != to.value.len() {
        return (&from.value, None, 0.0);
    }
    let t = ((tick - from.tick as f64) / (to.tick - from.tick) as f64) as f32;
    (&from.value, Some(&to.value), from.easing.apply(t))
}

/// The vertex `t` of the way from `a` to `b`, its handles with it.
fn between(a: &ShapePathPoint, b: &ShapePathPoint, t: f32) -> ShapePathPoint {
    ShapePathPoint {
        anchor: Lerp::lerp(a.anchor, b.anchor, t),
        handle_in: Lerp::lerp(a.handle_in, b.handle_in, t),
        handle_out: Lerp::lerp(a.handle_out, b.handle_out, t),
    }
}

#[cfg(test)]
#[path = "mask_path_tests.rs"]
mod tests;
