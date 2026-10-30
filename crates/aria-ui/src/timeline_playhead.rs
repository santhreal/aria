//! The timeline's playhead: the head on the ruler, the line across each lane
//! group, and the timecode over the outline column.
//!
//! A playback vsync or a ruler scrub moves the playhead and nothing else the
//! timeline draws. While the timeline body is built it records where each part
//! of the playhead goes ([`Layer`]), and the shell paints the playhead over the
//! body from that record on every frame it draws ([`layer`]). A move of the
//! playhead alone then redraws the shell and replays the body's last drawing
//! ([`Layer::holds`]).
//!
//! In the states listed in [`INLINE_WHEN`], the body draws something that
//! reads the playhead or covers its line, so the body draws the head and the
//! lines itself, in the order it draws the rest, and a playhead move builds
//! the body again. The timecode at rest is painted by the layer in every
//! state; the body lays out an invisible copy of it, which holds the field's
//! size.
//!
//! Both paths paint through [`paint_ruler_mark`] and [`paint_lane_line`].

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Bounds, ContentMask, Div, EdgeFade, Edges, Hsla, IntoElement, ParentElement as _, Pixels,
    SharedString, Styled as _, TextStyle, Window, canvas, div, fill, point, px, size,
};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::theme::Theme;
use crate::transport::ticks_to_px_f;

/// Width and height of the playhead's head on the ruler.
const HEAD_W: f32 = 11.0;
const HEAD_H: f32 = 8.0;
/// How far into the head the line stands.
const LINE_AT: f32 = 5.0;
/// Width of the line down the ruler.
const RULER_LINE_W: f32 = 1.0;
/// Width of the line across a lane group.
const LANE_LINE_W: f32 = 1.5;
/// How far past either edge of a lane group the line is still painted, so a
/// playhead on the first or last pixel keeps its full width.
const LANE_SLACK: f32 = 2.0;

/// The states in which the timeline body draws the playhead's head and lines
/// itself, each with the reason: what the body draws then reads the playhead,
/// or covers its line.
pub(crate) const INLINE_WHEN: [(&str, fn(&AppState) -> bool); 5] = [
    // A property row draws its value and keyframe state at the playhead.
    ("property rows", |app| !app.expanded_clips.is_empty()),
    // The Graph Editor draws its own playhead line.
    ("graph editor", |app| app.timeline_graph_open()),
    // The trim readout is drawn over the lane line.
    ("trim", |app| app.trim_feedback.is_some()),
    // The snap guide is drawn over the ruler head.
    ("snap guide", |app| app.snap_guide.is_some()),
    // The empty-timeline prompt is drawn over the lanes.
    ("empty timeline", |app| {
        app.store
            .sequence
            .tracks
            .iter()
            .all(|track| track.clips.is_empty())
    }),
];

#[cfg(test)]
thread_local! {
    /// Draws the playhead and the timecode inline in every state, so a test
    /// can compare the layer's drawing with the body's.
    pub(crate) static FORCE_INLINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The head and the line down the ruler painted last on this thread, in
    /// window pixels.
    static MARK: std::cell::Cell<Option<(Bounds<Pixels>, Bounds<Pixels>)>> = const { std::cell::Cell::new(None) };
    /// The lines painted across lane groups on this thread since the last
    /// [`take_lane_lines`], in window pixels.
    static LANES: RefCell<Vec<Bounds<Pixels>>> = const { RefCell::new(Vec::new()) };
}

/// The head and the line down the ruler painted last on this thread, in
/// window pixels.
#[cfg(test)]
pub(crate) fn painted_mark() -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
    MARK.get()
}

/// Every line painted across a lane group on this thread since the last
/// call, in paint order.
#[cfg(test)]
pub(crate) fn take_lane_lines() -> Vec<Bounds<Pixels>> {
    LANES.with(|lanes| std::mem::take(&mut *lanes.borrow_mut()))
}

