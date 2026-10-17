//! The keyframe icon every time area paints: the timeline's property rows, its
//! graph editor, and the time areas beside Effect Controls' values.

use gpui::{Bounds, Corners, Edges, Hsla, Window, fill, point, px, quad, size};

use crate::app::property_rows::KeyframeIconShape;
use crate::theme::Theme;

/// The colours a keyframe icon is painted in, read off the theme once so a
/// canvas that paints many keys holds four colours rather than the theme.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyInk {
    fill: Hsla,
    selected_fill: Hsla,
    border: Hsla,
    selected_border: Hsla,
}

impl KeyInk {
    pub(crate) fn of(theme: &Theme) -> Self {
        Self {
            fill: theme.accent,
            selected_fill: theme.warning,
            border: theme.border_strong,
            selected_border: theme.text,
        }
    }
}

/// Paint a keyframe icon (`Linear` diamond, `Bezier` hourglass, `EaseIn` /
/// `EaseOut` half-hourglass, `Hold` square, or `Roving` circle) centered at
/// `(cx_px, cy_px)`.
pub(crate) fn paint_keyframe_icon(
    window: &mut Window,
    paint_calls: &mut usize,
    ink: KeyInk,
    cx_px: f32,
    cy_px: f32,
    shape: KeyframeIconShape,
    selected: bool,
) {
    let (fill_col, border_col) = if selected {
        (ink.selected_fill, ink.selected_border)
    } else {
        (ink.fill, ink.border)
    };
    match shape {
        KeyframeIconShape::Linear => {
            // 5-band diamond (1, 3, 5, 7, 5, 3, 1 px widths).
            let widths = [2.0f32, 4.0, 6.0, 8.0, 6.0, 4.0, 2.0];
            for (row, w) in widths.into_iter().enumerate() {
                let y = cy_px - 3.5 + row as f32;
                band(window, paint_calls, cx_px - w * 0.5, y, w, fill_col);
            }
        }
        KeyframeIconShape::Bezier => {
            // Hourglass: wide top and bottom, pinched waist at center.
            let widths = [8.0f32, 6.0, 4.0, 2.0, 4.0, 6.0, 8.0];
            for (row, w) in widths.into_iter().enumerate() {
                let y = cy_px - 3.5 + row as f32;
                band(window, paint_calls, cx_px - w * 0.5, y, w, fill_col);
            }
        }
        KeyframeIconShape::EaseIn | KeyframeIconShape::EaseOut => {
            // Asymmetric hourglass/diamond.
            let widths = [7.0f32, 5.0, 3.0, 6.0, 3.0, 5.0, 7.0];
            for (row, w) in widths.into_iter().enumerate() {
                let y = cy_px - 3.5 + row as f32;
                band(window, paint_calls, cx_px - w * 0.5, y, w, fill_col);
            }
        }
        KeyframeIconShape::Hold => {
            // Square keyframe icon.
            window.paint_quad(quad(
                Bounds {
                    origin: point(px(cx_px - 3.5), px(cy_px - 3.5)),
                    size: size(px(7.0), px(7.0)),
                },
                Corners::all(px(1.0)),
                fill_col,
                Edges::all(px(1.0)),
                border_col,
                gpui::BorderStyle::default(),
            ));
            *paint_calls += 1;
        }
        KeyframeIconShape::Roving => {
            // Circle keyframe icon.
            window.paint_quad(quad(
                Bounds {
                    origin: point(px(cx_px - 3.5), px(cy_px - 3.5)),
                    size: size(px(7.0), px(7.0)),
                },
                Corners::all(px(3.5)),
                fill_col,
                Edges::all(px(1.0)),
                border_col,
                gpui::BorderStyle::default(),
            ));
            *paint_calls += 1;
        }
    }
}

/// One 1 px tall band of a key's outline.
fn band(window: &mut Window, paint_calls: &mut usize, x: f32, y: f32, w: f32, color: Hsla) {
    window.paint_quad(fill(
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(1.0)),
        },
        color,
    ));
    *paint_calls += 1;
}
