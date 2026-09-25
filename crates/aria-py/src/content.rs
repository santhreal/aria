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
pub use content_text::{Text, TextBackground, TextShadow, TextStroke};
pub(crate) use content_text::apply_text_style;
use content_shape::{shape_generator, stroke_from};

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
        return Ok(shape_generator(
            Shape::Rectangle,
            r.rgba,
            r.width,
            r.height,
            r.corner_radius,
            r.stroke.clone(),
        ));
    }
    if let Ok(e) = obj.extract::<PyRef<Ellipse>>() {
        return Ok(shape_generator(
            Shape::Ellipse,
            e.rgba,
            e.width,
            e.height,
            e.corner_radius,
            e.stroke.clone(),
        ));
    }
    if let Ok(p) = obj.extract::<PyRef<Polygon>>() {
        return Ok(shape_generator(
            Shape::Polygon { sides: p.sides },
            p.rgba,
            p.width,
            p.height,
            p.corner_radius,
            p.stroke.clone(),
        ));
    }
    if let Ok(s) = obj.extract::<PyRef<Star>>() {
        return Ok(Generator::Shape {
            shape: Shape::Star {
                points: s.points,
                inner_ratio: Param::Constant(s.inner_ratio),
            },
            rgba: Param::Constant(s.rgba),
            width: Param::Constant(s.width),
            height: Param::Constant(s.height),
            corner_radius: Param::Constant(0.0),
            stroke: stroke_from(s.stroke.clone()),
        });
    }
    if let Ok(l) = obj.extract::<PyRef<Line>>() {
        return Ok(shape_generator(
            Shape::Line,
            l.rgba,
            l.length,
            l.thickness,
            0.0,
            l.stroke.clone(),
        ));
    }
    if let Ok(a) = obj.extract::<PyRef<Arrow>>() {
        return Ok(shape_generator(
            Shape::Arrow,
            a.rgba,
            a.width,
            a.height,
            a.corner_radius,
            a.stroke.clone(),
        ));
    }
    if let Ok(h) = obj.extract::<PyRef<Heart>>() {
        return Ok(shape_generator(
            Shape::Heart,
            h.rgba,
            h.width,
            h.height,
            h.corner_radius,
            h.stroke.clone(),
        ));
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