/// Whether the layer paints the playhead's head and lines for the state as
/// it stands: no state in [`INLINE_WHEN`] holds.
pub(crate) fn layered(app: &AppState) -> bool {
    #[cfg(test)]
    if FORCE_INLINE.get() {
        return false;
    }
    !INLINE_WHEN.iter().any(|(_, inline)| inline(app))
}

/// Whether the layer paints the timecode at rest over the invisible copy the
/// body lays out. It does outside a test that forces the body to draw it.
pub(crate) fn layers_timecode() -> bool {
    #[cfg(test)]
    if FORCE_INLINE.get() {
        return false;
    }
    true
}

/// The scale and scroll the timeline body was built at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct View {
    pub pps: f32,
    pub fps: f32,
    pub scroll_x: f32,
}

impl View {
    /// `ticks` as px from the left edge of the time area.
    fn x(self, ticks: f32) -> f32 {
        ticks_to_px_f(ticks, self.fps, self.pps) - self.scroll_x
    }
}

/// Window bounds of a part of the body, and the content mask it was drawn in.
#[derive(Clone, Copy)]
struct Slot {
    bounds: Bounds<Pixels>,
    mask: ContentMask<Pixels>,
}

impl Slot {
    fn at(bounds: Bounds<Pixels>, window: &Window) -> Self {
        Self {
            bounds,
            mask: window.content_mask(),
        }
    }
}

/// The ruler's slot, the depth of its edge fades, and whether content
/// continues past its left and right edges, which fades them.
struct RulerSlot {
    slot: Slot,
    fade_depth: f32,
    fade: (bool, bool),
}

/// The timecode field's text slot, the style its text is set in, and the text
/// the body laid out.
struct Timecode {
    slot: Slot,
    style: TextStyle,
    text: SharedString,
}

/// What the last build of the timeline body recorded.
#[derive(Default)]
struct Drawn {
    /// Whether the layer paints the head and the lines.
    lines: bool,
    view: View,
    ruler: Option<RulerSlot>,
    /// The video lane group's time area, then the audio group's.
    lanes: [Option<Slot>; 2],
    /// `None` while the field is open for typing.
    timecode: Option<Timecode>,
}

/// The record the timeline body writes while it is built and the shell's
/// layer paints from.
#[derive(Clone, Default)]
pub(crate) struct Layer(Rc<RefCell<Drawn>>);

impl Layer {
    /// Starts the record of a build of the body at `view`. `lines` is whether
    /// the layer paints the head and the lines.
    pub(crate) fn begin(&self, lines: bool, view: View) {
        *self.0.borrow_mut() = Drawn {
            lines,
            view,
            ..Drawn::default()
        };
    }

    /// Whether the body's last drawing is still right after a move of the
    /// playhead alone: the layer paints the head and the lines, no state in
    /// [`INLINE_WHEN`] holds, and the timecode is in the state the body laid
    /// it out in, at rest with the same figures or open.
    pub(crate) fn holds(&self, app: &AppState) -> bool {
        let drawn = self.0.borrow();
        drawn.lines
            && layered(app)
            && match (&drawn.timecode, app.clock_field(ClockSite::Timeline)) {
                (Some(timecode), (false, clock)) => same_figures(&timecode.text, &clock),
                (None, (open, _)) => open,
                (Some(_), (true, _)) => false,
            }
    }

