//! Committed reference pictures, and the diff that names what moved.
//!
//! The class this closes: a panel that draws pixels rather than elements has no
//! assertible output. Three of them do — the waveform monitor, the vectorscope
//! and the RGB histogram are painted into [`RgbaImage`] by [`crate::scopes`] and
//! handed to GPUI as one texture each, because a cell per element would cost
//! more per frame than the render that produced the picture. Every test in this
//! crate drove the *measurement* and nothing ever looked at the picture, so a
//! graticule drawn at the wrong level, an inverted axis, or an intensity ramp
//! that hides the trace would all have shipped green.
//!
//! A reference lives in `crates/aria-ui/references/<name>.png` and is compared
//! pixel for pixel. A failure names the bounding box of everything that differs
//! and prints a coarse map of it over the whole picture, so a shifted graticule
//! (one full-width row) reads differently from a moved trace (a blob) and from a
//! repainted canvas (solid) — three regressions that are the same pixel count.
//! Nothing is written, on success or on failure.
//!
//! **Blessing.** A reference is created, and an existing one replaced, by
//! `ARIA_BLESS_SNAPSHOTS=1 cargo test -p aria-ui --lib snapshot_diff`. Without
//! it a missing reference is a failure, so a new picture cannot ship
//! unreviewed — the same shape as `BLESS_TOOL_SCHEMA`.
//!
//! **What this does not cover: the element tree.** GPUI panels are laid out by
//! taffy and rasterized by the platform, and the pinned revision's headless
//! rasterizer (`Window::render_to_image`) is macOS-only —
//! `gpui_platform::current_headless_renderer` returns `None` everywhere else, so
//! `VisualTestContext::capture_screenshot` cannot produce a picture on this
//! host. What a *layout* regression does to the tree is covered by
//! `tests_shell.rs`, which draws every panel and every dialog and asserts the
//! projected state; what it does to pixels is not assertible here until that
//! rasterizer exists on Linux.
//!
//! The other picture to judge, the composited preview frame, is not here
//! either, and deliberately: it comes out of a GPU rasterizer, so the same
//! scene is not bit-identical across a discrete GPU and a software fallback, and
//! a reference PNG would be a reference to one machine. Composite output is
//! asserted by readback tests in `aria-compositor`, against values rather than
//! against a file.

use std::path::PathBuf;

use aria_core::RgbaImage;

/// Per-channel difference a pixel may carry and still count as unchanged.
///
/// One code value, not zero: the scope painters go through `f32` luma and
/// chroma weights, and a target that contracts a multiply-add differently
/// rounds a boundary pixel by one. Two would start hiding a real off-by-one in
/// a graticule.
pub const TOLERANCE: u8 = 1;

/// Environment variable that rewrites references instead of asserting on them.
pub const BLESS: &str = "ARIA_BLESS_SNAPSHOTS";

/// What changed between a picture and its reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    /// Pixels differing by more than [`TOLERANCE`] on some channel.
    pub pixels: u32,
    /// Bounding box of every one of them: `(x, y, width, height)`. This is the
    /// region to look at — a whole-picture diff count says a picture changed,
    /// which is the one thing the reader already knew.
    pub region: (u32, u32, u32, u32),
    /// Worst per-channel difference found anywhere.
    pub worst: u8,
}

