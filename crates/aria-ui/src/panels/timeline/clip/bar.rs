//! The parts of a clip bar, top to bottom: the name strip, the content strip,
//! and the line one property of the clip is drawn as.
//!
//! Paint and hit testing both place the parts through [`BarRows`], so a press
//! lands on what was drawn under it.

use gpui::{
    App, Bounds, ContentMask, Corners, Edges, Hsla, PathBuilder, Pixels, SharedString, TextAlign,
    TextRun, Window, point, px, quad, size,
};

use crate::panels::timeline::ticks_to_px;
use crate::theme::Theme;
use crate::transport::ticks_to_px_f;
use crate::types::{UiClip, UiClipLine, UiFxBadge, UiGenerator, UiTransition};

/// Height of the name strip at the top of every bar.
pub(crate) const NAME_STRIP: f32 = 14.0;
/// Pointer distance, in pixels, within which a press lands on a clip's line
/// or on one of its keyframe dots.
pub(crate) const LINE_REACH: f32 = 3.0;
/// Lanes at least this tall draw keyframe dots on a clip's line: the Medium
/// height, which is the default.
pub(crate) const DOTS_FROM: f32 = aria_models::TrackDisplay::DEFAULT_HEIGHT as f32;
/// Side of a keyframe dot.
const DOT: f32 = 5.0;
/// Size of the fx badge.
const FX_W: f32 = 16.0;
const FX_H: f32 = 10.0;
/// Space between the items of the name strip.
const GAP: f32 = 4.0;
const NAME_PX: f32 = 11.0;
const BADGE_PX: f32 = 9.0;

/// Where the parts of a bar sit in its lane, as offsets from the lane's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BarRows {
    /// Top of the bar.
    pub top: f32,
    /// Height of the bar.
    pub height: f32,
    /// Where the line draws the top of its scale.
    pub line_top: f32,
    /// Where the line draws the bottom of its scale.
    pub line_bottom: f32,
}

impl BarRows {
    /// The parts of a bar in a lane `lane_h` tall. A lane shorter than Small
    /// has room for the name strip only.
    pub(crate) fn of(lane_h: f32) -> Self {
        let inset = if lane_h < 32.0 { 2.0 } else { 4.0 };
        let height = (lane_h - 2.0 * inset).max(NAME_STRIP);
        Self {
            top: inset,
            height,
            line_top: inset + NAME_STRIP + 2.0,
            line_bottom: inset + height - 2.0,
        }
    }

    /// Whether the bar has room below its name strip for the line.
    pub(crate) fn has_line(self) -> bool {
        self.band() >= 4.0
    }

    /// Height of the band the line moves in.
    pub(crate) fn band(self) -> f32 {
        self.line_bottom - self.line_top
    }

    /// The offset from the lane's top at which the line draws `fraction` of
    /// its scale.
    pub(crate) fn line_y(self, fraction: f32) -> f32 {
        self.line_bottom - self.band() * fraction.clamp(0.0, 1.0)
    }
}

/// The timeline ticks a transition at the right junction of `clip` covers.
pub(crate) fn transition_span(clip: &UiClip, transition: &UiTransition) -> (i64, i64) {
    transition.window(clip.timeline_end_ticks)
}

/// What a point on a bar's line band lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineHit {
    /// The keyframe at this timeline tick.
    Key(i64),
    /// The line between keyframes.
    Line,
}

/// Hit-test the line of `clip`, in a lane `lane_h` tall whose top is at
/// `lane_top`, at the local point `(x, y)`. `bar_x` is the bar's left edge in
/// the same space. Only a line a drag can write answers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn line_hit(
    clip: &UiClip,
    lane_top: f32,
    lane_h: f32,
    bar_x: f32,
    x: f32,
    y: f32,
    fps: f32,
    pps: f32,
) -> Option<LineHit> {
    let line = clip.line.as_ref().filter(|line| line.draggable)?;
    let rows = BarRows::of(lane_h);
    if !rows.has_line() {
        return None;
    }
    let tick_at = |at: f32| {
        clip.timeline_start_ticks as f64
            + f64::from(at - bar_x) * f64::from(fps.max(1.0)) / f64::from(pps.max(f32::EPSILON))
    };
    if lane_h >= DOTS_FROM {
        let mut nearest: Option<(f32, i64)> = None;
        for &(tick, value) in line.keys.iter() {
            if tick < clip.timeline_start_ticks || tick > clip.timeline_end_ticks {
                continue;
            }
            let kx = bar_x + ticks_to_px(tick - clip.timeline_start_ticks, fps, pps);
            let ky = lane_top + rows.line_y(line.scale.fraction(value));
            let reach = (DOT * 0.5).max(LINE_REACH) + 1.0;
            let (dx, dy) = ((x - kx).abs(), (y - ky).abs());
            if dx <= reach && dy <= reach && nearest.is_none_or(|(best, _)| dx < best) {
                nearest = Some((dx, tick));
            }
        }
        if let Some((_, tick)) = nearest {
            return Some(LineHit::Key(tick));
        }
    }
    let ly = lane_top + rows.line_y(line.fraction_at(tick_at(x)));
    ((y - ly).abs() <= LINE_REACH).then_some(LineHit::Line)
}