    /// The ruler's mark: a box over the ruler body that records the body's
    /// bounds, mask and fade, and paints the head at `x` px from the ruler's
    /// left edge when the body draws the playhead.
    pub(crate) fn ruler_mark(
        &self,
        x: Option<f32>,
        fade_depth: f32,
        fade: (bool, bool),
        color: Hsla,
    ) -> impl IntoElement + use<> {
        let drawn = self.0.clone();
        canvas(
            move |bounds, window, _| {
                drawn.borrow_mut().ruler = Some(RulerSlot {
                    slot: Slot::at(bounds, window),
                    fade_depth,
                    fade,
                });
            },
            move |bounds, (), window, _| {
                if let Some(x) = x {
                    paint_ruler_mark(window, bounds, x, color);
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Records the time area of a lane group, from its canvas's prepaint.
    pub(crate) fn record_lanes(&self, audio: bool, bounds: Bounds<Pixels>, window: &Window) {
        self.0.borrow_mut().lanes[usize::from(audio)] = Some(Slot::at(bounds, window));
    }

    /// The window bounds of a lane group's time area as the last build of the
    /// body recorded them: the video group's, or the audio group's when
    /// `audio`.
    pub(crate) fn lane_area(&self, audio: bool) -> Option<Bounds<Pixels>> {
        self.0.borrow().lanes[usize::from(audio)].map(|slot| slot.bounds)
    }

    /// The timecode at rest: laid out at the size of `text` and invisible,
    /// with a box over it that records its bounds, mask and text style for the
    /// layer to paint the current timecode in.
    pub(crate) fn timecode(&self, text: SharedString) -> Div {
        let drawn = self.0.clone();
        let laid_out = text.clone();
        div()
            .relative()
            .min_w_0()
            .truncate()
            .invisible()
            .child(text)
            .child(
                canvas(
                    move |bounds, window, _| {
                        drawn.borrow_mut().timecode = Some(Timecode {
                            slot: Slot::at(bounds, window),
                            style: window.text_style(),
                            text: laid_out,
                        });
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

/// Whether `a` and `b` set the same figures: the same length, and the same
/// byte at every position that is not a digit in both. In tabular figures
/// such strings are the same width.
fn same_figures(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(a, b)| a == b || (a.is_ascii_digit() && b.is_ascii_digit()))
}

/// The shell's layer over the timeline frame's body: the head and the lines at
/// the playhead's tick while [`layered`] held at the body's last build, and the
/// timecode at rest.
pub(crate) fn layer(app: &AppState, theme: &Theme) -> impl IntoElement + use<> {
    let drawn = app.timeline_playhead.0.clone();
    let at = app.store.playhead_ticks as f32;
    let clock = match app.clock_field(ClockSite::Timeline) {
        (false, clock) => Some(clock),
        (true, _) => None,
    };
    let color = theme.playhead;
    canvas(
        |_, _, _| {},
        move |_, (), window, cx| {
            let drawn = drawn.borrow();
            if drawn.lines {
                let x = drawn.view.x(at);
                if let Some(ruler) = &drawn.ruler {
                    let bounds = ruler.slot.bounds;
                    let fade = ruler_fade(bounds, ruler.fade_depth, ruler.fade, x);
                    window.with_content_mask(Some(ruler.slot.mask), |window| {
                        crate::edge_fade::paint_in(window, fade, |window| {
                            paint_ruler_mark(window, bounds, x, color);
                        });
                    });
                }
                for lanes in drawn.lanes.iter().flatten() {
                    window.with_content_mask(Some(lanes.mask), |window| {
                        window.with_content_mask(Some(ContentMask::from(lanes.bounds)), |window| {
                            paint_lane_line(window, lanes.bounds, x, color);
                        });
                    });
                }
            }
            if let (Some(timecode), Some(clock)) = (&drawn.timecode, clock) {
                paint_timecode(window, cx, timecode, clock);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The ruler's fade, `depth` px deep, over a head at `x`: the faded edges the
/// head reaches into. `None` when it reaches into neither, since a fade costs a
/// layer mask.
fn ruler_fade(
    bounds: Bounds<Pixels>,
    depth: f32,
    (left, right): (bool, bool),
    x: f32,
) -> Option<EdgeFade> {
    let width = f32::from(bounds.size.width);
    let left = left && x - LINE_AT < depth;
    let right = right && x - LINE_AT + HEAD_W > width - depth;
    (left || right).then(|| {
        crate::edge_fade::fade(
            bounds,
            depth,
            Edges {
                top: false,
                bottom: false,
                left,
                right,
            },
        )
    })
}

/// Paints the playhead's head and its line down the ruler at `x` px from the
/// ruler's left edge.
pub(crate) fn paint_ruler_mark(window: &mut Window, bounds: Bounds<Pixels>, x: f32, color: Hsla) {
    let left = bounds.origin.x + px(x);
    let top = bounds.origin.y;
    let head = Bounds {
        origin: point(left - px(LINE_AT), top),
        size: size(px(HEAD_W), px(HEAD_H)),
    };
    let line = Bounds {
        origin: point(left, top),
        size: size(px(RULER_LINE_W), bounds.size.height),
    };
    window.paint_quad(fill(head, color));
    window.paint_quad(fill(line, color));
    #[cfg(test)]
    MARK.set(Some((head, line)));
}

/// Paints the playhead's line across a lane group's time area at `x` px from
/// its left edge, unless the line is off the area. Whether it was painted.
pub(crate) fn paint_lane_line(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    x: f32,
    color: Hsla,
) -> bool {
    let width = f32::from(bounds.size.width).max(1.0);
    if x < -LANE_SLACK || x > width + LANE_SLACK {
        return false;
    }
    let height = f32::from(bounds.size.height).max(1.0);
    let line = Bounds {
        origin: point(px(f32::from(bounds.origin.x) + x), bounds.origin.y),
        size: size(px(LANE_LINE_W), px(height)),
    };
    window.paint_quad(fill(line, color));
    #[cfg(test)]
    LANES.with(|lanes| lanes.borrow_mut().push(line));
    true
}

/// Paints `clock` in the timecode slot, set the way a text element sets it.
fn paint_timecode(window: &mut Window, cx: &mut App, timecode: &Timecode, clock: SharedString) {
    let style = &timecode.style;
    let rem = window.rem_size();
    let font_size = style.font_size.to_pixels(rem);
    let line_height = window.pixel_snap(style.line_height.to_pixels(font_size.into(), rem));
    let runs = [style.to_run(clock.len())];
    let lines =
        match window
            .text_system()
            .shape_text(clock, font_size, &runs, None, style.line_clamp)
        {
            Ok(lines) => lines,
            Err(err) => {
                tracing::warn!("the timeline timecode could not be shaped: {err:#}");
                return;
            }
        };
    let bounds = timecode.slot.bounds;
    window.with_content_mask(Some(timecode.slot.mask), |window| {
        let mut origin = bounds.origin;
        for line in &lines {
            let painted = line
                .paint_background(
                    origin,
                    line_height,
                    style.text_align,
                    Some(bounds),
                    window,
                    cx,
                )
                .and_then(|()| {
                    line.paint(
                        origin,
                        line_height,
                        style.text_align,
                        Some(bounds),
                        window,
                        cx,
                    )
                });
            if let Err(err) = painted {
                tracing::warn!("the timeline timecode could not be painted: {err:#}");
            }
            origin.y += line.size(line_height).height;
        }
    });
}

#[cfg(test)]
mod tests {
    //! A timecode that keeps its figures keeps its width in tabular figures,
    //! so the field the body laid out still holds it. Fails when a change of
    //! length or of a separator reads as the same figures, or a change of a
    //! digit as different ones.

    #[test]
    fn only_a_change_of_digits_keeps_the_figures() {
        for (a, b, same) in [
            ("00:00:01:02", "00:00:01:02", true),
            ("00:00:01:02", "00:00:01:03", true),
            ("00:00:59:29", "00:01:00:00", true),
            ("00:00:01:02", "00:00:01;02", false),
            ("9+15", "10+00", false),
            ("1234", "123a", false),
            ("00:00:01:02", "", false),
        ] {
            assert_eq!(super::same_figures(a, b), same, "{a:?} and {b:?}");
        }
    }
}
