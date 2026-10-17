//! The canvas a timeline draws on: the size it sets, the size its aspect
//! preset implies, or the size of its largest video media.

use super::*;

/// Fallback canvas size when `Auto` aspect can't find any video media.
const DEFAULT_CANVAS: (u32, u32) = (1920, 1080);

/// Canvas pixel size for `project`: an explicit size wins; otherwise fixed
/// presets resolve to a 1080-baseline box on the shorter side, and `Auto`
/// follows the largest video media (falling back to 1920×1080 when there is
/// none).
pub fn canvas_size(project: &Project) -> (u32, u32) {
    sized_canvas(project.timeline().canvas(), || {
        project.largest_used_video_dimensions()
    })
}

/// [`canvas_size`] of `timeline`, the open one or a nested sequence's.
pub(super) fn timeline_canvas(project: &Project, timeline: &Timeline) -> (u32, u32) {
    sized_canvas(timeline.canvas(), || {
        project.largest_video_dimensions_on(timeline)
    })
}

/// The size `canvas` sets, else the size it derives from its aspect preset,
/// else from `largest`, the largest video media on its timeline.
fn sized_canvas(
    canvas: CanvasSettings,
    largest: impl FnOnce() -> Option<(u32, u32)>,
) -> (u32, u32) {
    match canvas.resolution {
        Some([w, h]) => (even(w), even(h)),
        None => derived_canvas(canvas, largest),
    }
}

/// [`canvas_size`] as if no explicit size were set: the shape the aspect preset
/// or the footage implies.
///
/// The delivery-size chooser scales this rather than the current size, so a
/// project that already carries an explicit size still offers the shape its
/// footage produced instead of scaling the size it was handed. Asking for it
/// directly is also the difference between a read and a copy of the whole
/// project: the caller used to clear the resolution on a clone.
pub fn derived_canvas_size(project: &Project) -> (u32, u32) {
    derived_canvas(project.timeline().canvas(), || {
        project.largest_used_video_dimensions()
    })
}

/// The size `canvas`'s aspect preset implies; for `Auto`, the largest video
/// media `largest` reads, or the default.
fn derived_canvas(
    canvas: CanvasSettings,
    largest: impl FnOnce() -> Option<(u32, u32)>,
) -> (u32, u32) {
    match canvas.aspect.pixels_for_baseline(REFERENCE_HEIGHT as u32) {
        Some([w, h]) => (w, h),
        None => largest().map_or(DEFAULT_CANVAS, |(width, height)| {
            (even(width), even(height))
        }),
    }
}

/// Round `v` down to the nearest even value (≥ 2): H.264 needs even dimensions.
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}
