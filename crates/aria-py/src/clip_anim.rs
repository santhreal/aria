//! `Clip` animation: the preset slots, keyframe writes from `animate(**kwargs)`, and
//! the two ways a property returns to a constant.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items, so the move changed no name resolution.

use super::*;

pub(super) fn animation_in(handle: &Clip, py: Python) -> PyResult<Option<String>> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::require(project, handle.id)?
            .animation_in
            .as_ref()
            .map(|a| a.id.clone()))
    })
}

pub(super) fn animation_out(handle: &Clip, py: Python) -> PyResult<Option<String>> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::require(project, handle.id)?
            .animation_out
            .as_ref()
            .map(|a| a.id.clone()))
    })
}

pub(super) fn animation_combo(handle: &Clip, py: Python) -> PyResult<Option<String>> {
    handle.with_project_ref(py, |project| {
        Ok(Clip::require(project, handle.id)?
            .animation_combo
            .as_ref()
            .map(|a| a.id.clone()))
    })
}

pub(super) fn set_animation(
    handle: &Clip,
    py: Python,
    slot: &str,
    animation_id: Option<&str>,
) -> PyResult<()> {
    let animation_slot = parse_animation_slot(slot)?;
    let animation = animation_id.map(AnimationRef::new);
    handle.with_project(py, |project| {
        project
            .model_mut()
            .set_clip_animation(handle.id, animation_slot, animation)
            .map_err(model_err)
    })
}

pub(super) fn animate(
    handle: &Clip,
    py: Python,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<()> {
    let dict = kwargs
        .ok_or_else(|| PyValueError::new_err("animate() requires at least one property"))?;
    let default_easing = match dict.get_item("easing")? {
        Some(v) => parse_easing(&v)?,
        None => Easing::Linear,
    };
    let at_override: Option<f64> = match dict.get_item("at")? {
        Some(v) => Some(
            v.extract()
                .map_err(|_| PyValueError::new_err("at= must be a number of seconds"))?,
        ),
        None => None,
    };
    handle.with_project(py, |project| {
        let clip_span = Clip::span_ticks(project, handle.id)?;
        let rate = project.rate();
        let mut applied = 0usize;
        for (key, value) in dict.iter() {
            let name: String = key.extract()?;
            if name == "at" || name == "easing" {
                continue;
            }
            apply_clip_animation(
                project,
                handle.id,
                clip_span,
                rate,
                &name,
                &value,
                at_override,
                default_easing,
            )?;
            applied += 1;
        }
        if applied == 0 {
            return Err(PyValueError::new_err(
                "animate() requires at least one property",
            ));
        }
        Ok(())
    })
}

pub(super) fn remove_keyframe(handle: &Clip, py: Python, name: &str, at: f64) -> PyResult<()> {
    handle.with_project(py, |project| {
        let (clip_start, clip_dur) = Clip::span_ticks(project, handle.id)?;
        let abs = clip_key_time(clip_start, clip_dur, at, project.rate());
        let param = clip_param(name)?;
        project
            .model_mut()
            .remove_param_keyframe(handle.id, param, abs)
            .map_err(model_err)
    })
}

pub(super) fn clear_animation(
    handle: &Clip,
    py: Python,
    names: &Bound<'_, PyTuple>,
) -> PyResult<()> {
    let list: Vec<String> = names.extract().map_err(|_| {
        PyValueError::new_err("clear_animation expects property names as strings")
    })?;
    if list.is_empty() {
        return Err(PyValueError::new_err(
            "clear_animation requires at least one property name",
        ));
    }
    handle.with_project(py, |project| {
        for name in list {
            let param = clip_param(&name)?;
            let value = sample_param_at_clip_start(project, handle.id, &param)?;
            project
                .model_mut()
                .set_param_constant(handle.id, param, value)
                .map_err(model_err)?;
        }
        Ok(())
    })
}
