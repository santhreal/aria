//! Shaping, rasterizing and memoization cases.
//!
//! A pure move out of `lib.rs`; the module reaches its fixtures through
//! `use super::*` exactly as it did inline.

use super::*;

/// Bundled OFL test face (`assets/`), so every test shapes real glyphs
/// deterministically — even on a fontless CI box.
const TEST_FONT: &[u8] = include_bytes!("../assets/Micro5-Regular.ttf");

/// A renderer guaranteed to have at least the bundled face.
fn test_renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    let added = r.load_font(TEST_FONT.to_vec());
    assert!(added > 0, "bundled test font failed to parse");
    assert!(r.font_count() > 0);
    r
}

/// Count pixels with non-zero alpha (glyph coverage).
fn covered(img: &RgbaImage) -> usize {
    img.pixels.chunks_exact(4).filter(|p| p[3] != 0).count()
}

#[test]
fn load_font_reports_added_faces() {
    let mut r = TextRenderer::new();
    assert_eq!(
        r.load_font(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        0,
        "garbage bytes are not a font"
    );
    let before = r.font_count();
    assert!(r.load_font(TEST_FONT.to_vec()) > 0);
    assert!(r.font_count() > before);
}

#[test]
fn empty_text_is_zero_area() {
    let mut r = TextRenderer::new();
    let img = r.rasterize("", &TextStyle::new(32.0));
    assert_eq!(img.width, 0);
    assert_eq!(img.height, 0);
    assert!(img.is_well_formed());
}

#[test]
fn whitespace_is_zero_area_even_with_padding() {
    let mut r = TextRenderer::new();
    // Spaces have advances but no ink; padding must not manufacture a
    // blank bitmap out of them (holds with or without fonts installed).
    let img = r.rasterize("   ", &TextStyle::new(32.0).with_padding(8));
    assert_eq!((img.width, img.height), (0, 0));
    let empty_padded = r.rasterize("", &TextStyle::new(32.0).with_padding(8));
    assert_eq!((empty_padded.width, empty_padded.height), (0, 0));
}

#[test]
fn rasterizes_glyph_coverage() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0).with_color([255, 0, 0, 255]);
    let img = r.rasterize("Hi", &style);

    assert!(img.is_well_formed());
    assert!(
        img.width > 0 && img.height > 0,
        "got {}x{}",
        img.width,
        img.height
    );

    // Some pixels must be covered, and covered pixels carry the fill color
    // with no green/blue (we asked for pure red).
    let lit = covered(&img);
    assert!(lit > 0, "no glyph coverage rasterized");
    for p in img.pixels.chunks_exact(4).filter(|p| p[3] != 0) {
        assert!(
            p[0] >= p[1] && p[0] >= p[2],
            "fill color not red-dominant: {p:?}"
        );
        assert_eq!(p[2], 0, "blue leaked into a red fill: {p:?}");
    }
}

#[test]
fn padding_grows_the_bitmap_symmetrically() {
    let mut r = test_renderer();
    let base = r.rasterize("A", &TextStyle::new(40.0));
    let padded = r.rasterize("A", &TextStyle::new(40.0).with_padding(5));
    assert_eq!(padded.width, base.width + 10);
    assert_eq!(padded.height, base.height + 10);
    // The outermost ring of the padded bitmap is fully transparent.
    assert_eq!(padded.pixel(0, 0), [0, 0, 0, 0]);
}

#[test]
fn style_alpha_scales_coverage() {
    let mut r = test_renderer();
    let opaque = r.rasterize("W", &TextStyle::new(48.0).with_color([255, 255, 255, 255]));
    let faded = r.rasterize("W", &TextStyle::new(48.0).with_color([255, 255, 255, 128]));

    let max_a = |img: &RgbaImage| img.pixels.chunks_exact(4).map(|p| p[3]).max().unwrap_or(0);
    assert!(
        max_a(&opaque) > max_a(&faded),
        "style alpha did not reduce coverage"
    );
}

#[test]
fn shape_exposes_clusters_in_text_order() {
    let mut r = test_renderer();
    let shaped = r.shape("Hi", &TextStyle::new(48.0));
    assert!(shaped.has_ink());
    assert_eq!(shaped.clusters.len(), 2);
    assert_eq!(shaped.clusters[0].text_range, 0..1);
    assert_eq!(shaped.clusters[1].text_range, 1..2);
    assert!(shaped.clusters.iter().all(|c| c.image.width > 0));
    // LTR: the second character sits to the right of the first.
    assert!(shaped.clusters[1].offset[0] > shaped.clusters[0].offset[0]);
    assert_eq!(shaped.clusters[0].line, 0);
}

