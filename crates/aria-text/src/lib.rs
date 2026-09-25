//! aria-text: shape and rasterize styled text into straight-alpha
//! [`RgbaImage`] bitmaps the compositor can place as generator layers.
//!
//! Text is a **generator** — pixels are produced, not decoded — so this crate
//! stays pure-Rust and GPU-free: it depends only on [`cosmic_text`] (shaping +
//! layout via `rustybuzz`/`swash`) and [`aria_core`]. The engine wires the
//! resulting bitmaps to compositor RGBA layers; nothing here, in its tests, or
//! in its benches depends on `wgpu`.
//!
//! Two entry points share one shaping/measurement path:
//!
//! - [`TextRenderer::rasterize`]: the whole run as a single bitmap, for static
//!   text.
//! - [`TextRenderer::shape`]: the run as positioned per-cluster bitmaps
//!   ([`ShapedText`]), for character-level animation. The full string is
//!   shaped **once** — kerning, ligatures, BiDi, and complex scripts stay
//!   correct — and the animation system then transforms each cluster's
//!   placement without ever re-shaping.
//!
//! ```no_run
//! use aria_text::{TextRenderer, TextStyle};
//!
//! let mut text = TextRenderer::new();
//! let style = TextStyle::new(48.0).with_color([255, 220, 0, 255]);
//! let bitmap = text.rasterize("Aria", &style); // straight-alpha RgbaImage
//! let shaped = text.shape("Aria", &style); // one ClusterBox per character
//! ```
//!
//! Fonts come from the host by default ([`TextRenderer::new`]); call
//! [`TextRenderer::load_font`] to add bundled/custom faces for deterministic,
//! platform-independent output.

mod animated;
mod effects;
mod style;

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_core::RgbaImage;
use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, Family, FontSystem, LineIter, Metrics, Shaping,
    Style as FontStyle, SwashCache, SwashContent, SwashImage, Weight,
};

pub use animated::{AnimatedText, paint_animated};
pub use style::{
    FontFamily, TextAlign, TextBackground, TextShadow, TextStroke, TextStyle, TextVerticalAlign,
};

struct SystemFonts {
    locale: String,
    database: cosmic_text::fontdb::Database,
    families: Vec<String>,
}

#[cfg(test)]
static SYSTEM_FONT_SCANS: AtomicUsize = AtomicUsize::new(0);

/// The installed font catalog, parsed once for every text renderer in the
/// process.
///
/// `FontSystem::new` walks every system font directory and parses every face.
/// A renderer exists per engine and per background job, so doing that in
/// `TextRenderer::new` made application-state construction grow with the host's
/// installed fonts and made parallel jobs scan the same files together.
static SYSTEM_FONTS: LazyLock<SystemFonts> = LazyLock::new(|| {
    #[cfg(test)]
    SYSTEM_FONT_SCANS.fetch_add(1, Ordering::Relaxed);
    let font_system = FontSystem::new();
    let (locale, database) = font_system.into_locale_and_db();
    let mut families: Vec<String> = database
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .collect();
    families.sort_unstable();
    families.dedup();
    SystemFonts {
        locale,
        database,
        families,
    }
});

/// A font system with isolated shaping state over the one parsed host catalog.
fn font_system() -> FontSystem {
    FontSystem::new_with_locale_and_db(SYSTEM_FONTS.locale.clone(), SYSTEM_FONTS.database.clone())
}

/// Enumerate installed font family names, deduplicated and sorted.
///
/// The returned vector is independent, but no call scans or parses the host's
/// font directories again.
pub fn system_font_families() -> Vec<String> {
    SYSTEM_FONTS.families.clone()
}

/// A text run shaped and rasterized as individually placeable pieces — the
/// substrate for character-level animation (typewriter, per-char fade, wave).
///
/// Produced by [`TextRenderer::shape`]. Compositing every cluster's `image` at
/// its `offset` reproduces [`TextRenderer::rasterize`] exactly (minus padding);
/// an animator instead perturbs each cluster's placement/opacity per frame,
/// which never re-shapes the text.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedText {
    /// Ink-tight pixel size of the whole run (no padding). `(0, 0)` when
    /// nothing produced coverage (empty/whitespace-only text, or no fonts).
    pub extent: (u32, u32),
    /// Animation units in logical (text) order: sorted by line, then by byte
    /// position — the order a typewriter effect reveals them, also for RTL.
    pub clusters: Vec<ClusterBox>,
}

