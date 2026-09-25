//! Effect, transition, sticker, animation, speed-ramp, and marker-colour
//! catalog introspection.

use aria_models::{
    MarkerColor, animation_catalog, effect_catalog, speed_preset_catalog, sticker_catalog,
    transition_catalog,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::convert::param_spec_dict;

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(effects, m)?)?;
    m.add_function(wrap_pyfunction!(transitions, m)?)?;
    m.add_function(wrap_pyfunction!(stickers, m)?)?;
    m.add_function(wrap_pyfunction!(animations, m)?)?;
    m.add_function(wrap_pyfunction!(speed_presets, m)?)?;
    m.add_function(wrap_pyfunction!(marker_colors, m)?)?;
    Ok(())
}

#[pyfunction]
fn effects(py: Python) -> PyResult<Vec<Py<PyDict>>> {
    effect_catalog()
        .iter()
        .map(|spec| {
            let dict = PyDict::new(py);
            dict.set_item("id", spec.id.as_str())?;
            dict.set_item("label", spec.label.as_str())?;
            let params: Vec<Py<PyDict>> = spec
                .params
                .iter()
                .map(|p| param_spec_dict(py, &p.name, &p.label, p.default, p.min, p.max))
                .collect::<PyResult<_>>()?;
            dict.set_item("params", params)?;
            Ok(dict.into())
        })
        .collect()
}

#[pyfunction]
fn stickers(py: Python) -> PyResult<Vec<Py<PyDict>>> {
    sticker_catalog()
        .iter()
        .map(|spec| {
            let dict = PyDict::new(py);
            dict.set_item("id", spec.id.as_str())?;
            dict.set_item("label", spec.label.as_str())?;
            dict.set_item("width", spec.width)?;
            dict.set_item("height", spec.height)?;
            dict.set_item("animated", spec.animated)?;
            Ok(dict.into())
        })
        .collect()
}

#[pyfunction]
fn animations(py: Python) -> PyResult<Vec<Py<PyDict>>> {
    animation_catalog()
        .iter()
        .map(|spec| {
            let dict = PyDict::new(py);
            dict.set_item("id", spec.id.as_str())?;
            dict.set_item("label", spec.label.as_str())?;
            dict.set_item("slot", spec.slot.id())?;
            dict.set_item("text_only", spec.text_only)?;
            Ok(dict.into())
        })
        .collect()
}

#[pyfunction]
fn transitions(py: Python) -> PyResult<Vec<Py<PyDict>>> {
    transition_catalog()
        .iter()
        .map(|spec| {
            let dict = PyDict::new(py);
            dict.set_item("id", spec.id.as_str())?;
            dict.set_item("label", spec.label.as_str())?;
            Ok(dict.into())
        })
        .collect()
}

/// Every built-in speed ramp: `id`, `label`, and the ramp's own points as
/// `[(position, speed), …]` on the same 0…1 domain `clip.set_speed_curve`
/// takes, so a script can start from a preset and bend it.
#[pyfunction]
fn speed_presets(py: Python) -> PyResult<Vec<Py<PyDict>>> {
    speed_preset_catalog()
        .iter()
        .map(|spec| {
            let dict = PyDict::new(py);
            dict.set_item("id", spec.id.as_str())?;
            dict.set_item("label", spec.label.as_str())?;
            let points: Vec<(f64, f32)> = spec
                .points
                .iter()
                .map(|point| (point.at, point.value))
                .collect();
            dict.set_item("points", points)?;
            Ok(dict.into())
        })
        .collect()
}

/// The marker palette, in the order `project.add_marker` cycles it.
#[pyfunction]
fn marker_colors() -> Vec<&'static str> {
    MarkerColor::ALL.iter().map(|color| color.name()).collect()
}
