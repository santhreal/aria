//! `Clip` retiming ramps, media relink, template-slot marks, and linkage.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items.
//!
//! What these four have in common is that they are all *about* the clip rather
//! than its picture: how fast it plays, which file it plays, whether a template
//! may refill it, and which other clips move with it.

use aria_models::{Keyframe, LinkId, Param, SPEED_CURVE_SCALE, TimeRange};

use super::*;
use crate::slot::Slot;

/// A ramp point as a script writes it: `(position, speed)` or
/// `(position, speed, easing)`, position running `0.0`…`1.0` across the clip.
/// Normalized, so the ramp survives a trim or a base-speed change — the same
/// domain the model stores.
fn parse_curve_point(item: &Bound<'_, PyAny>) -> PyResult<Keyframe<f32>> {
    let tuple = item.cast::<PyTuple>().map_err(|_| {
        PyValueError::new_err("a speed ramp point is (position, speed[, easing])")
    })?;
    if tuple.len() != 2 && tuple.len() != 3 {
        return Err(PyValueError::new_err(
            "a speed ramp point is (position, speed[, easing])",
        ));
    }
    let at: f64 = tuple.get_item(0)?.extract()?;
    if !(0.0..=1.0).contains(&at) {
        return Err(PyValueError::new_err(format!(
            "ramp position {at} is outside 0.0…1.0"
        )));
    }
    let value: f32 = tuple.get_item(1)?.extract()?;
    let easing = match tuple.len() {
        3 => parse_easing(&tuple.get_item(2)?)?,
        _ => Easing::default(),
    };
    Ok(Keyframe {
        tick: (at * SPEED_CURVE_SCALE as f64).round() as i64,
        value,
        easing,
        tangents: None,
    })
}

/// The ramp a script asked for: `None` clears it, a string names a built-in
/// preset, anything iterable is a list of points.
fn parse_curve(curve: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Param<f32>>> {
    let Some(curve) = curve else {
        return Ok(None);
    };
    if curve.is_none() {
        return Ok(None);
    }
    if let Ok(name) = curve.extract::<String>() {
        return aria_models::speed_preset(&name).map(Some).ok_or_else(|| {
            let known: Vec<&str> = aria_models::speed_preset_catalog()
                .iter()
                .map(|spec| spec.id.as_str())
                .collect();
            PyValueError::new_err(format!(
                "unknown speed preset {name:?}; one of {}",
                known.join(", ")
            ))
        });
    }
    let mut keyframes = Vec::new();
    for item in curve.try_iter()? {
        keyframes.push(parse_curve_point(&item?)?);
    }
    if keyframes.len() < 2 {
        return Err(PyValueError::new_err(
            "a speed ramp needs at least two points",
        ));
    }
    keyframes.sort_by_key(|key| key.tick);
    if keyframes.windows(2).any(|pair| pair[0].tick == pair[1].tick) {
        return Err(PyValueError::new_err(
            "two ramp points share one position",
        ));
    }
    Ok(Some(Param::Keyframed { keyframes }))
}

pub(super) fn set_speed_curve(
    handle: &Clip,
    py: Python,
    curve: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let curve = parse_curve(curve)?;
    handle.with_project(py, |project| {
        project
            .model_mut()
            .set_clip_speed_curve(handle.id, curve)
            .map_err(model_err)
    })
}

pub(super) fn speed_curve(handle: &Clip, py: Python) -> PyResult<Option<Vec<(f64, f32)>>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(match &clip.speed_curve {
            Param::Constant(_) => None,
            Param::Keyframed { keyframes } => Some(
                keyframes
                    .iter()
                    .map(|key| (key.tick as f64 / SPEED_CURVE_SCALE as f64, key.value))
                    .collect(),
            ),
        })
    })
}

pub(super) fn speed_preset(handle: &Clip, py: Python) -> PyResult<Option<String>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(aria_models::speed_preset_id(&clip.speed_curve).map(|id| id.to_string()))
    })
}

