//! Content descriptors: text, solids, and shapes.

use aria_models::{
    Generator, Param, Shape, ShapeStroke as ModelShapeStroke, TextAlignH, TextAlignV,
    TextBackground as ModelTextBackground, TextCase, TextShadow as ModelTextShadow,
    TextStroke as ModelTextStroke, TextStyle,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::convert::{parse_color, parse_color_opt};

#[path = "content_shape.rs"]
mod content_shape;
#[path = "content_text.rs"]
mod content_text;

// The descriptors keep their `crate::content::…` paths: the split is internal.
pub use content_shape::{Arrow, Ellipse, Heart, Line, Polygon, Rect, ShapeStrokeSpec, Star};
pub(crate) use content_text::apply_text_style;
pub use content_text::{Text, TextBackground, TextShadow, TextStroke};

/// A solid fill descriptor.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Solid {
    pub(crate) rgba: [u8; 4],
}

#[pymethods]
impl Solid {
    #[new]
    fn new(color: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            rgba: parse_color(color)?,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Solid(r={}, g={}, b={}, a={})",
            self.rgba[0], self.rgba[1], self.rgba[2], self.rgba[3]
        )
    }
}

/// A bundled sticker descriptor: `asset` is a catalog id (e.g. `"heart"`,
/// `"star_spin"`). Unknown ids error here rather than placing an invisible
/// clip.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Sticker {
    pub(crate) asset: String,
}

#[pymethods]
impl Sticker {
    #[new]
    fn new(asset: &str) -> PyResult<Self> {
        if aria_models::sticker_spec(asset).is_none() {
            let ids: Vec<&str> = aria_models::sticker_catalog()
                .iter()
                .map(|s| s.id.as_str())
                .collect();
            return Err(PyValueError::new_err(format!(
                "unknown sticker '{asset}' (available: {})",
                ids.join(", ")
            )));
        }
        Ok(Self {
            asset: asset.to_owned(),
        })
    }

    fn __repr__(&self) -> String {
        format!("Sticker({:?})", self.asset)
    }
}

pub(crate) fn generator_from(obj: &Bound<'_, PyAny>) -> PyResult<Generator> {
    if let Ok(s) = obj.extract::<PyRef<Solid>>() {
        return Ok(Generator::SolidColor { rgba: s.rgba });
    }
    if let Ok(t) = obj.extract::<PyRef<Text>>() {
        return Ok(Generator::Text {
            content: t.content.clone(),
            style: t.style.clone(),
        });
    }
    if let Ok(s) = obj.extract::<PyRef<Sticker>>() {
        return Ok(Generator::Sticker {
            asset: s.asset.clone(),
        });
    }
    if let Ok(r) = obj.extract::<PyRef<Rect>>() {
        return Ok(r.generator());
    }
    if let Ok(e) = obj.extract::<PyRef<Ellipse>>() {
        return Ok(e.generator());
    }
    if let Ok(p) = obj.extract::<PyRef<Polygon>>() {
        return Ok(p.generator());
    }
    if let Ok(s) = obj.extract::<PyRef<Star>>() {
        return Ok(s.generator());
    }
    if let Ok(l) = obj.extract::<PyRef<Line>>() {
        return Ok(l.generator());
    }
    if let Ok(a) = obj.extract::<PyRef<Arrow>>() {
        return Ok(a.generator());
    }
    if let Ok(h) = obj.extract::<PyRef<Heart>>() {
        return Ok(h.generator());
    }
    Err(PyValueError::new_err(
        "content must be Media, MediaSlice, Text, Solid, Sticker, or a shape descriptor",
    ))
}

fn extract_optional<T: pyo3::PyClass + Clone>(value: &Bound<'_, PyAny>) -> PyResult<Option<T>> {
    if value.is_none() {
        Ok(None)
    } else {
        let r: PyRef<'_, T> = value.extract().map_err(PyErr::from)?;
        Ok(Some((*r).clone()))
    }
}
