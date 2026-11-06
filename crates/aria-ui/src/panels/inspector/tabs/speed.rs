//! Speed block: flat speed, the ramp presets, and the ramp curve editor.
//!
//! Presets come from `aria_models::speed_preset_catalog` at render time, and the
//! active one is resolved by shape rather than by a stored id, so a hand-edited
//! ramp shows as no preset. The whole block is hidden on a clip the model refuses
//! a ramp on.
//!
//! The dots are the ramp's real keyframes, so a point is drawn where it sits,
//! and the line between them is what the clip plays, easing included. The
//! rectangle the graph occupies is measured during paint (`gpui` canvas),
//! because that is the only place the drawn geometry is known, and the
//! pointer is read against that rectangle. Values run logarithmically so 100%
//! is near the middle instead of the bottom two percent.
//!
//! A drag is one edit: pointer moves update a draft on `AppState`, the release
//! commits, Esc abandons. The press lands on the graph; the moves and the
//! release reach the ramp through the window, so a drag follows the pointer
//! past the graph's edge. What each pointer event means is a row of
//! [`ramp_gesture`] rather than a rule inside a
//! listener, so the graph's behavior can be enumerated and pressed.

use aria_models::{Keyframe, Param, SPEED_CURVE_SCALE};
use gpui::{
    Bounds, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    PathBuilder, Pixels, SharedString, Styled, canvas, div, fill, point, px, size,
};

use crate::app::clip_speed::{
    RAMP_POINTS, RampDrag, ramp_tick_to_frac, ramp_value_to_frac, speed_presets,
};
use crate::app::param_units::Units;
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::surfaces::ramp_gesture::{self, Gesture};
use crate::theme::Theme;
use crate::timecode::format_ticks;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{
    Chip, Columns, Readout, chip_row, choice_chip, label_note, pick_button, under_labels,
    unkeyed_row,
};

/// Height of the curve graph. Tall enough for a log axis to read, short enough to
/// leave the rest of the tab on screen.
const GRAPH_HEIGHT: f32 = 120.0;

/// Side of a keyframe dot.
const DOT: f32 = 9.0;

/// Width of the curve's line.
const LINE: f32 = 1.5;

/// Opacity of the accent under the curve.
const AREA: f32 = 0.14;

/// How many even steps the curve is drawn through. A stretch between two
/// points bends with its easing and again on the log axis, so it is drawn as
/// a polyline; 60 is smooth at any inspector width.
const SAMPLES: usize = 60;

pub fn speed_block(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let read = state.read(cx);
    let active = read.selected_speed_preset();
    let points = read.selected_ramp_values();
    let has_ramp = read.selected_has_ramp();
    let curve = read.selected_ramp();
    let drag = read.ramp_dragging();
    let length = read.selected_ramp_length_ticks();

    let flat = if clip.speed_den == 0 {
        1.0
    } else {
        clip.speed_num as f32 / clip.speed_den as f32
    };

    let current = speed_presets()
        .iter()
        .find(|spec| active == Some(spec.id.as_str()))
        .map_or_else(
            || SharedString::new_static(if has_ramp { "Custom" } else { "None" }),
            |spec| SharedString::new_static(spec.label.as_str()),
        );
    let mut chips: Vec<_> = speed_presets()
        .iter()
        .map(|spec| {
            let id = spec.id.as_str();
            let label = SharedString::new_static(spec.label.as_str());
            choice_chip(
                ("speed-preset", id),
                label,
                active == Some(id),
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::SpeedPreset(id.to_string())),
            )
        })
        .collect();
    if has_ramp {
        chips.push(Chip::other(pick_button(
            "speed-clear",
            "Clear ramp",
            theme,
            cx,
            |state| clip_pick::apply(state, Pick::ClearSpeedCurve),
        )));
    }

    let rows: Vec<_> = points
        .into_iter()
        .map(|(id, label, value)| {
            let tick = RAMP_POINTS
                .iter()
                .find(|(point_id, ..)| *point_id == id)
                .map(|(.., tick)| *tick)
                .unwrap_or(0);
            unkeyed_row(
                theme,
                &state,
                MotionKey::new("insp.row", id),
                label,
                Readout::of(Units::SPEED, value),
                Columns::Free,
                move |state, by| {
                    let by = Units::SPEED.number.scale.stored_delta(value, by);
                    state.nudge_selected_ramp_point(tick, by)
                },
                cx,
            )
            .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(label_note(
            Columns::Free,
            theme.text_muted,
            if has_ramp {
                format!("Ramp on a {} base", Units::SPEED.print(flat))
            } else {
                format!("Flat {}", Units::SPEED.print(flat))
            },
        ))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "speed-ramp"),
            "Ramp",
            Columns::Free,
            current,
            chips,
        ))
        .child(under_labels(
            Columns::Free,
            [graph(theme, curve, drag, &state, cx).into_any_element()],
        ))
        .when_some(length, |el, ticks| {
            el.child(label_note(
                Columns::Free,
                theme.text_muted,
                format!("Length {}", format_ticks(ticks, fps_num, fps_den)),
            ))
        })
        .children(rows)
}

