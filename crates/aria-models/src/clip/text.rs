use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::param::Param;

/// Sanity ceilings for persisted text metrics in 1080-reference pixels.
/// These are deliberately broader than the desktop inspector ranges so API
/// clients can create oversized display type without exposing the renderer to
/// unbounded allocations or morphology work.
pub(crate) const MAX_TEXT_SIZE: f32 = 4096.0;
pub(crate) const MAX_TEXT_LETTER_SPACING: f32 = 4096.0;
pub(crate) const MAX_TEXT_LINE_SPACING: f32 = 100.0;
pub(crate) const MAX_TEXT_STROKE_WIDTH: f32 = 512.0;
pub(crate) const MAX_TEXT_SHADOW_DISTANCE: f32 = 4096.0;

#[path = "text_runs.rs"]
mod runs;
pub use runs::{MAX_TEXT_RUNS, TEXT_RUN_VERSION, TextStyleRun};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextCase {
    /// Render the text as authored.
    #[default]
    Normal,
    /// UPPERCASE.
    Upper,
    /// lowercase.
    Lower,
    /// Title Case (first letter of each word).
    Title,
}

impl TextCase {
    /// Every case, in the order a picker lists them.
    pub const ALL: [Self; 4] = [Self::Normal, Self::Upper, Self::Lower, Self::Title];

    /// The case's name as a picker shows it.
    pub fn label(self) -> &'static str {
        match self {
            TextCase::Normal => "As typed",
            TextCase::Upper => "Upper",
            TextCase::Lower => "Lower",
            TextCase::Title => "Title",
        }
    }

    /// Apply the casing transform to `s`.
    pub fn apply(self, s: &str) -> String {
        match self {
            TextCase::Normal => s.to_owned(),
            TextCase::Upper => s.to_uppercase(),
            TextCase::Lower => s.to_lowercase(),
            TextCase::Title => title_case(s),
        }
    }
}

/// Capitalize the first letter of every whitespace-separated word.
fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            at_word_start = true;
            out.push(ch);
        } else if at_word_start {
            at_word_start = false;
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Horizontal alignment of the laid-out title within the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlignH {
    Left,
    #[default]
    Center,
    Right,
    /// Wrapped lines stretch their spaces to the canvas width; the last line
    /// of each paragraph sits at the left, and the block sits at the left of
    /// the canvas.
    Justify,
}

impl TextAlignH {
    /// Every alignment, in the order a picker lists them.
    pub const ALL: [Self; 4] = [Self::Left, Self::Center, Self::Right, Self::Justify];

    /// The alignment's name as a picker shows it.
    pub fn label(self) -> &'static str {
        match self {
            TextAlignH::Left => "Left",
            TextAlignH::Center => "Center",
            TextAlignH::Right => "Right",
            TextAlignH::Justify => "Justify",
        }
    }
}

/// Vertical alignment of the title block within the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlignV {
    Top,
    #[default]
    Middle,
    Bottom,
}

impl TextAlignV {
    /// Every alignment, in the order a picker lists them.
    pub const ALL: [Self; 3] = [Self::Top, Self::Middle, Self::Bottom];

    /// The alignment's name as a picker shows it.
    pub fn label(self) -> &'static str {
        match self {
            TextAlignV::Top => "Top",
            TextAlignV::Middle => "Middle",
            TextAlignV::Bottom => "Bottom",
        }
    }
}

/// Outline drawn around glyphs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStroke {
    /// Stroke color (RGBA, 0-255).
    pub rgba: Param<[u8; 4]>,
    /// Stroke width in reference pixels (see [`TextStyle::size`]).
    pub width: Param<f32>,
}

impl Default for TextStroke {
    fn default() -> Self {
        Self {
            rgba: Param::Constant([0, 0, 0, 255]),
            width: Param::Constant(6.0),
        }
    }
}

/// A filled card drawn behind the title block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBackground {
    /// Card color (RGBA, 0-255); the alpha doubles as the opacity slider.
    pub rgba: Param<[u8; 4]>,
    /// Corner rounding, `0.0` (square) ..= `1.0` (pill).
    pub radius: Param<f32>,
}

impl Default for TextBackground {
    fn default() -> Self {
        Self {
            rgba: Param::Constant([0, 0, 0, 255]),
            radius: Param::Constant(0.0),
        }
    }
}