#[test]
fn shape_maps_lines_and_baselines() {
    let mut r = test_renderer();
    let shaped = r.shape("A\nB", &TextStyle::new(48.0));
    assert_eq!(shaped.clusters.len(), 2);
    // Byte ranges are in the source string, skipping the newline.
    assert_eq!(shaped.clusters[0].text_range, 0..1);
    assert_eq!(shaped.clusters[1].text_range, 2..3);
    assert_eq!((shaped.clusters[0].line, shaped.clusters[1].line), (0, 1));
    assert!(
        shaped.clusters[1].baseline > shaped.clusters[0].baseline,
        "second line's baseline must be lower"
    );
}

#[test]
fn shape_extent_matches_rasterize() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0);
    let shaped = r.shape("AV", &style);
    let img = r.rasterize("AV", &style);
    assert_eq!((img.width, img.height), shaped.extent);
    assert!(covered(&img) > 0);
}

#[test]
fn center_align_with_wrap_width_does_not_clip() {
    let mut r = test_renderer();
    // Regression: centered text in a wide wrap box used to render into a
    // bitmap sized without the alignment offset, clipping half the glyphs.
    let left = r.rasterize("Hi", &TextStyle::new(48.0).with_max_width(400.0));
    let centered = r.rasterize(
        "Hi",
        &TextStyle::new(48.0)
            .with_align(TextAlign::Center)
            .with_max_width(400.0),
    );
    // Subpixel positioning may wiggle the AA fringe by a pixel; anything
    // more means the alignment offset leaked into the bitmap size again.
    assert!(
        left.width.abs_diff(centered.width) <= 1 && left.height.abs_diff(centered.height) <= 1,
        "centered {}x{} vs left {}x{}",
        centered.width,
        centered.height,
        left.width,
        left.height
    );
    // Coverage must match within a small tolerance, not lose half the
    // glyphs (the old bug clipped the right half of the centered run).
    let (a, b) = (covered(&left) as i64, covered(&centered) as i64);
    assert!(
        (a - b).abs() <= (a / 5).max(8),
        "centered run lost coverage: left={a} centered={b}"
    );
}

#[test]
fn center_align_without_wrap_width_centers_short_lines() {
    let mut r = test_renderer();
    // The lone "H" on line 2 must shift right when centering against the
    // wider first line — previously a silent no-op without max_width.
    let style = TextStyle::new(40.0);
    let lone_h = |shaped: &ShapedText| {
        shaped
            .clusters
            .iter()
            .find(|c| c.line == 1)
            .expect("second line cluster")
            .offset[0]
    };
    let flush = r.shape("HHHH\nH", &style);
    let centered = r.shape("HHHH\nH", &style.clone().with_align(TextAlign::Center));
    assert!(lone_h(&flush) < 5.0, "left-aligned H should hug the margin");
    assert!(
        lone_h(&centered) > lone_h(&flush) + 10.0,
        "centered H did not move: flush={} centered={}",
        lone_h(&flush),
        lone_h(&centered)
    );
}

#[test]
fn letter_spacing_changes_shaped_width() {
    let mut r = test_renderer();
    let compact = r.shape("HHHH", &TextStyle::new(48.0));
    let tracked = r.shape("HHHH", &TextStyle::new(48.0).with_letter_spacing(12.0));
    assert!(
        tracked.extent.0 > compact.extent.0 + 20,
        "tracking did not widen text: compact={} tracked={}",
        compact.extent.0,
        tracked.extent.0
    );
}

/// Mean x of the inked pixels in the top and bottom fifth of a bitmap.
///
/// An upright stem puts both at the same place; a slanted one leans the top
/// to the right of the bottom.
fn ink_lean(img: &RgbaImage) -> f32 {
    let band = (img.height / 5).max(1);
    let mut mean = [0.0f32; 2];
    for (slot, rows) in [(0usize, 0..band), (1, img.height - band..img.height)] {
        let mut sum = 0.0f32;
        let mut lit = 0.0f32;
        for y in rows {
            for x in 0..img.width {
                let px = ((y * img.width + x) * 4) as usize;
                if img.pixels[px + 3] != 0 {
                    sum += x as f32;
                    lit += 1.0;
                }
            }
        }
        mean[slot] = if lit > 0.0 { sum / lit } else { 0.0 };
    }
    mean[0] - mean[1]
}

#[test]
fn bold_lays_down_more_ink_than_regular() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0);
    let regular = r.rasterize("HHHH", &style);
    let bold = r.rasterize("HHHH", &style.clone().with_bold(true));
    let (thin, thick) = (covered(&regular), covered(&bold));
    assert!(
        thick > thin,
        "bold rasterized no extra ink: regular={thin} bold={thick}"
    );
}