/// The curve graph: a bordered box holding one canvas, which measures the
/// rectangle and paints the 100% guide, the curve over the area under it, and
/// one dot per keyframe.
///
/// A press is routed on the box rather than on the dots, because a press has to
/// be able to grab a nearby point *or* add one where it landed, and only the box
/// has the pointer's place on the graph. The moves and the release that follow
/// reach the ramp through the window.
fn graph(
    theme: &Theme,
    curve: Param<f32>,
    drag: Option<RampDrag>,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id("speed-graph")
        .debug_selector(|| "speed-graph".into())
        .relative()
        .h(px(GRAPH_HEIGHT))
        .w_full()
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .child(plot(Ink::of(theme), curve, drag, state))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                run(
                    this,
                    Gesture::Press {
                        at: point_of(event.position),
                    },
                    cx,
                );
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                run(
                    this,
                    Gesture::Drop {
                        at: point_of(event.position),
                    },
                    cx,
                );
            }),
        )
}

/// The colors the graph is painted with.
#[derive(Clone, Copy)]
struct Ink {
    guide: Hsla,
    area: Hsla,
    line: Hsla,
    dot: Hsla,
    held: Hsla,
}

impl Ink {
    fn of(theme: &Theme) -> Self {
        Self {
            guide: theme.border,
            area: theme.accent.opacity(AREA),
            line: theme.accent,
            dot: theme.text,
            held: theme.accent,
        }
    }
}

/// Records the graph rectangle on every prepaint, so a pointer position can be
/// turned into a ramp point, and paints the curve into that same rectangle, so
/// a dot is drawn where a press reads it.
///
/// Repaint only when the rectangle moved; anything else is a repaint loop.
fn plot(
    ink: Ink,
    curve: Param<f32>,
    drag: Option<RampDrag>,
    state: &gpui::Entity<crate::app::AppState>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    canvas(
        move |bounds, _, cx| {
            let frame = [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ];
            if state.update(cx, |state, _| state.record_ramp_frame(frame)) {
                crate::shell::notify_after_draw(&state, cx);
            }
        },
        move |bounds, (), window, _| paint(bounds, &curve, drag, ink, window),
    )
    .absolute()
    .size_full()
}

/// Paint the 100% guide, the area under the curve, the curve, and the dots.
fn paint(
    bounds: Bounds<Pixels>,
    curve: &Param<f32>,
    drag: Option<RampDrag>,
    ink: Ink,
    window: &mut gpui::Window,
) {
    let at = |frac: f32, value: f32| place(bounds, frac, value);

    // The 100% guide, so a point above or below unit speed is obvious.
    let guide = point(bounds.origin.x, at(0.0, 1.0).y);
    window.paint_quad(fill(
        Bounds::new(guide, size(bounds.size.width, px(1.))),
        ink.guide,
    ));

    let mut area = PathBuilder::fill();
    let mut stroke = PathBuilder::stroke(px(LINE));
    area.move_to(bounds.bottom_left());
    for (index, (frac, value)) in line(curve).enumerate() {
        let vertex = at(frac, value);
        area.line_to(vertex);
        if index == 0 {
            stroke.move_to(vertex);
        } else {
            stroke.line_to(vertex);
        }
    }
    area.line_to(bounds.bottom_right());
    area.close();
    if let Ok(area) = area.build() {
        window.paint_path(area, ink.area);
    }
    if let Ok(stroke) = stroke.build() {
        window.paint_path(stroke, ink.line);
    }

    for (frac, value, held) in dots(curve.keyframes(), drag) {
        let color = if held { ink.held } else { ink.dot };
        let dot = Bounds::centered_at(at(frac, value), size(px(DOT), px(DOT)));
        window.paint_quad(fill(dot, color).corner_radii(px(DOT / 2.0)));
    }
}