/// Shape `text` in the theme's sans face.
fn shape(
    window: &mut Window,
    text: SharedString,
    font_px: f32,
    color: Hsla,
    theme: &Theme,
) -> gpui::ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: theme.ui_font(gpui::FontWeight::NORMAL),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        tracking: Default::default(),
    };
    window
        .text_system()
        .shape_line(text, px(font_px), &[run], None)
}

fn paint_shaped(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    line: &gpui::ShapedLine,
    x: f32,
    y: f32,
    height: f32,
) {
    let _ = line.paint(
        point(px(x), px(y)),
        px(height),
        TextAlign::Left,
        None,
        window,
        cx,
    );
    *paint_calls += 1;
}

fn rounded(
    window: &mut Window,
    paint_calls: &mut usize,
    bounds: Bounds<Pixels>,
    radius: f32,
    fill: Hsla,
) {
    window.paint_quad(quad(
        bounds,
        Corners::all(px(radius)),
        fill,
        Edges::default(),
        gpui::transparent_black(),
        gpui::BorderStyle::default(),
    ));
    *paint_calls += 1;
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(x), px(y)),
        size: size(px(w.max(0.0)), px(h.max(0.0))),
    }
}

/// The fill of the fx badge: grey with nothing applied or changed, yellow
/// with effects applied, purple with only intrinsic properties changed.
pub(crate) fn fx_fill(theme: &Theme, fx: UiFxBadge) -> Hsla {
    match (fx.effects, fx.intrinsic) {
        (false, false) => theme.text_muted,
        (true, _) => theme.marker(aria_models::MarkerColor::Yellow),
        (false, true) => theme.marker(aria_models::MarkerColor::Purple),
    }
}

/// The strip's geometry, left to right: where the name starts, and the
/// right edge nothing in the strip draws past.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Strip {
    pub left: f32,
    pub right: f32,
}

impl Strip {
    /// The strip of a bar spanning `[x, x + w)` in a view starting at
    /// `view_left`. The name keeps to the view's left edge while the bar
    /// starts off it, and clears the fade grips when they are drawn.
    pub(crate) fn of(x: f32, w: f32, view_left: f32, grips: bool) -> Self {
        let grip = if grips {
            super::TRIM + super::FADE_GRIP + 2.0
        } else {
            GAP
        };
        Self {
            left: (x + grip).max(view_left + GAP),
            right: x + w - grip,
        }
    }
}

/// What the name strip painted where a press can land: the fx badge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct StripHits {
    pub fx: Option<Bounds<Pixels>>,
}

