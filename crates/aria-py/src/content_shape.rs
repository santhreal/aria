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

    fn __repr__(&self) -> String {
        format!("ShapeStroke(width={})", self.width)
    }
}

fn stroke_from(spec: Option<ShapeStrokeSpec>) -> Option<ModelShapeStroke> {
    spec.map(|s| ModelShapeStroke::new(s.rgba, s.width))
}

/// `generator` when the model would place it, else the model's reason as a
/// `ValueError`. Every shape constructor calls it with the generator it hands
/// a track, so a size, count, ratio or stroke the model rejects fails where
/// the script wrote it rather than at `Track.add`.
fn sound(generator: Generator) -> PyResult<()> {
    generator
        .validate()
        .map_err(|err| PyValueError::new_err(err.to_string()))
}

macro_rules! shape_class {
    ($name:ident, $shape:expr) => {
        #[pyclass(skip_from_py_object)]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) width: f32,
            pub(crate) height: f32,
            pub(crate) rgba: [u8; 4],
            pub(crate) corner_radius: f32,
            pub(crate) stroke: Option<ShapeStrokeSpec>,
        }

        impl $name {
            /// The generator a track places for this descriptor.
            pub(crate) fn generator(&self) -> Generator {
                shape_generator(
                    $shape,
                    self.rgba,
                    self.width,
                    self.height,
                    self.corner_radius,
                    self.stroke.clone(),
                )
            }
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
                let made = Self {
                    width,
                    height,
                    rgba: parse_color_opt(color)?,
                    corner_radius,
                    stroke,
                };
                sound(made.generator())?;
                Ok(made)
            }

            fn __repr__(&self) -> String {
                format!(
                    "{}(width={}, height={})",
                    stringify!($name),
                    self.width,
                    self.height
                )
            }
        }
    };
}

shape_class!(Rect, Shape::Rectangle);
shape_class!(Ellipse, Shape::Ellipse);
shape_class!(Arrow, Shape::Arrow);
shape_class!(Heart, Shape::Heart);

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

impl Polygon {
    /// The generator a track places for this descriptor.
    pub(crate) fn generator(&self) -> Generator {
        shape_generator(
            Shape::Polygon { sides: self.sides },
            self.rgba,
            self.width,
            self.height,
            self.corner_radius,
            self.stroke.clone(),
        )
    }
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
        let made = Self {
            sides,
            width,
            height,
            rgba: parse_color_opt(color)?,
            corner_radius,
            stroke,
        };
        sound(made.generator())?;
        Ok(made)
    }

    fn __repr__(&self) -> String {
        format!(
            "Polygon({}, width={}, height={})",
            self.sides, self.width, self.height
        )
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

impl Star {
    /// The generator a track places for this descriptor.
    pub(crate) fn generator(&self) -> Generator {
        Generator::Shape {
            shape: Shape::Star {
                points: self.points,
                inner_ratio: Param::Constant(self.inner_ratio),
            },
            rgba: Param::Constant(self.rgba),
            width: Param::Constant(self.width),
            height: Param::Constant(self.height),
            corner_radius: Param::Constant(0.0),
            stroke: stroke_from(self.stroke.clone()),
        }
    }
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
        let made = Self {
            points,
            inner_ratio,
            width,
            height,
            rgba: parse_color_opt(color)?,
            stroke,
        };
        sound(made.generator())?;
        Ok(made)
    }

    fn __repr__(&self) -> String {
        format!(
            "Star({}, inner_ratio={}, width={}, height={})",
            self.points, self.inner_ratio, self.width, self.height
        )
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

impl Line {
    /// The generator a track places for this descriptor.
    pub(crate) fn generator(&self) -> Generator {
        shape_generator(
            Shape::Line,
            self.rgba,
            self.length,
            self.thickness,
            0.0,
            self.stroke.clone(),
        )
    }
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
        let made = Self {
            length,
            thickness,
            rgba: parse_color_opt(color)?,
            stroke,
        };
        sound(made.generator())?;
        Ok(made)
    }

    fn __repr__(&self) -> String {
        format!("Line(length={}, thickness={})", self.length, self.thickness)
    }
}

fn shape_generator(
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
