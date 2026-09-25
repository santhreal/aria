//! `Clip` canvas geometry: the constant transform properties, the animated sample at
//! a time, and the crop rectangle.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items, so the move changed no name resolution.

use super::*;

pub(super) fn position(handle: &Clip, py: Python) -> PyResult<(f32, f32)> {
    handle.with_project_ref(py, |project| {
        let t = Clip::current_transform(project, handle.id)?;
        Ok((t.position[0], t.position[1]))
    })
}

pub(super) fn set_position(handle: &Clip, py: Python, value: (f32, f32)) -> PyResult<()> {
    handle.with_project(py, |project| {
        let mut t = Clip::current_transform(project, handle.id)?;
        t.position = [value.0, value.1];
        Clip::set_transform_constant(project, handle.id, t)
    })
}

pub(super) fn anchor(handle: &Clip, py: Python) -> PyResult<(f32, f32)> {
    handle.with_project_ref(py, |project| {
        let t = Clip::current_transform(project, handle.id)?;
        Ok((t.anchor_point[0], t.anchor_point[1]))
    })
}

pub(super) fn set_anchor(handle: &Clip, py: Python, value: (f32, f32)) -> PyResult<()> {
    handle.with_project(py, |project| {
        let mut t = Clip::current_transform(project, handle.id)?;
        t.anchor_point = [value.0, value.1];
        Clip::set_transform_constant(project, handle.id, t)
    })
}

pub(super) fn scale(handle: &Clip, py: Python) -> PyResult<f32> {
    handle.with_project_ref(py, |project| {
        // Uniform X (Python API stays a single float until per-axis lands).
        Ok(Clip::current_transform(project, handle.id)?.scale.x)
    })
}

pub(super) fn set_scale(handle: &Clip, py: Python, value: f32) -> PyResult<()> {
    handle.with_project(py, |project| {
        let mut t = Clip::current_transform(project, handle.id)?;
        t.scale = aria_models::Scale2::uniform(value);
        Clip::set_transform_constant(project, handle.id, t)
    })
}

pub(super) fn rotation(handle: &Clip, py: Python) -> PyResult<f32> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::current_transform(project, handle.id)?.rotation)
    })
}

pub(super) fn set_rotation(handle: &Clip, py: Python, value: f32) -> PyResult<()> {
    handle.with_project(py, |project| {
        let mut t = Clip::current_transform(project, handle.id)?;
        t.rotation = value;
        Clip::set_transform_constant(project, handle.id, t)
    })
}

pub(super) fn opacity(handle: &Clip, py: Python) -> PyResult<f32> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::current_transform(project, handle.id)?.opacity)
    })
}

pub(super) fn set_opacity(handle: &Clip, py: Python, value: f32) -> PyResult<()> {
    handle.with_project(py, |project| {
        let mut t = Clip::current_transform(project, handle.id)?;
        t.opacity = value;
        Clip::set_transform_constant(project, handle.id, t)
    })
}

pub(super) fn transform_at(
    handle: &Clip,
    py: Python,
    t: f64,
)-> PyResult<((f32, f32), f32, f32, f32)> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        let tick = ticks(t, project.rate());
        let xf = clip.transform.sample(tick);
        Ok((
            (xf.position[0], xf.position[1]),
            xf.scale.x,
            xf.rotation,
            xf.opacity,
        ))
    })
}

    #[allow(clippy::too_many_arguments)]
pub(super) fn crop(
    handle: &Clip,
    py: Python,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    flip_h: bool,
    flip_v: bool,
) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .set_clip_crop(handle.id, CropRect { x, y, w, h }, flip_h, flip_v, None)
            .map_err(model_err)
    })
}