/// A soft drop shadow behind the title, offset down-right at 45°.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextShadow {
    /// Shadow color (RGBA, 0-255); the alpha doubles as the opacity slider.
    pub rgba: Param<[u8; 4]>,
    /// Blur radius as a fraction of the effective font size, `0.0`..=`1.0`.
    pub blur: Param<f32>,
    /// Offset distance in reference pixels (see [`TextStyle::size`]).
    pub distance: Param<f32>,
}

impl Default for TextShadow {
    fn default() -> Self {
        Self {
            rgba: Param::Constant([0, 0, 0, 230]),
            blur: Param::Constant(0.15),
            distance: Param::Constant(5.0),
        }
    }
}

/// The full visual treatment of a [`crate::Generator::Text`] layer.
///
/// Sizes (`size`, `letter_spacing`, stroke width, shadow distance) are in
/// *reference pixels* relative to a 1080px-tall canvas; the rasterizer scales
/// them by `canvas_height / 1080` so a project looks the same regardless of
/// output resolution. Every field is `#[serde(default)]` so older projects
/// (which only stored `content`) deserialize to the legacy default look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    /// Font family name (`""` ⇒ the system default font).
    #[serde(default)]
    pub font: String,
    /// Font size in reference pixels (1080px-tall canvas).
    #[serde(default = "default_font_size")]
    pub size: Param<f32>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub case: TextCase,
    /// Fill color (RGBA, 0-255).
    #[serde(default = "default_text_fill")]
    pub fill: Param<[u8; 4]>,
    /// Extra space between glyphs, in reference pixels (can be negative).
    #[serde(default = "default_letter_spacing")]
    pub letter_spacing: Param<f32>,
    /// Apply the font's kerning pairs. Absent from saves while on, so files
    /// written before the switch existed load with kerning on, as they drew.
    #[serde(
        default = "super::default_true",
        skip_serializing_if = "super::is_true"
    )]
    pub kerning: bool,
    /// Line-height multiplier (`1.2` ⇒ 120% of the font size).
    #[serde(default = "default_line_spacing")]
    pub line_spacing: Param<f32>,
    #[serde(default)]
    pub align_h: TextAlignH,
    #[serde(default)]
    pub align_v: TextAlignV,
    /// Whether the title wraps onto multiple lines when it overflows the
    /// canvas width. `true` (default) keeps the legacy auto-wrap; `false` lays
    /// the text out on a single line — explicit newlines still break — so a
    /// long title overflows the frame edges instead of reflowing (CapCut).
    #[serde(default = "default_wrap")]
    pub wrap: bool,
    /// Optional glyph outline.
    #[serde(default)]
    pub stroke: Option<TextStroke>,
    /// Optional background card.
    #[serde(default)]
    pub background: Option<TextBackground>,
    /// Optional drop shadow.
    #[serde(default)]
    pub shadow: Option<TextShadow>,
    /// Text effect preset id (see [`crate::look::text_effect_catalog`]), the
    /// UI's selected chip. Setting a style with a preset **bakes** the
    /// catalog's stroke / shadow / background onto these fields (see
    /// [`crate::Generator::resolve_presets`]), so files stay self-describing;
    /// `None` leaves the manual treatments alone. Absent from saves while
    /// unset, so old files load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_preset: Option<String>,
    /// Per-range character style overrides (`start..end` in character indices).
    /// Empty on clips styled as a whole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextStyleRun>,
}

/// Default font size in reference pixels — matches the legacy `height / 12`
/// look at a 1080px canvas.
fn default_font_size() -> Param<f32> {
    Param::Constant(90.0)
}

/// Default fill color for a title (opaque white), matching the legacy raster.
fn default_text_fill() -> Param<[u8; 4]> {
    Param::Constant([255, 255, 255, 255])
}

/// Default line-height multiplier (matches the legacy `font_size * 1.2`).
fn default_line_spacing() -> Param<f32> {
    Param::Constant(1.2)
}

fn default_letter_spacing() -> Param<f32> {
    Param::Constant(0.0)
}

/// Default wrap behavior (on) — matches the legacy always-wrap raster so older
/// projects, which had no toggle, deserialize to their original look.
fn default_wrap() -> bool {
    true
}