/// Compare `actual` against `reference`.
///
/// `Ok(None)` means every pixel is within [`TOLERANCE`]. A size mismatch is an
/// error rather than a difference: there is no region to report, and a picture
/// that changed shape is a different picture.
pub fn compare(actual: &RgbaImage, reference: &RgbaImage) -> Result<Option<Difference>, String> {
    if !actual.is_well_formed() {
        return Err(format!(
            "the picture is {}x{} but carries {} bytes",
            actual.width,
            actual.height,
            actual.pixels.len()
        ));
    }
    if actual.width != reference.width || actual.height != reference.height {
        return Err(format!(
            "the picture is {}x{} and the reference is {}x{}",
            actual.width, actual.height, reference.width, reference.height
        ));
    }
    let mut pixels = 0_u32;
    let mut worst = 0_u8;
    let (mut left, mut top) = (u32::MAX, u32::MAX);
    let (mut right, mut bottom) = (0_u32, 0_u32);
    for y in 0..actual.height {
        for x in 0..actual.width {
            let at = ((y * actual.width + x) * 4) as usize;
            let delta = (0..4)
                .map(|c| actual.pixels[at + c].abs_diff(reference.pixels[at + c]))
                .max()
                .unwrap_or(0);
            if delta <= TOLERANCE {
                continue;
            }
            pixels += 1;
            worst = worst.max(delta);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if pixels == 0 {
        return Ok(None);
    }
    Ok(Some(Difference {
        pixels,
        region: (left, top, right - left + 1, bottom - top + 1),
        worst,
    }))
}

/// Columns and rows of the map a failure prints.
const MAP_W: u32 = 32;
const MAP_H: u32 = 16;

/// Where the difference is, drawn over the whole picture as text.
///
/// A bounding box says how big the change is; this says what shape it has and
/// where. A shifted graticule is a full-width row, a moved trace is a blob, and
/// a repainted canvas is solid — three failures that read identically as a
/// pixel count. `#` is a cell holding at least one differing pixel.
pub fn diff_map(actual: &RgbaImage, reference: &RgbaImage) -> String {
    let mut cells = vec![false; (MAP_W * MAP_H) as usize];
    for y in 0..actual.height {
        for x in 0..actual.width {
            let at = ((y * actual.width + x) * 4) as usize;
            let delta = (0..4)
                .map(|c| actual.pixels[at + c].abs_diff(reference.pixels[at + c]))
                .max()
                .unwrap_or(0);
            if delta <= TOLERANCE {
                continue;
            }
            let column = (x * MAP_W / actual.width.max(1)).min(MAP_W - 1);
            let row = (y * MAP_H / actual.height.max(1)).min(MAP_H - 1);
            cells[(row * MAP_W + column) as usize] = true;
        }
    }
    let mut out = String::with_capacity(((MAP_W + 3) * MAP_H) as usize);
    for row in 0..MAP_H {
        out.push_str("  ");
        for column in 0..MAP_W {
            out.push(match cells[(row * MAP_W + column) as usize] {
                true => '#',
                false => '.',
            });
        }
        out.push('\n');
    }
    out
}

/// Where the committed references live.
pub fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("references")
}

/// Every reference name on disk, sorted. The suite reads this to prove no
/// reference is unclaimed.
pub fn reference_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(reference_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension()? == "png")
                .then(|| path.file_stem()?.to_str().map(str::to_string))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

/// Assert `actual` against the reference called `name`, or write it when
/// blessing.
///
/// The failure carries the region, the map, and how to record the new picture.
/// **Nothing is written on a failure.** A PNG pair beside the reference would
/// need somewhere to put it, and the two candidates are both wrong: the crate
/// (a red run leaves files in the tree) and the system temp root (owned by the
/// scratch pool, and only by it — `test_support::tests::the_pool_is_the_only_temp_owner`
/// refuses a second caller, because that is how this machine's disk filled three
/// times). The map is readable where the failure is read, which is a terminal.
pub fn check(name: &str, actual: &RgbaImage) -> Result<(), String> {
    let path = reference_dir().join(format!("{name}.png"));
    if std::env::var_os(BLESS).is_some() {
        return write(&path, actual);
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return Err(format!(
            "no reference for '{name}' at {}. If this picture is correct, record it with \
             `{BLESS}=1 cargo test -p aria-ui --lib snapshot_diff`",
            path.display()
        ));
    };
    let reference = aria_render::decode_png(&bytes).map_err(|err| {
        format!(
            "the reference at {} is not a picture: {err}",
            path.display()
        )
    })?;
    let Some(difference) = compare(actual, &reference)? else {
        return Ok(());
    };
    let (x, y, w, h) = difference.region;
    Err(format!(
        "'{name}' changed: {} pixel(s) differ by up to {} in {w}x{h} at ({x}, {y}) of a \
         {}x{} picture. Reference: {}\n{}\nIf the new picture is right, record it with \
         `{BLESS}=1 cargo test -p aria-ui --lib snapshot_diff`.",
        difference.pixels,
        difference.worst,
        actual.width,
        actual.height,
        path.display(),
        diff_map(actual, &reference)
    ))
}

fn write(path: &std::path::Path, image: &RgbaImage) -> Result<(), String> {
    let bytes = aria_render::encode_png(image).map_err(|err| err.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
#[path = "visual_diff_tests.rs"]
mod tests;
