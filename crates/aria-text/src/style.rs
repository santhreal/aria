//! How a text run should look: size, color, family, alignment, wrapping,
//! and optional stroke / background / shadow treatments.

/// Horizontal alignment of wrapped / multi-line text inside its bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    /// Wrapped lines stretch their spaces to fill the wrap width; the last
    /// line of each paragraph sits at the left.
    Justify,
}

/// Vertical placement of an ink-tight text bitmap within its canvas.
///
/// This does not add transparent pixels to the raster. The render layer uses
/// it when positioning the finished bitmap, so selection bounds continue to
/// hug the visible text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextVerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// Which font family to shape with. `Named` looks the family up by name in the
/// loaded font set; the generic families fall back to whatever the platform
/// (or a loaded font) provides.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum FontFamily {
    #[default]
    SansSerif,
    Serif,
    Monospace,
    Named(String),
}

/// Outline drawn around glyphs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStroke {
    /// Stroke color (straight-alpha RGBA).
    pub rgba: [u8; 4],
    /// Stroke width in pixels (already canvas-scaled by the caller).
    pub width: f32,
}

/// A filled card drawn behind the title block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBackground {
    /// Card color (straight-alpha RGBA); alpha is the opacity.
    pub rgba: [u8; 4],
    /// Corner rounding, `0.0` (square) ..= `1.0` (pill).
    pub radius: f32,
}

/// Soft drop shadow behind the title, offset down-right at 45°.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    /// Shadow color (straight-alpha RGBA); alpha is the opacity.
    pub rgba: [u8; 4],
    /// Blur radius as a fraction of the font size, `0.0`..=`1.0`.
    pub blur: f32,
    /// Offset distance in pixels (already canvas-scaled by the caller).
    pub distance: f32,
}

/// Per-byte-range style override within a shaped text string.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRun {
    /// Byte range `start..end` in the shaped text string.
    pub range: std::ops::Range<usize>,
    pub family: Option<FontFamily>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<[u8; 4]>,
    pub letter_spacing: Option<f32>,
}

/// The styling for a rasterized text run.
///
/// Construct with [`TextStyle::new`] (a size, white, left-aligned, sans-serif,
/// unwrapped) and adjust with the `with_*` builders.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Font size in pixels.
    pub font_size: f32,
    /// Baseline-to-baseline line height in pixels.
    pub line_height: f32,
    /// Straight-alpha RGBA fill (the `a` scales the whole run's opacity).
    pub color: [u8; 4],
    pub family: FontFamily,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Extra tracking in pixels. The layout adapter converts this to the
    /// em-relative value expected by the shaping engine.
    pub letter_spacing: f32,
    /// Apply the font's kerning pairs. Off, every glyph advances by its own
    /// width alone.
    pub kerning: bool,
    pub align: TextAlign,
    pub vertical_align: TextVerticalAlign,
    /// Wrap width in pixels. `None` lays each paragraph out on one line; `Some`
    /// word-wraps to that width.
    pub max_width: Option<f32>,
    /// Transparent margin (px) added on every side of the measured text box —
    /// headroom for glyph overhang. Stroke / shadow / background compute their
    /// own additional headroom on top of this.
    pub padding: u32,
    /// Optional glyph outline (composited under the fill).
    pub stroke: Option<TextStroke>,
    /// Optional background card behind the text block.
    pub background: Option<TextBackground>,
    /// Optional drop shadow / glow behind the text.
    pub shadow: Option<TextShadow>,
    /// Optional per-range character overrides (`range` in byte offsets).
    pub runs: Vec<TextRun>,
}

impl TextStyle {
    /// A white, left-aligned, unwrapped sans-serif run at `font_size` px, with
    /// a 1.25× line height.
    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            line_height: font_size * 1.25,
            color: [255, 255, 255, 255],
            family: FontFamily::SansSerif,
            bold: false,
            italic: false,
            underline: false,
            letter_spacing: 0.0,
            kerning: true,
            align: TextAlign::Left,
            vertical_align: TextVerticalAlign::Middle,
            max_width: None,
            padding: 0,
            stroke: None,
            background: None,
            shadow: None,
            runs: Vec::new(),
        }
    }

    pub fn with_color(mut self, color: [u8; 4]) -> Self {
        self.color = color;
        self
    }

    pub fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    pub fn with_letter_spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }

    pub fn with_kerning(mut self, kerning: bool) -> Self {
        self.kerning = kerning;
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn with_vertical_align(mut self, align: TextVerticalAlign) -> Self {
        self.vertical_align = align;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// Word-wrap to `width` pixels.
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn with_padding(mut self, padding: u32) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_stroke(mut self, stroke: TextStroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    pub fn with_background(mut self, background: TextBackground) -> Self {
        self.background = Some(background);
        self
    }

    pub fn with_shadow(mut self, shadow: TextShadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        self.runs = runs;
        self
    }

    /// Effective fill color at `byte` offset in the shaped string.
    pub fn color_at(&self, byte: usize) -> [u8; 4] {
        self.runs
            .iter()
            .rev()
            .find(|r| r.range.contains(&byte))
            .and_then(|r| r.color)
            .unwrap_or(self.color)
    }

    /// Effective underline flag at `byte` offset in the shaped string.
    pub fn underline_at(&self, byte: usize) -> bool {
        self.runs
            .iter()
            .rev()
            .find(|r| r.range.contains(&byte))
            .and_then(|r| r.underline)
            .unwrap_or(self.underline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextRenderer;

    #[test]
    fn per_run_overrides_tint_clusters_and_scale_run_metrics() {
        let mut r = TextRenderer::new();
        if r.font_count() == 0 {
            return;
        }
        let base = TextStyle::new(36.0).with_color([255, 255, 255, 255]);
        let plain = r.shape("AB", &base);
        let styled = base.with_runs(vec![TextRun {
            range: 0..1,
            family: None,
            font_size: Some(72.0),
            bold: Some(true),
            italic: None,
            underline: Some(true),
            color: Some([255, 0, 0, 255]),
            letter_spacing: None,
        }]);
        let shaped = r.shape("AB", &styled);
        assert!(shaped.extent.1 > plain.extent.1, "72px run grows height");
        let first_ink = shaped.clusters[0]
            .image
            .pixels
            .chunks_exact(4)
            .find(|px| px[3] == 255)
            .expect("first cluster has opaque ink");
        assert_eq!([first_ink[0], first_ink[1], first_ink[2]], [255, 0, 0]);
        let second_ink = shaped.clusters[1]
            .image
            .pixels
            .chunks_exact(4)
            .find(|px| px[3] == 255)
            .expect("second cluster has opaque ink");
        assert_eq!(
            [second_ink[0], second_ink[1], second_ink[2]],
            [255, 255, 255]
        );
    }
}