impl ShapedText {
    /// True when no cluster produced any coverage.
    pub fn has_ink(&self) -> bool {
        self.extent.0 > 0 && self.extent.1 > 0
    }
}

/// One shaping cluster of a [`ShapedText`]: the animation unit.
///
/// A cluster is what the shaper considers indivisible — usually one user-
/// perceived character ("é", emoji + ZWJ sequences), and one unit per ligature
/// ("fi" moves as a whole, like CapCut/AE text animators). Whitespace clusters
/// are kept with a zero-area `image` so stagger timing still counts them as a
/// beat.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterBox {
    /// Byte range of this cluster in the source string.
    pub text_range: Range<usize>,
    /// Zero-based visual line index (wrapped lines count separately) — for
    /// by-line staggers.
    pub line: usize,
    /// Top-left of `image` within the run's [`extent`](ShapedText::extent)
    /// box, in pixels.
    pub offset: [f32; 2],
    /// Baseline y of this cluster's line, relative to the extent box top —
    /// the anchor for rise/drop animations.
    pub baseline: f32,
    /// This cluster's glyphs, ink-tight, straight alpha, style color/opacity
    /// folded in. Zero-area for clusters with no coverage (spaces).
    pub image: RgbaImage,
}

/// Shapes and rasterizes text. Holds the font set, a glyph cache, and memo
/// caches of whole-run results, so reuse one renderer across many calls:
/// repeating a `shape`/`rasterize` call with unchanged input costs a memo
/// lookup plus a copy-out of the cached bitmap(s) — no re-shaping. That keeps
/// per-frame callers (preview scrub, export loops) off the shaping path.
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Memoized [`shape`](Self::shape) results, keyed by (text, style).
    /// Cleared by [`load_font`](Self::load_font) — a new face can change
    /// shaping/fallback for any string.
    shape_memo: HashMap<ShapeKey, ShapedText>,
    /// Memoized [`rasterize`](Self::rasterize) results (padding folded in).
    raster_memo: HashMap<RasterKey, RgbaImage>,
}

impl std::fmt::Debug for TextRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextRenderer")
            .field("shaped", &self.shape_memo.len())
            .field("rasterized", &self.raster_memo.len())
            .finish()
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashable identity of the fields that affect shaping and glyph pixels.
/// Bitmap-only treatments deliberately stay out: dragging a stroke/shadow/
/// background control must reuse the already-shaped glyph run.
///
/// `f32`s are keyed by bit pattern, so styles that differ only in float
/// encoding are simply distinct keys (never a wrong hit).
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
    text: String,
    font_size: u32,
    line_height: u32,
    color: [u8; 4],
    family: FontFamily,
    bold: bool,
    italic: bool,
    letter_spacing: u32,
    align: TextAlign,
    max_width: Option<u32>,
}

impl ShapeKey {
    fn new(text: &str, style: &TextStyle) -> Self {
        Self {
            text: text.to_owned(),
            font_size: style.font_size.to_bits(),
            line_height: style.line_height.to_bits(),
            color: style.color,
            family: style.family.clone(),
            bold: style.bold,
            italic: style.italic,
            letter_spacing: style.letter_spacing.to_bits(),
            align: style.align,
            max_width: style.max_width.map(f32::to_bits),
        }
    }
}

/// Full bitmap identity: the shaped run plus bitmap padding/treatments.
#[derive(PartialEq, Eq, Hash)]
struct RasterKey {
    shape: ShapeKey,
    underline: bool,
    padding: u32,
    stroke: Option<([u8; 4], u32)>,
    background: Option<([u8; 4], u32)>,
    shadow: Option<([u8; 4], u32, u32)>,
}

impl RasterKey {
    fn new(text: &str, style: &TextStyle) -> Self {
        Self {
            shape: ShapeKey::new(text, style),
            underline: style.underline,
            padding: style.padding,
            stroke: style.stroke.map(|s| (s.rgba, s.width.to_bits())),
            background: style.background.map(|b| (b.rgba, b.radius.to_bits())),
            shadow: style
                .shadow
                .map(|s| (s.rgba, s.blur.to_bits(), s.distance.to_bits())),
        }
    }
}

