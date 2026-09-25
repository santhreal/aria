//! `Clip` sound and retiming: gain, the two fades, and the speed factor.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items, so the move changed no name resolution.

use super::*;

pub(super) fn volume(handle: &Clip, py: Python) -> PyResult<f32> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(clip.volume.sample(0))
    })
}

pub(super) fn set_volume(handle: &Clip, py: Python, value: f32) -> PyResult<()> {
    handle.with_project(py, |project| {
        let rate = project.rate();
        project
            .model_mut()
            .set_clip_audio(
                handle.id,
                Some(value),
                RationalTime::zero(rate),
                RationalTime::zero(rate),
            )
            .map_err(model_err)
    })
}

pub(super) fn fade_in(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(seconds(clip.fade_in, project.rate()))
    })
}

pub(super) fn set_fade_in(handle: &Clip, py: Python, value: f64) -> PyResult<()> {
    handle.with_project(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        let rate = project.rate();
        let fade_out = RationalTime::new(clip.fade_out, rate);
        project
            .model_mut()
            .set_clip_audio(handle.id, None, time_at(value, rate), fade_out)
            .map_err(model_err)
    })
}

pub(super) fn fade_out(handle: &Clip, py: Python) -> PyResult<f64> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(seconds(clip.fade_out, project.rate()))
    })
}

pub(super) fn set_fade_out(handle: &Clip, py: Python, value: f64) -> PyResult<()> {
    handle.with_project(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        let rate = project.rate();
        let fade_in = RationalTime::new(clip.fade_in, rate);
        project
            .model_mut()
            .set_clip_audio(handle.id, None, fade_in, time_at(value, rate))
            .map_err(model_err)
    })
}

pub(super) fn set_speed(handle: &Clip, py: Python, factor: f64, reverse: bool) -> PyResult<()> {
    handle.with_project(py, |project| {
        let speed = speed_from_f64(factor)?;
        project
            .model_mut()
            .set_clip_speed(handle.id, speed, reverse)
            .map_err(model_err)
    })
}
