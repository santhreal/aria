//! Placing a parented clip: its own pose read in its parents' frame.
//!
//! A clip's own pose is its transform after its drivers and look animations.
//! While a parent covers the instant, the pose is placed in the parent's frame
//! ([`aria_models::ParentFrame`]), and the parent's own pose is read the same
//! way, so a chain composes outermost first. Every placement the renderer
//! makes reads [`world_pose`]: the frame itself, motion blur samples, and an
//! editor's Viewer handles.

use aria_core::{Rational, RationalTime, resample};
use aria_models::{
    AnimatedTransform, Clip, ClipId, ClipTransform, LookAnimations, ModelError, ParentFrame,
    ParentKeep, Project,
};

use crate::animation::apply_look_animations;
use crate::driver::DriverFrame;
use crate::resolve::canvas_size;

/// `clip`'s own pose at fractional timeline tick `tick`: its transform after
/// its drivers and look animations, before any parent places it.
pub(crate) fn own_pose(project: &Project, clip: &Clip, tick: f64, rate: Rational) -> ClipTransform {
    let timeline = project
        .timeline_with_clip(clip.id)
        .unwrap_or_else(|| project.timeline());
    let driven = DriverFrame::in_timeline(project, timeline).apply(clip, tick);
    let local = clip.animation_tick_f(tick);
    apply_look_animations(
        LookAnimations::of(&driven),
        clip.timeline.duration.value,
        driven.transform.sample_at(local),
        local.round() as i64,
        local,
        rate,
    )
}

/// The frame `clip` is placed in at fractional timeline tick `tick` on a
/// canvas of `canvas` pixels, each parent read by `own_pose`. `live`
/// replaces one clip's own pose: a gesture in progress on that clip moves its
/// children with it. `None` when no parent covers `tick`.
pub fn parent_frame(
    project: &Project,
    clip: &Clip,
    tick: f64,
    rate: Rational,
    canvas: [f32; 2],
    live: Option<(ClipId, ClipTransform)>,
) -> Option<ParentFrame> {
    project.parent_frame(clip, tick, canvas, |parent, tick| match live {
        Some((id, pose)) if id == parent.id => pose,
        _ => own_pose(project, parent, tick, rate),
    })
}

/// `clip`'s pose on the canvas at fractional timeline tick `tick`: `own`, its
/// own pose, placed in its parents' frame while one covers `tick`.
pub fn world_pose(
    project: &Project,
    clip: &Clip,
    own: ClipTransform,
    tick: f64,
    rate: Rational,
    canvas: [f32; 2],
    live: Option<(ClipId, ClipTransform)>,
) -> ClipTransform {
    match parent_frame(project, clip, tick, rate, canvas, live) {
        Some(frame) => frame.compose(&own),
        None => own,
    }
}

/// The frame `parent` lends a child at fractional timeline tick `tick`: its
/// own pose, placed in its own parents' frame.
pub fn frame_of(
    project: &Project,
    parent: &Clip,
    tick: f64,
    rate: Rational,
    canvas: [f32; 2],
) -> ParentFrame {
    let own = own_pose(project, parent, tick, rate);
    let world = world_pose(project, parent, own, tick, rate, canvas, None);
    ParentFrame::of(&world, canvas)
}

/// `clip`'s transform once it is parented to `parent`, or with `None`
/// released, as `keep` selects.
///
/// [`ParentKeep::Place`] keeps the clip where it is drawn at timeline time
/// `at`: every position, scale, and rotation value, keyframes included, is
/// rewritten from the frame it is placed in now to the new one. When the
/// frame that changes (the new parent's, or on release the old one's) does
/// not cover `at`, the kept instant is the nearest one it covers.
/// [`ParentKeep::Snap`] puts the clip on the new parent's anchor at its scale
/// and rotation; on release it keeps the place.
///
/// Fails when `parent` may not parent `clip`
/// ([`Project::check_parent`]) or is flattened to no size at the kept
/// instant.
pub fn reparented(
    project: &Project,
    clip_id: ClipId,
    parent: Option<ClipId>,
    at: RationalTime,
    keep: ParentKeep,
) -> Result<AnimatedTransform, ModelError> {
    let clip = project
        .clip(clip_id)
        .ok_or(ModelError::UnknownClip(clip_id))?;
    let mut transform = clip.transform.clone();
    let new_parent = match parent {
        Some(id) => {
            project.check_parent(clip_id, id)?;
            Some(project.clip(id).ok_or(ModelError::UnknownClip(id))?)
        }
        None => None,
    };
    if keep == ParentKeep::Snap && new_parent.is_some() {
        transform.snap_to_parent();
        return Ok(transform);
    }
    let timeline = project
        .timeline_with_clip(clip_id)
        .ok_or(ModelError::UnknownClip(clip_id))?;
    let rate = timeline.frame_rate;
    let old_parent = clip.parent.and_then(|id| timeline.clip(id));
    let mut tick = resample(at, rate).value;
    if let Some(over) = new_parent.or(old_parent) {
        let start = clip.timeline.start.value.max(over.timeline.start.value);
        let end = clip.timeline.end_tick().min(over.timeline.end_tick());
        if start < end {
            tick = tick.clamp(start, end - 1);
        }
    }
    let tick = tick as f64;
    let (width, height) = canvas_size(project);
    let canvas = [width as f32, height as f32];
    let from = parent_frame(project, clip, tick, rate, canvas, None);
    let to = new_parent.map(|parent| frame_of(project, parent, tick, rate, canvas));
    transform.reparent(from.as_ref(), to.as_ref(), parent.unwrap_or(clip_id))?;
    Ok(transform)
}

#[cfg(test)]
#[path = "parent_tests.rs"]
mod tests;
