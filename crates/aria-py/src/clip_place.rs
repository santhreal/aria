//! `Clip` placement and trimming: where a clip sits on its track, what slice of
//! its media it shows, and the edits that move or remove it.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items, so the move changed no name resolution.

use super::*;

pub(super) fn start(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.timeline.start.seconds())
    })
}

pub(super) fn end(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.end().map_err(model_err)?.seconds())
    })
}

pub(super) fn duration(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.timeline.duration.seconds())
    })
}

pub(super) fn track(handle: &Clip, py: Python) -> PyResult<Track> {
    handle.with_project_ref(py, |project| {
        let track_id = project
            .model()
            .timeline()
            .track_of(handle.id)
            .ok_or_else(|| AriaError::new_err("stale clip handle"))?;
        Ok(Track::new(handle.project.clone_ref(py), track_id))
    })
}

pub(super) fn media(handle: &Clip, py: Python) -> PyResult<Option<Media>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip
            .media()
            .map(|id| Media::new(handle.project.clone_ref(py), id)))
    })
}

pub(super) fn source_start(handle: &Clip, py: Python) -> PyResult<Option<f64>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.source_range().map(|r| r.start.seconds()))
    })
}

pub(super) fn source_duration(handle: &Clip, py: Python) -> PyResult<Option<f64>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.source_range().map(|r| r.duration.seconds()))
    })
}

pub(super) fn speed(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(speed_to_f64(clip.speed))
    })
}

pub(super) fn reversed(handle: &Clip, py: Python) -> PyResult<bool> {
    handle.with_project_ref(py, |project| Ok(Clip::require(project, handle.id)?.reversed))
}

pub(super) fn split(handle: &Clip, py: Python, at: f64) -> PyResult<Clip> {
    handle.with_project(py, |project| {
        let rate = project.rate();
        let new_id = project
            .model_mut()
            .split_clip(handle.id, time_at(at, rate))
            .map_err(model_err)?;
        Ok(Clip::new(handle.project.clone_ref(py), new_id))
    })
}

pub(super) fn trim(
    handle: &Clip,
    py: Python,
    start: Option<f64>,
    end: Option<f64>,
) -> PyResult<()> {
    handle.with_project(py, |project| {
        let rate = project.rate();
        let clip = Clip::require(project, handle.id)?;
        let start = start.unwrap_or_else(|| clip.timeline.start.seconds());
        let end = match end {
            Some(e) => e,
            None => clip.end().map_err(model_err)?.seconds(),
        };
        if end <= start {
            return Err(PyValueError::new_err("end must be greater than start"));
        }
        let timeline = span(start, end - start, rate);
        project
            .model_mut()
            .trim_clip(handle.id, timeline)
            .map_err(model_err)
    })
}

pub(super) fn move_(
    handle: &Clip,
    py: Python,
    start: f64,
    track: Option<&Track>,
) -> PyResult<()> {
    handle.with_project(py, |project| {
        let rate = project.rate();
        let to_track = match track {
            Some(t) => {
                if !t.project().is(handle.project.bind(py)) {
                    return Err(AriaError::new_err(
                        "track belongs to a different project",
                    ));
                }
                t.id()
            }
            None => project
                .model()
                .timeline()
                .track_of(handle.id)
                .ok_or_else(|| AriaError::new_err("stale clip handle"))?,
        };
        project
            .model_mut()
            .move_clip(handle.id, to_track, time_at(start, rate))
            .map_err(model_err)
    })
}

pub(super) fn delete(handle: &Clip, py: Python) -> PyResult<()> {
    handle.with_project(py, |project| {
        project.model_mut().timeline_mut().remove_clip(handle.id);
        Ok(())
    })
}

pub(super) fn ripple_delete(handle: &Clip, py: Python) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .ripple_delete(handle.id)
            .map_err(model_err)?;
        Ok(())
    })
}