impl TextStyle {
    /// Reject non-finite or unbounded metrics before a style reaches text
    /// layout/rasterization. Called by [`crate::Generator::validate`] for all
    /// project mutations; the renderer still sanitizes defensively for direct
    /// `aria-text` callers and legacy files.
    pub fn validate(&self) -> Result<(), ModelError> {
        self.size.validate_shape()?;
        self.fill.validate_shape()?;
        self.letter_spacing.validate_shape()?;
        self.line_spacing.validate_shape()?;
        self.size
            .for_each_value(|v| validate_range(*v, f32::EPSILON, MAX_TEXT_SIZE, "text size"))?;
        self.letter_spacing.for_each_value(|v| {
            if !v.is_finite() || v.abs() > MAX_TEXT_LETTER_SPACING {
                return Err(ModelError::InvalidParam(format!(
                    "text letter spacing must be finite and within -{MAX_TEXT_LETTER_SPACING}..={MAX_TEXT_LETTER_SPACING} reference px"
                )));
            }
            Ok(())
        })?;
        self.line_spacing.for_each_value(|v| {
            validate_range(*v, f32::EPSILON, MAX_TEXT_LINE_SPACING, "text line spacing")
        })?;
        if let Some(stroke) = &self.stroke {
            stroke.rgba.validate_shape()?;
            stroke.width.validate_shape()?;
            stroke.width.for_each_value(|v| {
                validate_range(*v, 0.0, MAX_TEXT_STROKE_WIDTH, "text stroke width")
            })?;
        }
        if let Some(background) = &self.background {
            background.rgba.validate_shape()?;
            background.radius.validate_shape()?;
            background
                .radius
                .for_each_value(|v| validate_range(*v, 0.0, 1.0, "text background radius"))?;
        }
        if let Some(shadow) = &self.shadow {
            shadow.rgba.validate_shape()?;
            shadow.blur.validate_shape()?;
            shadow.distance.validate_shape()?;
            shadow
                .blur
                .for_each_value(|v| validate_range(*v, 0.0, 1.0, "text shadow blur"))?;
            shadow.distance.for_each_value(|v| {
                validate_range(*v, 0.0, MAX_TEXT_SHADOW_DISTANCE, "text shadow distance")
            })?;
        }
        self.validate_runs()
    }
}

fn validate_range(value: f32, min: f32, max: f32, what: &str) -> Result<(), ModelError> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(ModelError::InvalidParam(format!(
            "{what} must be in {min}..={max}"
        )));
    }
    Ok(())
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: String::new(),
            size: default_font_size(),
            bold: false,
            italic: false,
            underline: false,
            case: TextCase::Normal,
            fill: default_text_fill(),
            letter_spacing: default_letter_spacing(),
            kerning: true,
            line_spacing: default_line_spacing(),
            align_h: TextAlignH::Center,
            align_v: TextAlignV::Middle,
            wrap: default_wrap(),
            stroke: None,
            background: None,
            shadow: None,
            effect_preset: None,
            runs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_case_applies_unicode_and_accents_correctly() {
        assert_eq!(TextCase::Normal.apply("hello WORLD"), "hello WORLD");
        assert_eq!(TextCase::Upper.apply("élégant straße"), "ÉLÉGANT STRASSE");
        assert_eq!(TextCase::Lower.apply("ÉLÉGANT STRASSE"), "élégant strasse");
        assert_eq!(
            TextCase::Title.apply("élégant straße café"),
            "Élégant Straße Café"
        );
        assert_eq!(
            TextCase::Title.apply("hello-world test"),
            "Hello-world Test"
        );
        assert_eq!(
            TextCase::Title.apply("  multiple   spaces  "),
            "  Multiple   Spaces  "
        );
    }

    #[test]
    fn text_style_validate_bounds() {
        let style = TextStyle::default();
        assert!(style.validate().is_ok());

        let mut invalid_size = style.clone();
        invalid_size.size = Param::Constant(0.0);
        assert!(invalid_size.validate().is_err());

        let mut invalid_spacing = style;
        invalid_spacing.letter_spacing = Param::Constant(-(MAX_TEXT_LETTER_SPACING + 1.0));
        assert!(invalid_spacing.validate().is_err());
    }
}