/// Where `value` at x fraction `frac` sits in the graph rectangle `bounds`: the
/// inverse of [`AppState::ramp_at_pointer`](crate::app::AppState::ramp_at_pointer),
/// so a dot is drawn where a press on it reads.
fn place(bounds: Bounds<Pixels>, frac: f32, value: f32) -> gpui::Point<Pixels> {
    point(
        bounds.origin.x + bounds.size.width * frac,
        bounds.origin.y + bounds.size.height * (1.0 - ramp_value_to_frac(value)),
    )
}

/// The x fractions the curve is drawn through, in order: an even grid of
/// [`SAMPLES`], so a stretch bends as the log axis bends it, merged with every
/// point, so a corner is drawn where its point sits rather than at the nearest
/// grid step.
fn vertices(keyframes: &[Keyframe<f32>]) -> impl Iterator<Item = f32> + '_ {
    let mut grid = (0..SAMPLES)
        .map(|step| step as f32 / (SAMPLES - 1) as f32)
        .peekable();
    let mut points = keyframes
        .iter()
        .map(|frame| ramp_tick_to_frac(frame.tick))
        .peekable();
    std::iter::from_fn(move || match (grid.peek(), points.peek()) {
        (Some(step), Some(next)) if next < step => points.next(),
        (Some(_), _) => grid.next(),
        (None, _) => points.next(),
    })
}

/// Each keyframe as `(x fraction, multiplier, held)`. The held dot is the one
/// at the drag's current tick, so it stays lit once the pointer moves it
/// sideways off the tick it was grabbed at.
fn dots(
    keyframes: &[Keyframe<f32>],
    drag: Option<RampDrag>,
) -> impl Iterator<Item = (f32, f32, bool)> + '_ {
    let held = drag.map(|drag| drag.tick);
    keyframes.iter().map(move |frame| {
        (
            ramp_tick_to_frac(frame.tick),
            frame.value,
            held == Some(frame.tick),
        )
    })
}

/// A pointer position in the graph's own units.
fn point_of(at: gpui::Point<Pixels>) -> [f32; 2] {
    [f32::from(at.x), f32::from(at.y)]
}

/// Run one gesture, repainting only when the graph changed. A refusal is
/// reported in the status line rather than swallowed.
fn run(
    shell: &mut crate::shell::Shell,
    gesture: Gesture,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    if shell
        .state
        .update(cx, |state, _| ramp_gesture::run(state, gesture))
    {
        cx.notify();
    }
}

/// The curve as `(x fraction, multiplier)` vertices at each of [`vertices`],
/// left to right. Each multiplier is the curve's own sample, so a stretch is
/// drawn with the easing the clip plays it with.
fn line(curve: &Param<f32>) -> impl Iterator<Item = (f32, f32)> + '_ {
    vertices(curve.keyframes()).map(|frac| {
        let tick = f64::from(frac) * SPEED_CURVE_SCALE as f64;
        (frac, curve.sample_at(tick))
    })
}

#[cfg(test)]
mod tests {
    //! The painted graph's geometry: where the curve and the dots are drawn, and
    //! which dot is lit.
    //!
    //! A test window records no paint, so these hold the pure functions the
    //! paint is made of. They do not catch a color swapped in [`super::Ink`]
    //! or a path built and never painted.

    use aria_models::{Easing, Keyframe, SPEED_CURVE_SCALE, speed_preset, speed_preset_catalog};
    use gpui::{Bounds, point, px, size};

    use super::{SAMPLES, dots, line, place, vertices};
    use crate::app::clip_speed::{RampDrag, ramp_frac_to_tick, ramp_tick_to_frac};

    /// Linear keyframes at `points`.
    fn linear(points: &[(i64, f32)]) -> Vec<Keyframe<f32>> {
        points
            .iter()
            .map(|(tick, value)| Keyframe::new(*tick, *value, Easing::Linear))
            .collect()
    }

    /// A dot moved sideways off the tick it was grabbed at stays the lit one,
    /// and no dot is lit with nothing held.
    ///
    /// Closes "the highlight follows the grab tick": each dot was compared with
    /// the drag's origin, so the held dot went dark as soon as the pointer
    /// moved it along time.
    #[test]
    fn the_dot_under_a_sideways_drag_stays_lit() {
        let grabbed = SPEED_CURVE_SCALE / 2;
        let moved = grabbed + SPEED_CURVE_SCALE / 10;
        let curve = linear(&[(0, 1.0), (moved, 2.0), (SPEED_CURVE_SCALE, 1.0)]);
        let drag = RampDrag {
            origin: grabbed,
            tick: moved,
            value: 2.0,
        };
        let lit: Vec<bool> = dots(&curve, Some(drag)).map(|(.., held)| held).collect();
        assert_eq!(lit, [false, true, false]);
        assert!(dots(&curve, None).all(|(.., held)| !held));
    }