/// A glyph's draw rectangle in buffer space.
#[derive(Clone, Copy)]
struct InkBox {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

impl InkBox {
    /// `(min_x, min_y, max_x, max_y)`, exclusive maxima.
    fn bounds(self) -> (i32, i32, i32, i32) {
        (
            self.left,
            self.top,
            self.left + self.width as i32,
            self.top + self.height as i32,
        )
    }
}

fn union(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

struct PendingGlyph {
    key: CacheKey,
    ink: Option<InkBox>,
}

struct PendingCluster {
    line: usize,
    range: Range<usize>,
    pen_x: f32,
    line_top: f32,
    baseline_y: f32,
    glyphs: Vec<PendingGlyph>,
    ink: Option<(i32, i32, i32, i32)>,
}

/// Blend one glyph image into a cluster bitmap at `(dx, dy)` (in-bounds by
/// construction: the cluster box is the union of its glyphs' boxes).
///
/// Color semantics mirror cosmic-text's `SwashCache::with_pixels`: `Mask`
/// content is coverage tinted with the style color; `Color` content (emoji)
/// carries its own RGBA. The style's alpha scales both.
fn write_glyph(
    pixels: &mut [u8],
    stride_w: u32,
    dx: i32,
    dy: i32,
    img: &SwashImage,
    color: [u8; 4],
    style_alpha: u32,
) {
    let (gw, gh) = (img.placement.width as usize, img.placement.height as usize);
    let fold = |coverage: u8| ((u32::from(coverage) * style_alpha + 127) / 255) as u8;
    let mut blend = |col: usize, row: usize, src: [u8; 4]| {
        if src[3] == 0 {
            return;
        }
        let (px, py) = (dx as usize + col, dy as usize + row);
        let idx = (py * stride_w as usize + px) * 4;
        over_straight(&mut pixels[idx..idx + 4], src);
    };

    match img.content {
        SwashContent::Mask => {
            for row in 0..gh {
                for col in 0..gw {
                    let coverage = img.data[row * gw + col];
                    if coverage == 0 {
                        continue;
                    }
                    blend(col, row, [color[0], color[1], color[2], fold(coverage)]);
                }
            }
        }
        SwashContent::Color => {
            for row in 0..gh {
                for col in 0..gw {
                    let i = (row * gw + col) * 4;
                    blend(
                        col,
                        row,
                        [
                            img.data[i],
                            img.data[i + 1],
                            img.data[i + 2],
                            fold(img.data[i + 3]),
                        ],
                    );
                }
            }
        }
        // swash never emits subpixel masks for the alpha/color sources
        // cosmic-text requests; if that changes, read the RGB mean as coverage.
        SwashContent::SubpixelMask => {
            for row in 0..gh {
                for col in 0..gw {
                    let i = (row * gw + col) * 4;
                    let mean = (u16::from(img.data[i])
                        + u16::from(img.data[i + 1])
                        + u16::from(img.data[i + 2]))
                        / 3;
                    blend(col, row, [color[0], color[1], color[2], fold(mean as u8)]);
                }
            }
        }
    }
}

/// Straight-alpha "source over destination" compositing of one pixel. Both
/// `dst` and `src` are non-premultiplied RGBA; `dst` is updated in place.
/// Callers skip fully transparent sources (`src[3] > 0` is a precondition).
pub(crate) fn over_straight(dst: &mut [u8], src: [u8; 4]) {
    // Fast path: an opaque source (the common interior-of-glyph case) replaces.
    if src[3] == 255 {
        dst.copy_from_slice(&src);
        return;
    }
    let sa = f32::from(src[3]) / 255.0;
    let da = f32::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    for i in 0..3 {
        let s = f32::from(src[i]) / 255.0;
        let d = f32::from(dst[i]) / 255.0;
        let c = (s * sa + d * da * (1.0 - sa)) / out_a;
        dst[i] = (c * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

// The renderer's methods and the test module live beside this file. Pure moves;
// nothing here changes what the crate exports.
#[path = "lib_renderer.rs"]
mod renderer;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