/// Paint the name strip of `clip` at the top `top` of a bar: the clip-type
/// icon, the name, the speed when it is not 100 %, then the badges (the link
/// tags, the fx badge, a drifted link's offset), and a missing plugin's badge
/// at the right end. Returns where the fx badge was drawn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_name_strip(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    clip: &UiClip,
    ink: Hsla,
    alpha: f32,
    strip: Strip,
    top: f32,
) -> StripHits {
    let mut hits = StripHits { fx: None };
    let missing = clip
        .missing_plugin
        .as_ref()
        .or_else(|| clip.clip_type.as_ref().and_then(|t| t.missing.as_ref()));
    let mut right = strip.right;
    if let Some(missing) = missing {
        let room = right - strip.left;
        let text = if room >= 160.0 {
            missing.clone()
        } else {
            SharedString::new_static("!")
        };
        let shaped = shape(window, text, BADGE_PX + 1.0, theme.warning, theme);
        let badge_w = (f32::from(shaped.width()) + 8.0).min(room.max(14.0));
        let badge_x = (right - badge_w).max(strip.left);
        window.paint_quad(quad(
            rect(badge_x, top + 1.0, badge_w, NAME_STRIP - 2.0),
            Corners::all(px(3.0)),
            theme.warning.opacity(0.22),
            Edges::all(px(1.0)),
            theme.warning,
            gpui::BorderStyle::default(),
        ));
        *paint_calls += 1;
        paint_shaped(
            window,
            cx,
            paint_calls,
            &shaped,
            badge_x + 4.0,
            top,
            NAME_STRIP,
        );
        right = badge_x - GAP;
    }
    if right - strip.left < 8.0 {
        return hits;
    }
    let ink = ink.opacity(alpha);
    let mask = ContentMask::from(rect(strip.left, top, right - strip.left, NAME_STRIP));
    window.with_content_mask(Some(mask), |window| {
        let mut x = strip.left;
        if let Some(path) = clip.clip_type.as_ref().and_then(|t| t.icon) {
            let _ = crate::icons::icon(path);
            let _ = window.paint_svg(
                rect(x, top + 1.0, 12.0, 12.0),
                SharedString::from(path),
                None,
                window.transformation(),
                ink,
                cx,
            );
            *paint_calls += 1;
            x += 12.0 + GAP - 1.0;
        }
        // The badges are shaped first so the name gives up its room to them
        // rather than pushing them off the bar.
        let speed = clip
            .speed_label
            .clone()
            .map(|text| shape(window, text, NAME_PX, ink, theme));
        let tags = match (clip.linked_to.is_some(), clip.grouped_to.is_some()) {
            (false, false) => None,
            (true, false) => Some("[L]"),
            (false, true) => Some("[G]"),
            (true, true) => Some("[L] [G]"),
        }
        .map(|tags| shape(window, SharedString::new_static(tags), NAME_PX, ink, theme));
        let drift_fill = theme.danger;
        let drift = clip.drift.map(|frames| {
            shape(
                window,
                format!("{frames:+}").into(),
                BADGE_PX + 1.0,
                Theme::ink_on(drift_fill),
                theme,
            )
        });
        let width = |line: &Option<gpui::ShapedLine>| {
            line.as_ref()
                .map_or(0.0, |line| f32::from(line.width()) + GAP)
        };
        let tail = width(&speed)
            + width(&tags)
            + FX_W
            + GAP
            + drift
                .as_ref()
                .map_or(0.0, |line| f32::from(line.width()) + 6.0 + GAP);
        let name = shape(window, clip.name.clone(), NAME_PX, ink, theme);
        let name_room = (right - x - tail).max(0.0);
        let name_w = f32::from(name.width()).min(name_room);
        if name_room >= 8.0 {
            window.with_content_mask(
                Some(ContentMask::from(rect(x, top, name_room, NAME_STRIP))),
                |window| paint_shaped(window, cx, paint_calls, &name, x, top, NAME_STRIP),
            );
            x += name_w + GAP;
        }
        for line in [&speed, &tags].into_iter().flatten() {
            paint_shaped(window, cx, paint_calls, line, x, top, NAME_STRIP);
            x += f32::from(line.width()) + GAP;
        }
        let fx = rect(x, top + (NAME_STRIP - FX_H) * 0.5, FX_W, FX_H);
        let fill = fx_fill(theme, clip.fx).opacity(alpha);
        rounded(window, paint_calls, fx, 2.0, fill);
        if clip.fx.effects && clip.fx.intrinsic {
            let purple = theme
                .marker(aria_models::MarkerColor::Purple)
                .opacity(alpha);
            rounded(
                window,
                paint_calls,
                rect(x, top + (NAME_STRIP + FX_H) * 0.5, FX_W, 2.0),
                0.0,
                purple,
            );
        }
        let glyph = shape(
            window,
            SharedString::new_static("fx"),
            BADGE_PX,
            Theme::ink_on(fill),
            theme,
        );
        let glyph_x = x + (FX_W - f32::from(glyph.width())) * 0.5;
        paint_shaped(window, cx, paint_calls, &glyph, glyph_x, top, NAME_STRIP);
        hits.fx = Some(fx);
        x += FX_W + GAP;
        if let Some(line) = drift {
            let badge_w = f32::from(line.width()) + 6.0;
            rounded(
                window,
                paint_calls,
                rect(x, top + 1.0, badge_w, NAME_STRIP - 2.0),
                3.0,
                drift_fill.opacity(alpha),
            );
            paint_shaped(window, cx, paint_calls, &line, x + 3.0, top, NAME_STRIP);
        }
    });
    hits
}