#[test]
fn italic_slants_the_ink() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0);
    let upright = ink_lean(&r.rasterize("HHHH", &style));
    let slanted = ink_lean(&r.rasterize("HHHH", &style.clone().with_italic(true)));
    assert!(
        slanted > upright + 1.0,
        "italic did not lean the glyphs: upright={upright} italic={slanted}"
    );
}

#[test]
fn letter_spacing_widens_the_bitmap_by_the_tracking() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0);
    let compact = r.rasterize("HHHH", &style);
    let spacing = 12.0f32;
    let tracked = r.rasterize("HHHH", &style.clone().with_letter_spacing(spacing));
    // Three gaps between four glyphs, so the run grows by 3 × spacing. Two
    // pixels of slack covers rounding at the bitmap edges.
    let wanted = spacing * 3.0;
    let grew = tracked.width as f32 - compact.width as f32;
    assert!(
        (grew - wanted).abs() <= 2.0,
        "tracking widened the raster by {grew}px, wanted about {wanted}px \
         (compact={} tracked={})",
        compact.width,
        tracked.width
    );
}

#[test]
fn max_width_wraps_text_onto_multiple_lines() {
    let mut r = test_renderer();
    let shaped = r.shape("HH HH HH", &TextStyle::new(48.0).with_max_width(70.0));
    assert!(
        shaped.clusters.iter().any(|cluster| cluster.line > 0),
        "wrap width left all clusters on one line"
    );
}

#[test]
fn memo_caches_repeat_calls_and_invalidate_on_font_load() {
    let mut r = test_renderer();
    let style = TextStyle::new(48.0);

    // Repeat rasterize: one entry each (rasterize populates shape too),
    // identical output.
    let first = r.rasterize("Hi", &style);
    let again = r.rasterize("Hi", &style);
    assert_eq!(first, again);
    assert_eq!(r.memo_sizes(), (1, 1));

    // Same key through shape() hits the shared entry. Bitmap-only style
    // changes create raster entries but keep reusing that shaped run.
    let _ = r.shape("Hi", &style);
    assert_eq!(r.memo_sizes(), (1, 1));
    let _ = r.rasterize("Hi", &style.clone().with_padding(3));
    assert_eq!(r.memo_sizes(), (1, 2));
    let _ = r.rasterize(
        "Hi",
        &style.clone().with_stroke(TextStroke {
            rgba: [0, 0, 0, 255],
            width: 6.0,
        }),
    );
    assert_eq!(r.memo_sizes(), (1, 3));

    // Loading a font can change shaping for any string: memos must drop.
    assert!(r.load_font(TEST_FONT.to_vec()) > 0);
    assert_eq!(r.memo_sizes(), (0, 0));
}

/// **The host font tree is parsed once, while custom faces stay isolated.**
///
/// A renderer used to call `FontSystem::new`, so every engine and background
/// render walked and parsed every installed font. Sharing the parsed catalog
/// must not turn `load_font` into process-global state: a project that registers
/// a bundled face cannot change fallback in another project.
#[test]
fn renderers_share_one_system_scan_and_not_custom_fonts() {
    let mut first = TextRenderer::new();
    let second = TextRenderer::new();
    let families = system_font_families();
    assert!(
        !families.is_empty(),
        "the system catalog found no font families"
    );
    assert_eq!(
        SYSTEM_FONT_SCANS.load(Ordering::Relaxed),
        1,
        "constructing two renderers and listing families rescanned the host"
    );

    let second_before = second.font_count();
    assert!(
        first.load_font(TEST_FONT.to_vec()) > 0,
        "the bundled face did not load into the first renderer"
    );
    assert_eq!(
        second.font_count(),
        second_before,
        "a custom face loaded into a different renderer"
    );
}

/// **One owner parses the host font catalog.**
///
/// The behavior test above proves the shared path scans once. This source
/// census closes the sibling regression: a new renderer or picker must not call
/// the upstream constructors directly and bypass the shared catalog.
#[test]
fn one_source_site_may_scan_system_fonts() {
    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
        for entry in entries {
            let entry = entry.expect("source directory entry");
            let path = entry.path();
            if path.is_dir() {
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(files.len() > 5, "the aria-text source census is vacuous");

    let font_system_new = ["FontSystem", "::new()"].concat();
    let database_scan = ["load_system", "_fonts()"].concat();
    let mut owners = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()));
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(&font_system_new) || line.contains(&database_scan) {
                owners.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(&root).unwrap_or(&file).display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert_eq!(
        owners.len(),
        1,
        "system fonts are scanned at {} source sites instead of one:\n  {}",
        owners.len(),
        owners.join("\n  ")
    );
    assert!(
        owners[0].starts_with("lib.rs:"),
        "the shared catalog is not the scan owner: {}",
        owners[0]
    );
}