/// Point the clip at a different file, keeping its placement, transform,
/// effects and ramp. `start` is the in-point into the new media; without it the
/// window starts at zero and runs as long as the clip's current source window
/// (a still keeps the clip's own length).
pub(super) fn set_media(
    handle: &Clip,
    py: Python,
    media: &Media,
    start: Option<f64>,
) -> PyResult<()> {
    if media.project().bind(py).as_ptr() != handle.project.bind(py).as_ptr() {
        return Err(AriaError::new_err("media belongs to a different project"));
    }
    handle.with_project(py, |project| {
        let source = Media::require(project, media.id())?;
        let rate = source.frame_rate;
        let media_duration = source.duration.value;
        let is_image = source.is_image;
        let clip = Clip::require(project, handle.id)?;
        let current = clip
            .source_range()
            .ok_or_else(|| AriaError::new_err("only a media clip can be relinked"))?;
        let start_tick = match start {
            Some(start) => ticks(start, rate),
            None => 0,
        };
        // The window keeps the clip's length so the edit is a relink, not a
        // retrim. A still has no extent to run out of; real media does, so the
        // window is clamped to what the file actually holds and refused when
        // the requested in-point is past its end.
        let wanted = current.duration.value.max(1);
        let duration = if is_image {
            wanted
        } else {
            let left = media_duration - start_tick;
            if left <= 0 {
                return Err(AriaError::new_err(format!(
                    "in-point {:.3}s is past the end of the media",
                    start.unwrap_or(0.0)
                )));
            }
            wanted.min(left)
        };
        let window = TimeRange::at_rate(start_tick, duration, rate);
        project
            .model_mut()
            .set_clip_media(handle.id, media.id(), window)
            .map_err(model_err)
    })
}

pub(super) fn replaceable(handle: &Clip, py: Python) -> PyResult<Option<Slot>> {
    handle.with_project_ref(py, |project| {
        let rate = project.rate();
        let clip = Clip::require(project, handle.id)?;
        let duration = seconds(clip.timeline.duration.value, rate);
        Ok(clip
            .replaceable
            .as_ref()
            .map(|marker| Slot::from_model(marker, Some(duration))))
    })
}

pub(super) fn set_replaceable(handle: &Clip, py: Python, slot: Option<Slot>) -> PyResult<()> {
    let marker = match &slot {
        Some(slot) => Some(slot.to_model()?),
        None => None,
    };
    handle.with_project(py, |project| {
        project
            .model_mut()
            .set_replaceable(handle.id, marker)
            .map_err(model_err)
    })
}

pub(super) fn text_editable(handle: &Clip, py: Python) -> PyResult<bool> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::require(project, handle.id)?.text_editable)
    })
}

pub(super) fn set_text_editable(handle: &Clip, py: Python, editable: bool) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .set_text_editable(handle.id, editable)
            .map_err(model_err)
    })
}

/// Put this clip and `others` in one link group, so a shell moves, trims and
/// deletes them together. Re-linking a clip that was in another group moves it,
/// which is what the timeline's own link command does.
pub(super) fn link_with(handle: &Clip, py: Python, others: &Bound<'_, PyAny>) -> PyResult<()> {
    let mut ids = vec![handle.id];
    for item in others.try_iter()? {
        let other: Py<Clip> = item?.extract()?;
        let other = other.bind(py).borrow();
        if other.project.bind(py).as_ptr() != handle.project.bind(py).as_ptr() {
            return Err(AriaError::new_err("clip belongs to a different project"));
        }
        ids.push(other.id());
    }
    handle.with_project(py, |project| {
        project.model_mut().link_clips(&ids).map_err(model_err)?;
        Ok(())
    })
}

/// Dissolve the group this clip is in. Raises when it is not linked, so a
/// script cannot believe it unlinked something it did not.
pub(super) fn unlink(handle: &Clip, py: Python) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .unlink_clips(&[handle.id])
            .map_err(model_err)
    })
}

/// The other clips in this clip's link group, empty when it is not linked.
pub(super) fn linked(handle: &Clip, py: Python) -> PyResult<Vec<Clip>> {
    let ids = handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(match clip.link {
            Some(link) => group_members(project, link, handle.id),
            None => Vec::new(),
        })
    })?;
    Ok(ids
        .into_iter()
        .map(|id| Clip::new(handle.project.clone_ref(py), id))
        .collect())
}

fn group_members(project: &Project, link: LinkId, exclude: ClipId) -> Vec<ClipId> {
    project
        .model()
        .linked_clips(link)
        .into_iter()
        .filter(|&id| id != exclude)
        .collect()
}