/// Paint the "Media Offline" notice and the file name in the content strip
/// of an offline bar, as many lines as fit under the name strip.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_offline(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    file: &SharedString,
    ink: Hsla,
    strip: Strip,
    top: f32,
    bottom: f32,
) {
    const LINE: f32 = 12.0;
    let room = bottom - top;
    if room < LINE || strip.right - strip.left < 8.0 {
        return;
    }
    let mask = ContentMask::from(rect(strip.left, top, strip.right - strip.left, room));
    window.with_content_mask(Some(mask), |window| {
        let notice = shape(
            window,
            SharedString::new_static("Media Offline"),
            NAME_PX - 1.0,
            ink,
            theme,
        );
        paint_shaped(window, cx, paint_calls, &notice, strip.left, top, LINE);
        if room >= LINE * 2.0 {
            let name = shape(window, file.clone(), NAME_PX - 1.0, ink.opacity(0.8), theme);
            paint_shaped(window, cx, paint_calls, &name, strip.left, top + LINE, LINE);
        }
    });
}

/// Paint a generated clip's content strip: the swatch of the colour it fills
/// with, when it has one, then the generator's name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_generator(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    generator: &UiGenerator,
    ink: Hsla,
    strip: Strip,
    top: f32,
    bottom: f32,
) {
    const LINE: f32 = 12.0;
    let room = bottom - top;
    if room < 6.0 || strip.right - strip.left < 8.0 {
        return;
    }
    let mask = ContentMask::from(rect(strip.left, top, strip.right - strip.left, room));
    window.with_content_mask(Some(mask), |window| {
        let mut x = strip.left;
        let side = (room - 2.0).min(LINE);
        if let Some([r, g, b, a]) = generator.swatch {
            let fill: Hsla = gpui::rgba(u32::from_be_bytes([r, g, b, a])).into();
            window.paint_quad(quad(
                rect(x, top + (room - side) * 0.5, side, side),
                Corners::all(px(2.0)),
                fill.opacity(ink.a),
                Edges::all(px(1.0)),
                ink.opacity(0.5 * ink.a),
                gpui::BorderStyle::default(),
            ));
            *paint_calls += 1;
            x += side + GAP;
        }
        if room >= LINE && !generator.label.is_empty() {
            let name = shape(window, generator.label.clone(), NAME_PX - 1.0, ink, theme);
            paint_shaped(
                window,
                cx,
                paint_calls,
                &name,
                x,
                top + (room - LINE) * 0.5,
                LINE,
            );
        }
    });
}

/// One copy of a filmstrip cell's picture, in pixels from the bar's left
/// edge: the picture's left edge, a full cell wide, and the part of it shown,
/// `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tile {
    pub image: f32,
    pub from: f32,
    pub to: f32,
}

/// The tiles of a cell standing for `[from, to)` of a bar `bar_w` wide,
/// inside the visible range `view`, all in pixels from the bar's left edge.
/// The picture keeps its width and is cropped where the cell, the bar, or the
/// view ends; a cell wider than its picture repeats the picture across itself
/// and draws nothing outside it. Each tile ends where the next one starts.
pub(crate) fn film_tiles(
    from: f32,
    to: f32,
    bar_w: f32,
    view: (f32, f32),
) -> impl Iterator<Item = Tile> {
    let cell_w = crate::panels::timeline::CELL_W;
    let lo = from.max(view.0).max(0.0);
    let hi = to.min(bar_w).min(view.1);
    let image = move |copy: u32| from + copy as f32 * cell_w;
    let first = ((lo - from) / cell_w).floor() as u32;
    let mut last = first;
    if hi > lo {
        last = (((hi - from) / cell_w).ceil() as u32).max(first + 1);
        // A quotient rounded past a whole number would add a copy that
        // starts where the cell ends.
        if last > first + 1 && image(last - 1) >= hi {
            last -= 1;
        }
    }
    (first..last).map(move |copy| Tile {
        image: image(copy),
        from: if copy == first { lo } else { image(copy) },
        to: if copy + 1 == last {
            hi
        } else {
            image(copy + 1)
        },
    })
}

/// Paint the filmstrip `cells` of a bar spanning `[x, x + w)`, `top` to
/// `top + height`, inside the visible range `view`, dimmed for a disabled
/// clip.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_filmstrip(
    window: &mut Window,
    paint_calls: &mut usize,
    theme: &Theme,
    cells: &[super::FilmCell],
    x: f32,
    w: f32,
    view: (f32, f32),
    top: f32,
    height: f32,
    enabled: bool,
) {
    let cell_w = crate::panels::timeline::CELL_W;
    for cell in cells {
        for tile in film_tiles(cell.from, cell.to, w, (view.0 - x, view.1 - x)) {
            let shown = rect(x + tile.from, top, tile.to - tile.from, height);
            let _ = window.paint_image(
                shown,
                rect(x + tile.image, top, cell_w, height),
                Corners::default(),
                cell.image.clone(),
                0,
                false,
            );
            *paint_calls += 1;
            if !enabled {
                super::paint_solid(
                    window,
                    paint_calls,
                    x + tile.from,
                    top,
                    tile.to - tile.from,
                    height,
                    theme.bg.opacity(0.6),
                );
            }
        }
    }
}

