//! Text descriptors: the caption layer and the three decorations it can carry,
//! plus the keyword surface `Clip.set_style` writes through.
//!
//! Declared from `content.rs` with `#[path]`; `use super::*` reaches that
//! module's imports and private items, so the move changed no name resolution.

use super::*;

/// A title / caption layer descriptor.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
pub struct Text {
    pub(crate) content: String,
    pub(crate) style: TextStyle,
}

#[pymethods]
impl Text {
    #[new]
    #[pyo3(signature = (
        content,
        font = "",
        size = 90.0,
        color = None,
        bold = false,
        italic = false,
        underline = false,
        case = "normal",
        letter_spacing = 0.0,
        kerning = true,
        line_spacing = 1.2,
        align = ("center".to_string(), "middle".to_string()),
        wrap = true,
        stroke = None,
        background = None,
        shadow = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        content: &str,
        font: &str,
        size: f32,
        color: Option<&Bound<'_, PyAny>>,
        bold: bool,
        italic: bool,
        underline: bool,
        case: &str,
        letter_spacing: f32,
        kerning: bool,
        line_spacing: f32,
        align: (String, String),
        wrap: bool,
        stroke: Option<TextStroke>,
        background: Option<TextBackground>,
        shadow: Option<TextShadow>,
    ) -> PyResult<Self> {
        let style = TextStyle {
            font: font.to_string(),
            size: size.into(),
            bold,
            italic,
            underline,
            case: parse_text_case(case)?,
            fill: parse_color_opt(color)?.into(),
            letter_spacing: letter_spacing.into(),
            kerning,
            line_spacing: line_spacing.into(),
            align_h: parse_align_h(&align.0)?,
            align_v: parse_align_v(&align.1)?,
            wrap,
            stroke: stroke.map(|s| s.inner),
            background: background.map(|b| b.inner),
            shadow: shadow.map(|s| s.inner),
            // Catalog presets are not exposed here: treatments are
            // always explicit stroke/background/shadow arguments.
            effect_preset: None,
            runs: Vec::new(),
        };
        // The model's own bounds decide what a size, spacing, or stroke width
        // may be, so a script learns at the constructor rather than at render.
        style
            .validate()
            .map_err(|err| PyValueError::new_err(err.to_string()))?;
        Ok(Self {
            content: content.to_string(),
            style,
        })
    }

    fn __repr__(&self) -> String {
        // A size a script animated has no one number to print, and printing the
        // first keyframe's would read as the whole clip's size.
        match self.style.size.constant() {
            Some(size) => format!("Text({:?}, size={size})", self.content),
            None => format!("Text({:?}, size=animated)", self.content),
        }
    }
}

fn parse_text_case(s: &str) -> PyResult<TextCase> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "normal" => TextCase::Normal,
        "upper" => TextCase::Upper,
        "lower" => TextCase::Lower,
        "title" => TextCase::Title,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown text case {other:?}"
            )));
        }
    })
}

fn parse_align_h(s: &str) -> PyResult<TextAlignH> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "left" => TextAlignH::Left,
        "center" => TextAlignH::Center,
        "right" => TextAlignH::Right,
        "justify" => TextAlignH::Justify,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown horizontal align {other:?}"
            )));
        }
    })
}

fn parse_align_v(s: &str) -> PyResult<TextAlignV> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "top" => TextAlignV::Top,
        "middle" => TextAlignV::Middle,
        "bottom" => TextAlignV::Bottom,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown vertical align {other:?}"
            )));
        }
    })
}

/// Outline stroke for a [`Text`] descriptor.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct TextStroke {
    pub(crate) inner: ModelTextStroke,
}

#[pymethods]
impl TextStroke {
    #[new]
    #[pyo3(signature = (color, width = 6.0))]
    fn new(color: &Bound<'_, PyAny>, width: f32) -> PyResult<Self> {
        Ok(Self {
            inner: ModelTextStroke {
                rgba: parse_color(color)?.into(),
                width: width.into(),
            },
        })
    }
}

/// Filled card behind a [`Text`] block.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct TextBackground {
    pub(crate) inner: ModelTextBackground,
}

#[pymethods]
impl TextBackground {
    #[new]
    #[pyo3(signature = (color, radius = 0.0))]
    fn new(color: &Bound<'_, PyAny>, radius: f32) -> PyResult<Self> {
        Ok(Self {
            inner: ModelTextBackground {
                rgba: parse_color(color)?.into(),
                radius: radius.into(),
            },
        })
    }
}

/// Drop shadow behind a [`Text`] block.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct TextShadow {
    pub(crate) inner: ModelTextShadow,
}

#[pymethods]
impl TextShadow {
    #[new]
    #[pyo3(signature = (color, blur = 0.15, distance = 5.0))]
    fn new(color: &Bound<'_, PyAny>, blur: f32, distance: f32) -> PyResult<Self> {
        Ok(Self {
            inner: ModelTextShadow {
                rgba: parse_color(color)?.into(),
                blur: blur.into(),
                distance: distance.into(),
            },
        })
    }
}

/// Apply a `dict` of style overrides, rejecting the whole call if the result is
/// not a style the model accepts. Applied to a copy first: a caller that passes
/// a NaN size and a valid colour gets neither, rather than half the change and
/// an error.
pub(crate) fn apply_text_style(style: &mut TextStyle, dict: &Bound<'_, PyDict>) -> PyResult<()> {
    let mut candidate = style.clone();
    apply_text_style_fields(&mut candidate, dict)?;
    candidate
        .validate()
        .map_err(|err| PyValueError::new_err(err.to_string()))?;
    *style = candidate;
    Ok(())
}

fn apply_text_style_fields(style: &mut TextStyle, dict: &Bound<'_, PyDict>) -> PyResult<()> {
    for (key, value) in dict.iter() {
        let key: String = key.extract()?;
        match key.as_str() {
            "font" => style.font = value.extract()?,
            "size" => style.size = value.extract::<f32>()?.into(),
            "color" => style.fill = parse_color(&value)?.into(),
            "bold" => style.bold = value.extract()?,
            "italic" => style.italic = value.extract()?,
            "underline" => style.underline = value.extract()?,
            "letter_spacing" => style.letter_spacing = value.extract::<f32>()?.into(),
            "kerning" => style.kerning = value.extract()?,
            "line_spacing" => style.line_spacing = value.extract::<f32>()?.into(),
            "wrap" => style.wrap = value.extract()?,
            "case" => style.case = parse_text_case(&value.extract::<String>()?)?,
            "align" => {
                let (h, v): (String, String) = value.extract()?;
                style.align_h = parse_align_h(&h)?;
                style.align_v = parse_align_v(&v)?;
            }
            "stroke" => style.stroke = extract_optional::<TextStroke>(&value)?.map(|s| s.inner),
            "background" => {
                style.background = extract_optional::<TextBackground>(&value)?.map(|b| b.inner);
            }
            "shadow" => style.shadow = extract_optional::<TextShadow>(&value)?.map(|s| s.inner),
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown text style parameter {other:?}"
                )));
            }
        }
    }
    Ok(())
}
