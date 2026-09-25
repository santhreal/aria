//! Shape descriptors: the stroke spec, the four macro-generated rectangular
//! shapes, and the three that carry their own fields.
//!
//! Declared from `content.rs` with `#[path]`; `use super::*` reaches that
//! module's imports and private items, so the move changed no name resolution.

use super::*;

/// Outline for a shape (Python name: `ShapeStroke`).
#[pyclass(name = "ShapeStroke", from_py_object)]
#[derive(Clone)]
pub struct ShapeStrokeSpec {
    pub(crate) rgba: [u8; 4],
    pub(crate) width: f32,
}

#[pymethods]
impl ShapeStrokeSpec {
    #[new]
    #[pyo3(signature = (color, width = 8.0))]
    fn new(color: &Bound<'_, PyAny>, width: f32) -> PyResult<Self> {
        Ok(Self {
            rgba: parse_color(color)?,
            width,
        })
    }
}

pub(super) fn stroke_from(spec: Option<ShapeStrokeSpec>) -> Option<ModelShapeStroke> {
    spec.map(|s| ModelShapeStroke::new(s.rgba, s.width))
}

macro_rules! shape_class {
    ($name:ident) => {
        #[pyclass(skip_from_py_object)]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) width: f32,
            pub(crate) height: f32,
            pub(crate) rgba: [u8; 4],
            pub(crate) corner_radius: f32,
            pub(crate) stroke: Option<ShapeStrokeSpec>,
        }

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (width = 200.0, height = 200.0, color = None, corner_radius = 0.0, stroke = None))]
            fn new(
                width: f32,
                height: f32,
                color: Option<&Bound<'_, PyAny>>,
                corner_radius: f32,
                stroke: Option<ShapeStrokeSpec>,
            ) -> PyResult<Self> {
                Ok(Self {
                    width,
                    height,
                    rgba: parse_color_opt(color)?,
                    corner_radius,
                    stroke,
                })
            }
        }
    };
}

shape_class!(Rect);
shape_class!(Ellipse);
shape_class!(Arrow);
shape_class!(Heart);

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Polygon {
    pub(crate) sides: u32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) rgba: [u8; 4],
    pub(crate) corner_radius: f32,
    pub(crate) stroke: Option<ShapeStrokeSpec>,
}

#[pymethods]
impl Polygon {
    #[new]
    #[pyo3(signature = (sides, width = 200.0, height = 200.0, color = None, corner_radius = 0.0, stroke = None))]
    fn new(
        sides: u32,
        width: f32,
        height: f32,
        color: Option<&Bound<'_, PyAny>>,
        corner_radius: f32,
        stroke: Option<ShapeStrokeSpec>,
    ) -> PyResult<Self> {
        Ok(Self {
            sides,
            width,
            height,
            rgba: parse_color_opt(color)?,
            corner_radius,
            stroke,
        })
    }
}

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Star {
    pub(crate) points: u32,
    pub(crate) inner_ratio: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) rgba: [u8; 4],
    pub(crate) stroke: Option<ShapeStrokeSpec>,
}

#[pymethods]
impl Star {
    #[new]
    #[pyo3(signature = (points, inner_ratio = 0.5, width = 200.0, height = 200.0, color = None, stroke = None))]
    fn new(
        points: u32,
        inner_ratio: f32,
        width: f32,
        height: f32,
        color: Option<&Bound<'_, PyAny>>,
        stroke: Option<ShapeStrokeSpec>,
    ) -> PyResult<Self> {
        Ok(Self {
            points,
            inner_ratio,
            width,
            height,
            rgba: parse_color_opt(color)?,
            stroke,
        })
    }
}

#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Line {
    pub(crate) length: f32,
    pub(crate) thickness: f32,
    pub(crate) rgba: [u8; 4],
    pub(crate) stroke: Option<ShapeStrokeSpec>,
}

#[pymethods]
impl Line {
    #[new]
    #[pyo3(signature = (length = 200.0, thickness = 8.0, color = None, stroke = None))]
    fn new(
        length: f32,
        thickness: f32,
        color: Option<&Bound<'_, PyAny>>,
        stroke: Option<ShapeStrokeSpec>,
    ) -> PyResult<Self> {
        Ok(Self {
            length,
            thickness,
            rgba: parse_color_opt(color)?,
            stroke,
        })
    }
}

pub(super) fn shape_generator(
    shape: Shape,
    rgba: [u8; 4],
    width: f32,
    height: f32,
    corner_radius: f32,
    stroke: Option<ShapeStrokeSpec>,
) -> Generator {
    Generator::Shape {
        shape,
        rgba: Param::Constant(rgba),
        width: Param::Constant(width),
        height: Param::Constant(height),
        corner_radius: Param::Constant(corner_radius),
        stroke: stroke_from(stroke),
    }
}
