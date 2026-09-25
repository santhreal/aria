//! `Clip` look: the effect stack, the outgoing transition, and the text/shape style
//! surface.
//!
//! Declared from `clip.rs` with `#[path]`. pyo3 allows one `#[pymethods]` block
//! per class, so that block stays in the parent as the Python surface and every
//! body lives here; `use super::*` reaches the parent's imports and private
//! items, so the move changed no name resolution.

use super::*;

pub(super) fn add_effect(
    handle: &Clip,
    py: Python,
    effect_id: &str,
    params: Option<&Bound<'_, PyDict>>,
) -> PyResult<Effect> {
    handle.with_project(py, |project| {
        let index = project
            .model_mut()
            .add_effect(handle.id, effect_id)
            .map_err(model_err)?;
        if let Some(dict) = params {
            let fx = Effect::new(handle.project.clone_ref(py), handle.id, index);
            for (key, value) in dict.iter() {
                let name: String = key.extract()?;
                let v: f32 = value.extract()?;
                fx.set_param(project, &name, v)?;
            }
        }
        Ok(Effect::new(handle.project.clone_ref(py), handle.id, index))
    })
}

pub(super) fn effects(handle: &Clip, py: Python) -> PyResult<Vec<Effect>> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok((0..clip.effects.len())
            .map(|i| Effect::new(handle.project.clone_ref(py), handle.id, i))
            .collect())
    })
}

pub(super) fn remove_effect(handle: &Clip, py: Python, effect: &Effect) -> PyResult<()> {
    effect.check_clip(handle)?;
    handle.with_project(py, |project| {
        project
            .model_mut()
            .remove_effect(handle.id, effect.index())
            .map_err(model_err)
    })
}

pub(super) fn transition(
    handle: &Clip,
    py: Python,
    transition_id: &str,
    duration: Option<f64>,
) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .add_transition(handle.id, transition_id)
            .map_err(model_err)?;
        if let Some(dur) = duration {
            let ticks = ticks(dur, project.rate()).max(1);
            project
                .model_mut()
                .set_transition_duration(handle.id, ticks)
                .map_err(model_err)?;
        }
        Ok(())
    })
}

pub(super) fn remove_transition(handle: &Clip, py: Python) -> PyResult<()> {
    handle.with_project(py, |project| {
        project
            .model_mut()
            .remove_transition(handle.id)
            .map_err(model_err)
    })
}

pub(super) fn text(handle: &Clip, py: Python) -> PyResult<String> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        match &clip.content {
            aria_models::ClipSource::Generated(Generator::Text { content, .. }) => {
                Ok(content.clone())
            }
            _ => Err(PyValueError::new_err("clip is not a text clip")),
        }
    })
}

pub(super) fn set_text(handle: &Clip, py: Python, content: String) -> PyResult<()> {
    handle.with_project(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        let aria_models::ClipSource::Generated(Generator::Text { style, .. }) =
            &clip.content
        else {
            return Err(PyValueError::new_err("clip is not a text clip"));
        };
        let generator = Generator::Text {
            content,
            style: style.clone(),
        };
        project
            .model_mut()
            .set_generator(handle.id, generator)
            .map_err(model_err)
    })
}

pub(super) fn set_style(
    handle: &Clip,
    py: Python,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<()> {
    let kwargs =
        kwargs.ok_or_else(|| PyValueError::new_err("set_style requires keyword arguments"))?;
    handle.with_project(py, |project| {
        enum Target {
            Text,
            Shape,
        }
        // Which style block a clip opens is the generator's own `has_text` /
        // `has_shape` column, not a list of variants kept here.
        let target = match &Clip::require(project, handle.id)?.content {
            aria_models::ClipSource::Media { .. }
            | aria_models::ClipSource::Multicam { .. }
            | aria_models::ClipSource::Sequence { .. } => None,
            aria_models::ClipSource::Generated(generator) => {
                let spec = generator.spec();
                if spec.has_text {
                    Some(Target::Text)
                } else if spec.has_shape {
                    Some(Target::Shape)
                } else {
                    None
                }
            }
        }
        .ok_or_else(|| PyValueError::new_err("set_style applies to text or shape clips"))?;
        match target {
            Target::Text => {
                let clip = Clip::require(project, handle.id)?;
                let aria_models::ClipSource::Generated(Generator::Text { content, style }) =
                    &clip.content
                else {
                    unreachable!("target checked above");
                };
                let text = content.clone();
                let mut new_style = style.clone();
                apply_text_style(&mut new_style, kwargs)?;
                project
                    .model_mut()
                    .set_generator(
                        handle.id,
                        Generator::Text {
                            content: text,
                            style: new_style,
                        },
                    )
                    .map_err(model_err)
            }
            Target::Shape => {
                for (key, value) in kwargs.iter() {
                    let key: String = key.extract()?;
                    let (param, val) = shape_style_param(&key, &value)?;
                    project
                        .model_mut()
                        .set_param_constant(handle.id, ClipParam::Shape { param }, val)
                        .map_err(model_err)?;
                }
                Ok(())
            }
        }
    })
}

pub(super) fn repr(handle: &Clip, py: Python) -> PyResult<String> {
    handle.with_project_ref(py, |project| {
        let clip = Clip::require(project, handle.id)?;
        Ok(format!(
            "Clip(start={:.3}s, duration={:.3}s, id={})",
            clip.timeline.start.seconds(),
            clip.timeline.duration.seconds(),
            handle.id.raw()
        ))
    })
}