    /// Each dot is drawn where a press reads it: the window point a dot is
    /// painted at maps back, through the rectangle the graph recorded, to the
    /// dot's own tick and multiplier.
    ///
    /// Closes "a dot drawn off the place a press grabs": the dots were laid out
    /// from their left edge and against the box's outer height, so each one
    /// sat half a dot right of the point it stood for.
    #[test]
    fn a_dot_is_drawn_where_a_press_on_it_reads() {
        let (mut state, _) = crate::app::premiere_tests::with_clip();
        let frame = [40.0, 300.0, 236.0, 118.0];
        state.record_ramp_frame(frame);
        let bounds = Bounds::new(
            point(px(frame[0]), px(frame[1])),
            size(px(frame[2]), px(frame[3])),
        );
        let points = [
            (0, 0.5),
            (SPEED_CURVE_SCALE / 3, 4.0),
            (SPEED_CURVE_SCALE, 1.0),
        ];
        let curve = linear(&points);
        for ((tick, value), (frac, drawn, _)) in points.iter().zip(dots(&curve, None)) {
            let at = place(bounds, frac, drawn);
            let (read_tick, read_value) = state
                .ramp_at_pointer(f32::from(at.x), f32::from(at.y))
                .expect("the graph is measured");
            assert_eq!(read_tick, *tick);
            assert!(
                (read_value - value).abs() < value * 1e-3,
                "a dot at {value}x reads as {read_value}x"
            );
        }
    }

    /// The curve is drawn left to right from edge to edge, in steps no wider
    /// than the grid, through every point, so a corner sits on its point.
    ///
    /// Closes "a corner cut at the nearest grid step": a polyline through the
    /// grid alone misses a point that falls between two steps.
    #[test]
    fn the_curve_runs_edge_to_edge_through_every_point() {
        let curve = linear(&[
            (0, 1.0),
            (SPEED_CURVE_SCALE / 7, 3.0),
            (SPEED_CURVE_SCALE * 5 / 9, 0.25),
            (SPEED_CURVE_SCALE, 1.0),
        ]);
        let drawn: Vec<f32> = vertices(&curve).collect();
        let step = 1.0 / (SAMPLES - 1) as f32;
        assert_eq!(drawn.len(), SAMPLES + curve.len());
        assert_eq!((drawn[0], drawn[drawn.len() - 1]), (0.0, 1.0));
        let even = |p: &[f32]| p[0] <= p[1] && p[1] - p[0] <= step + f32::EPSILON;
        assert!(drawn.windows(2).all(even), "{drawn:?}");
        for frame in &curve {
            let frac = ramp_tick_to_frac(frame.tick);
            assert!(
                drawn.contains(&frac),
                "the point at {frac} is off the curve"
            );
        }
    }

    /// Every catalog preset is drawn as the clip plays it: each vertex of the
    /// line is within two hundredths of the multiplier the preset has at the
    /// nearest whole tick; half a tick of rounding moves the steepest preset by
    /// less than that. Derived from the catalog, so a new preset is drawn here
    /// too.
    ///
    /// Closes "the graph straightens an eased ramp": the line was interpolated
    /// straight between the points, while every preset eases between them, so
    /// the graph showed a different ramp from the one the clip played.
    #[test]
    fn every_preset_is_drawn_with_its_easing() {
        let mut bent = false;
        for spec in speed_preset_catalog() {
            let curve = speed_preset(&spec.id).expect("a catalog preset builds");
            let frames = curve.keyframes();
            for (frac, drawn) in line(&curve) {
                let tick = ramp_frac_to_tick(frac);
                let plays = curve.sample(tick);
                assert!(
                    (drawn - plays).abs() < 2e-2,
                    "{} is drawn at {drawn}x at tick {tick} and plays at {plays}x",
                    spec.id
                );
                let next = frames.partition_point(|frame| frame.tick <= tick);
                let previous = next.checked_sub(1).and_then(|index| frames.get(index));
                if let (Some(a), Some(b)) = (previous, frames.get(next)) {
                    let t = (tick - a.tick) as f32 / (b.tick - a.tick) as f32;
                    bent |= (plays - (a.value + (b.value - a.value) * t)).abs() > 1e-1;
                }
            }
        }
        assert!(
            bent,
            "no preset eases between its points, so a straight line cannot be told apart"
        );
    }
}