/// Paint the bar of a transition spanning `[x0, x1)` over a cut, `top` to
/// `top + height`, with its name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_transition(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    name: &SharedString,
    x0: f32,
    x1: f32,
    top: f32,
    height: f32,
) {
    let w = (x1 - x0).max(4.0);
    let bounds = rect(x0, top, w, height);
    window.paint_quad(quad(
        bounds,
        Corners::all(px(3.0)),
        theme.surface_raised.opacity(0.92),
        Edges::all(px(1.0)),
        theme.accent,
        gpui::BorderStyle::default(),
    ));
    *paint_calls += 1;
    if w < 16.0 || name.is_empty() {
        return;
    }
    window.with_content_mask(
        Some(ContentMask::from(rect(
            x0 + GAP,
            top,
            w - 2.0 * GAP,
            height,
        ))),
        |window| {
            let shaped = shape(window, name.clone(), NAME_PX - 1.0, theme.text, theme);
            let text_w = f32::from(shaped.width());
            // The name sits in the middle of a bar wide enough for it and
            // from the left edge of one that is not.
            let x = (x0 + (w - text_w) * 0.5).max(x0 + GAP);
            let line = NAME_STRIP.min(height);
            paint_shaped(
                window,
                cx,
                paint_calls,
                &shaped,
                x,
                top + (height - line) * 0.5,
                line,
            );
        },
    );
}

/// Paint `line` across the bar spanning `[x, x + w)` in a lane whose top is
/// at `lane_y`, with its keyframe dots when the lane is `dots` tall enough.
/// `view` is the visible horizontal range, so an animated line builds its
/// path from the points on screen only.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_line(
    window: &mut Window,
    paint_calls: &mut usize,
    clip: &UiClip,
    line: &UiClipLine,
    rows: BarRows,
    lane_y: f32,
    x: f32,
    w: f32,
    view: (f32, f32),
    fps: f32,
    pps: f32,
    color: Hsla,
    dots: bool,
) {
    let start = clip.timeline_start_ticks as f32;
    let to_x = |tick: f64| x + ticks_to_px_f(tick as f32 - start, fps, pps);
    let to_y = |value: f32| lane_y + rows.line_y(line.scale.fraction(value));
    let (left, right) = (view.0.max(x), view.1.min(x + w));
    if right <= left {
        return;
    }
    if line.keys.is_empty() {
        let y = to_y(line.points.first().map_or(0.0, |&(_, value)| value));
        window.paint_quad(gpui::fill(rect(left, y - 0.5, right - left, 1.0), color));
        *paint_calls += 1;
    } else {
        let mut path = PathBuilder::stroke(px(1.0));
        let mut started = false;
        let points = &line.points;
        for (index, &(tick, value)) in points.iter().enumerate() {
            let px_x = to_x(tick);
            // A point is kept when it or a neighbour is on screen, so the
            // segments entering and leaving the view are drawn whole.
            let next_left = points
                .get(index + 1)
                .is_some_and(|&(next, _)| to_x(next) >= left);
            let prev_right = index
                .checked_sub(1)
                .and_then(|prev| points.get(prev))
                .is_some_and(|&(prev, _)| to_x(prev) <= right);
            if !(px_x >= left && px_x <= right
                || next_left && px_x < left
                || prev_right && px_x > right)
            {
                continue;
            }
            let at = point(px(px_x), px(to_y(value)));
            if started {
                path.line_to(at);
            } else {
                path.move_to(at);
                started = true;
            }
        }
        if started && let Ok(path) = path.build() {
            window.paint_path(path, color);
            *paint_calls += 1;
        }
    }
    if !dots {
        return;
    }
    for &(tick, value) in line.keys.iter() {
        if tick < clip.timeline_start_ticks || tick > clip.timeline_end_ticks {
            continue;
        }
        let kx = to_x(tick as f64);
        if kx < left - DOT || kx > right + DOT {
            continue;
        }
        rounded(
            window,
            paint_calls,
            rect(kx - DOT * 0.5, to_y(value) - DOT * 0.5, DOT, DOT),
            DOT * 0.5,
            color,
        );
    }
}

#[cfg(test)]
#[path = "bar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "strip_cover_tests.rs"]
mod strip_cover_tests;
