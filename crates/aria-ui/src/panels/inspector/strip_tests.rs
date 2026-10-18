//! Effect Controls drawn: the stopwatch, the keyframe navigator, the reset
//! button and the time area beside an effect parameter, each pressed where the
//! inspector paints it.
//!
//! Closes the class "an Effect Controls control that is painted and does
//! nothing, or does something other than the timeline's property row": every
//! press is read back through the model's keyframes and the playhead, which is
//! what the timeline's row reads. Does not cover what the strip paints; the
//! arithmetic of a press is `key_strip_tests`.

use std::path::PathBuf;

use aria_models::{ClipId, ClipParam, ParamValue};
use gpui::{
    Bounds, FontWeight, Modifiers, MouseButton, Pixels, Point, ScaledPixels, SharedString,
    TestAppContext, VisualTestContext, point,
};

use crate::app::AppState;
use crate::dock::{Axis, FrameId, Node, PANEL_INSPECTOR};
use crate::panels::inspector::blocks::key_strip::{STRIP_MIN, STRIP_W};
use crate::panels::inspector::blocks::widgets::LABEL_RESERVE;
use crate::surfaces::effects_rack::{self, Gesture};
use crate::test_support::{drawn_shell, panel_bounds, refresh_panels};
use crate::theme::Theme;

/// The blur's radius: the first slot of the first effect.
const RADIUS: ClipParam = ClipParam::Effect {
    effect: 0,
    param: 0,
};

/// The radius as the blur ships it.
const DEFAULT_RADIUS: f32 = 4.0;

/// The Transform block's rows, each with a time area.
const TRANSFORM: [ClipParam; 5] = [
    ClipParam::Position,
    ClipParam::AnchorPoint,
    ClipParam::Scale,
    ClipParam::Rotation,
    ClipParam::Opacity,
];

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Where the inspector painted `control` on the row of `param`.
fn painted(visual: &mut VisualTestContext, control: &str, param: ClipParam) -> Bounds<Pixels> {
    let selector: &'static str = format!("insp-{control}-{param:?}").leak();
    panel_bounds(visual, selector).unwrap_or_else(|| panic!("Effect Controls draws no {selector}"))
}

/// Where the inspector painted `control` on the radius row.
fn on_radius(visual: &mut VisualTestContext, control: &str) -> Bounds<Pixels> {
    painted(visual, control, RADIUS)
}

/// Import the texture, place it, select it, and open the inspector on `tab`.
fn select_the_texture(state: &mut AppState, tab: &str) {
    let media = state.import_media(fixture()).expect("import the fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the clip");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .next()
        .expect("a clip");
    state.select_clip(clip);
    state.inspector_tab = tab.to_string();
    state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
}

/// Draw the shell with the texture selected and the inspector on `tab`, after
/// `then` edits the state.
fn drawn_on<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
    tab: &str,
    then: impl FnOnce(&mut AppState),
) -> (gpui::Entity<AppState>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
    visual.update(|_, app| {
        state.update(app, |state, _| {
            select_the_texture(state, tab);
            then(state);
        });
    });
    refresh_panels(visual);
    (state, visual)
}

/// The vertical middle of `bounds`, in pixels.
fn middle(bounds: Bounds<Pixels>) -> f32 {
    f32::from(bounds.center().y)
}

/// The radius's keyframes in absolute ticks, the playhead, and the radius
/// there.
fn read(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> (Vec<i64>, i64, f32) {
    visual.read(|app| {
        let state = state.read(app);
        let Some(ParamValue::Scalar(radius)) = state.param_value(RADIUS) else {
            panic!("the radius reads no number");
        };
        let keys = state
            .keyframes_of(RADIUS)
            .iter()
            .map(|mark| mark.tick)
            .collect();
        (keys, state.playhead.value, radius)
    })
}

/// The window point `fraction` across a strip, at its middle.
fn across(strip: Bounds<Pixels>, fraction: f32) -> Point<Pixels> {
    point(
        strip.origin.x + strip.size.width * fraction,
        strip.center().y,
    )
}

fn click(visual: &mut VisualTestContext, at: Point<Pixels>) {
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
async fn every_control_beside_an_effect_parameter_edits_what_the_timeline_reads(
    cx: &mut TestAppContext,
) {
    let (state, visual) = drawn_on(cx, "effect-controls-strip", "effects", |state| {
        state
            .add_effect_to_selected("gaussian_blur")
            .expect("add a blur");
        let strip = state.key_strip(RADIUS).expect("the radius has a time area");
        state.set_playhead(strip.start + (strip.end - strip.start) / 4);
    });
    let (start, end) = visual.read(|app| {
        let strip = state
            .read(app)
            .key_strip(RADIUS)
            .expect("the radius has a time area");
        (strip.start, strip.end)
    });
    let quarter = start + (end - start) / 4;

    // The stopwatch animates the radius with a keyframe under the playhead.
    let watch = on_radius(visual, "key").center();
    click(visual, watch);
    assert_eq!(
        read(&state, visual),
        (vec![quarter], quarter, DEFAULT_RADIUS),
        "the stopwatch keyed the playhead"
    );

    // A press on the strip away from the keyframe moves the playhead to the
    // frame under it and leaves the curve alone.
    let three_quarters = across(on_radius(visual, "strip"), 0.75);
    visual.simulate_mouse_down(three_quarters, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(three_quarters, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let late = visual.read(|app| {
        state
            .read(app)
            .key_strip(RADIUS)
            .expect("the radius has a time area")
            .tick_at(0.75)
    });
    assert_eq!(
        read(&state, visual),
        (vec![quarter], late, DEFAULT_RADIUS),
        "a press on the strip scrubbed the playhead to the frame under it"
    );

    // An edit there keys the animated radius where the row reads it.
    visual.update(|_, app| {
        state.update(app, |state, _| {
            effects_rack::apply(
                state,
                Gesture::NudgeScalar {
                    index: 0,
                    slot: 0,
                    by: 6.0,
                },
            )
            .expect("nudge the radius");
        })
    });
    visual.run_until_parked();
    assert_eq!(
        read(&state, visual),
        (vec![quarter, late], late, DEFAULT_RADIUS + 6.0),
        "an edit on an animated radius keyed the playhead"
    );

    // The arrows walk the keyframes.
    let previous = on_radius(visual, "kf-prev").center();
    click(visual, previous);
    assert_eq!(read(&state, visual).1, quarter, "the previous arrow");
    let next = on_radius(visual, "kf-next").center();
    click(visual, next);
    assert_eq!(read(&state, visual).1, late, "the next arrow");

    // The diamond takes the keyframe under the playhead off, and puts one back
    // holding the value the curve reads there.
    let diamond = on_radius(visual, "kf-at").center();
    click(visual, diamond);
    assert_eq!(
        read(&state, visual).0,
        vec![quarter],
        "the lit diamond took the key off"
    );
    let diamond = on_radius(visual, "kf-at").center();
    click(visual, diamond);
    assert_eq!(
        read(&state, visual),
        (vec![quarter, late], late, DEFAULT_RADIUS),
        "the unlit diamond keyed the value the curve reads"
    );

    // Reset keys the default where the playhead is and keeps the curve.
    visual.update(|_, app| {
        state.update(app, |state, _| {
            effects_rack::apply(
                state,
                Gesture::NudgeScalar {
                    index: 0,
                    slot: 0,
                    by: 6.0,
                },
            )
            .expect("nudge the radius");
        })
    });
    visual.run_until_parked();
    let reset = on_radius(visual, "prop-reset").center();
    click(visual, reset);
    assert_eq!(
        read(&state, visual),
        (vec![quarter, late], late, DEFAULT_RADIUS),
        "reset keyed the default under the playhead"
    );

    // A drag on the strip moves the keyframe under the press, and one undo
    // puts it back.
    let strip = on_radius(visual, "strip");
    let from = visual.read(|app| {
        state
            .read(app)
            .key_strip(RADIUS)
            .expect("the radius has a time area")
            .fraction(quarter)
    });
    let (grab, drop) = (across(strip, from), across(strip, 0.5));
    visual.simulate_mouse_down(grab, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(drop, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(drop, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let (keys, _, _) = read(&state, visual);
    let half = start + (end - start) / 2;
    assert!(
        keys.len() == 2 && (keys[0] - half).abs() <= 1 && keys[1] == late,
        "the drag left the keys at {keys:?}, not near {half} and at {late}"
    );
    visual.update(|_, app| state.update(app, |state, _| assert!(state.undo())));
    visual.run_until_parked();
    assert_eq!(
        read(&state, visual).0,
        vec![quarter, late],
        "one undo put the dragged keyframe back"
    );

    // The lit stopwatch drops every keyframe and holds the value the playhead
    // reads, as one undo step.
    visual.update(|_, app| {
        state.update(app, |state, _| {
            effects_rack::apply(
                state,
                Gesture::NudgeScalar {
                    index: 0,
                    slot: 0,
                    by: 6.0,
                },
            )
            .expect("nudge the radius");
        })
    });
    visual.run_until_parked();
    let (keyed, at, held) = read(&state, visual);
    assert!(
        !keyed.is_empty() && held == DEFAULT_RADIUS + 6.0,
        "the radius is animated and reads the nudge: {keyed:?} {held}"
    );
    let watch = on_radius(visual, "key").center();
    click(visual, watch);
    assert_eq!(
        read(&state, visual),
        (Vec::new(), at, held),
        "the lit stopwatch held the value under the playhead and dropped every key"
    );
    visual.update(|_, app| state.update(app, |state, _| assert!(state.undo())));
    visual.run_until_parked();
    assert_eq!(
        read(&state, visual),
        (keyed, at, held),
        "one undo put the curve back"
    );
}

/// Every row with a time area draws it on the stopwatch's line, between one
/// left edge and one right edge shared by every row, so keyframes line up in
/// time down the panel.
///
/// Closes the class "a label long enough to push its row's time area onto a
/// second line or sideways": the Transform block holds the longest built-in
/// labels. Colour rows are the next test, and a frame too narrow for every
/// control the one after.
#[gpui::test]
async fn every_time_area_is_drawn_on_its_stopwatchs_line_in_one_column(cx: &mut TestAppContext) {
    let (_state, visual) = drawn_on(cx, "effect-controls-column", "transform", |_| {});
    let mut column = Vec::new();
    for param in TRANSFORM {
        let (watch, strip) = (
            painted(visual, "key", param),
            painted(visual, "strip", param),
        );
        assert!(
            (middle(watch) - middle(strip)).abs() < 1.0,
            "{param:?} drew its time area at {strip:?}, off its stopwatch's line at {watch:?}"
        );
        column.push((param, strip.left(), strip.right()));
    }
    assert!(
        column
            .windows(2)
            .all(|pair| (pair[0].1, pair[0].2) == (pair[1].1, pair[1].2)),
        "the time areas do not share one column: {column:?}"
    );
}

/// A colour row's time area starts and ends where a number row's does: the
/// palette is on the line under the label, so the navigator, the reset button
/// and the strip end the label's line as they end a number row's, and the
/// colour's name stands in the value column the number does.
///
/// Closes the class "a colour row whose palette or name pushes its time area
/// out of the column": the overlay's Color and Amount are rows of one effect,
/// read one above the other. A frame too narrow for the controls narrows every
/// strip by the same amount because the values share the column; the test
/// reads the column rather than a narrow frame. Does not cover a value wider
/// than the column.
#[gpui::test]
async fn a_colour_rows_time_area_lines_up_with_a_number_rows(cx: &mut TestAppContext) {
    let (_state, visual) = drawn_on(cx, "effect-controls-colour", "effects", |state| {
        state
            .add_effect_to_selected("color_overlay")
            .expect("add an overlay");
    });
    let colour_param = ClipParam::Effect {
        effect: 0,
        param: 0,
    };
    let (watch, colour) = (
        painted(visual, "key", colour_param),
        painted(visual, "strip", colour_param),
    );
    assert!(
        (middle(watch) - middle(colour)).abs() < 1.0,
        "the colour drew its time area at {colour:?}, off its stopwatch's line at {watch:?}"
    );
    let amount_param = ClipParam::Effect {
        effect: 0,
        param: 2,
    };
    let amount = painted(visual, "strip", amount_param);
    assert_eq!(
        (colour.left(), colour.right()),
        (amount.left(), amount.right()),
        "the colour's time area at {colour:?} is off the amount's at {amount:?}"
    );
    // The colour's name and the amount's number differ in width, and both stand
    // in one value column, so a narrower frame narrows both strips alike.
    let (name, number) = (
        painted(visual, "value", colour_param),
        painted(visual, "value", amount_param),
    );
    assert_eq!(
        (name.left(), name.right()),
        (number.left(), number.right()),
        "the colour's name at {name:?} is out of the amount's value column at {number:?}"
    );
}

/// The child path from `node` to the frame `id`, pushed onto `path`.
fn path_to(node: &Node, id: FrameId, path: &mut Vec<usize>) -> bool {
    match node {
        Node::Frame(frame) => frame.id == id,
        Node::Split { children, .. } => children.iter().enumerate().any(|(index, (_, child))| {
            path.push(index);
            let found = path_to(child, id, path);
            if !found {
                path.pop();
            }
            found
        }),
    }
}

/// The nearest left-to-right split holding the Inspector's frame, and which of
/// its children holds it.
fn inspector_column(state: &AppState) -> (Vec<usize>, usize) {
    let frame = state
        .dock_tree
        .find(PANEL_INSPECTOR)
        .expect("the Inspector is docked");
    let root = state.dock_tree.node_at(&[]).expect("a dock root");
    let mut path = Vec::new();
    assert!(
        path_to(root, frame, &mut path),
        "the Inspector's frame is in no split"
    );
    (0..path.len())
        .rev()
        .find(|&depth| {
            matches!(
                state.dock_tree.node_at(&path[..depth]),
                Some(Node::Split {
                    axis: Axis::Row,
                    ..
                })
            )
        })
        .map(|depth| (path[..depth].to_vec(), path[depth]))
        .expect("the Inspector's frame sits in a row")
}

/// Narrow the Inspector's column a step at a time, from half the row to
/// narrower than its controls, drawing each step. `step` reads the drawn
/// shell with the right edge of the Inspector's frame, and ends the walk by
/// returning true.
fn narrow_the_inspector(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    mut step: impl FnMut(&mut VisualTestContext, f32) -> bool,
) {
    // A weight set outside a drag glides to its target over time, which a test
    // window never advances, so the walk draws each weight at once.
    visual.update(|_, app| app.set_reduce_motion(true));
    let (split, child) = visual.read(|app| inspector_column(state.read(app)));
    let shipped = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .split_weights(&split)
            .expect("the row's weights")
    });
    let others: f32 = shipped
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != child)
        .map(|(_, weight)| weight)
        .sum();
    for at in 0..=40 {
        let share = 0.5 - 0.0105 * at as f32;
        let weights: Vec<f32> = shipped
            .iter()
            .enumerate()
            .map(|(index, weight)| {
                if index == child {
                    share
                } else {
                    weight / others * (1.0 - share)
                }
            })
            .collect();
        visual.update(|_, app| {
            state.update(app, |state, _| {
                assert!(state.dock_tree.set_weights(&split, &weights));
            });
        });
        refresh_panels(visual);
        let right = visual.read(|app| {
            let state = state.read(app);
            let id = state.dock_tree.find(PANEL_INSPECTOR).expect("docked");
            let [x, _, w, _] = *state
                .dock_frame_rects
                .lock()
                .get(&id)
                .expect("the Inspector's frame was drawn");
            x + w
        });
        if step(visual, right) {
            return;
        }
    }
}

/// Whether the last frame painted a quad inside `strip`: its card, its lane,
/// its playhead, or a keyframe.
fn inked(visual: &mut VisualTestContext, strip: Bounds<Pixels>) -> bool {
    visual.update(|window, _| {
        let strip = strip.scale(window.scale_factor());
        window.painted_quads().iter().any(|quad| {
            let center = quad.bounds.center();
            strip.contains(&center)
                && quad.bounds.size.width <= strip.size.width + ScaledPixels(0.5)
        })
    })
}

/// The width a frame too narrow for every control takes from the strip first:
/// while a strip has any width, every label holds `LABEL_RESERVE`, and a strip
/// narrower than `STRIP_MIN` paints nothing and leaves a press to its row. At
/// every width the strips stay in one column on their stopwatches' lines,
/// inside the frame.
///
/// Closes the class "a label truncated while the time area beside it still had
/// room to give", where Anchor point X ended in an ellipsis beside a full
/// strip, then beside a strip too narrow to read. The Inspector's column is
/// narrowed a step at a time from wide to narrower than the controls, and the
/// steps are asserted to reach all four regimes: a full strip, a narrowing
/// one, a blank one, and none beside a shrinking label. Does not cover the
/// ellipsis, which the test text system does not draw, a value wider than the
/// value column, the cursor over a strip, or a frame narrower than the
/// controls that never narrow.
#[gpui::test]
async fn a_narrow_frame_takes_its_width_from_the_strips_before_the_labels(cx: &mut TestAppContext) {
    let (state, visual) = drawn_on(cx, "effect-controls-narrow", "transform", |_| {});
    let (mut full, mut narrowing, mut blank, mut shrinking) = (false, false, false, false);
    narrow_the_inspector(&state, visual, |visual, frame| {
        let first = painted(visual, "strip", TRANSFORM[0]);
        for param in TRANSFORM {
            let (watch, strip) = (
                painted(visual, "key", param),
                painted(visual, "strip", param),
            );
            let label = f32::from(painted(visual, "label", param).size.width);
            let wide = f32::from(strip.size.width);
            let at = format!("frame edge {frame}: {param:?} label {label}, strip {strip:?}");
            assert!(
                (middle(watch) - middle(strip)).abs() < 1.0,
                "{at}: off its stopwatch's line at {watch:?}"
            );
            assert!(
                (strip.left(), strip.right()) == (first.left(), first.right()),
                "{at}: out of the column at {first:?}"
            );
            assert!(
                f32::from(strip.right()) <= frame + 0.5,
                "{at}: past the frame's right edge"
            );
            assert!(
                (0.0..=STRIP_W + 0.1).contains(&wide),
                "{at}: a strip outside its range"
            );
            if wide > 0.5 {
                assert!(
                    label >= LABEL_RESERVE - 0.2,
                    "{at}: the label shrank while the strip had width to give"
                );
            }
            let drawn = inked(visual, strip);
            assert!(
                drawn == (wide >= STRIP_MIN),
                "{at}: the strip painted {drawn} at {wide} px"
            );
            full |= wide >= STRIP_W - 0.1;
            narrowing |= wide > STRIP_MIN + 1.0 && wide < STRIP_W - 1.0;
            shrinking |= label < LABEL_RESERVE - 1.0;
            if wide > 0.5 && wide < STRIP_MIN - 0.5 && !blank {
                blank = true;
                let before = read_playhead(&state, visual);
                click(visual, across(strip, 0.5));
                assert_eq!(
                    read_playhead(&state, visual),
                    before,
                    "{at}: a press on a blank strip moved the playhead"
                );
            }
        }
        // Narrower still, the label reaches zero and the controls that never
        // narrow are wider than the frame, which no layout fits.
        shrinking
    });
    assert!(
        full && narrowing && blank && shrinking,
        "the steps reached full {full}, narrowing {narrowing}, blank {blank}, shrinking {shrinking}"
    );
}

/// The playhead, in ticks.
fn read_playhead(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> i64 {
    visual.read(|app| state.read(app).playhead.value)
}

/// Every Inspector tab takes a share of the strip in proportion to its label
/// and its padding, the tabs stay inside the strip, and the underline spans
/// the tab in front, at every width. While the strip is as wide as the labels
/// and their padding, no tab is narrower than its own.
///
/// Closes the class "a tab that cuts its label while the strip has room for
/// it": with equal shares, Transform, the longest label, was cut to "Transf…"
/// in the shipped layout while the short labels had room to spare. Also closes
/// "a tab off its underline": the underline is placed at the weights'
/// fractions, so a tab sized any other way leaves it. The walk must draw a
/// width where equal shares cut the longest label and weighted ones do not.
/// Does not cover the ellipsis, which the test text system does not draw.
#[gpui::test]
async fn every_inspector_tab_takes_a_share_by_its_label_under_its_underline(
    cx: &mut TestAppContext,
) {
    let (state, visual) = drawn_on(cx, "effect-controls-tabs", "transform", |_| {});
    // Each tab's label and padding, measured in the font the strip draws.
    let weights: Vec<(SharedString, f32)> = visual.update(|_, app| {
        let font = Theme::of(app).ui_font(FontWeight::NORMAL);
        crate::panels::inspector::host_of()
            .tabs()
            .iter()
            .map(|tab| {
                let label = app
                    .text_system()
                    .advance_for_text(&tab.label, &font, Theme::TEXT_UI);
                (tab.id.clone(), f32::from(label) + 2.0 * Theme::SPACE_SM)
            })
            .collect()
    });
    let (mut steps, mut decisive) = (0, 0);
    // The tab in front moves one place right at every step, so the underline
    // is checked at every offset, not only at the first tab's.
    let mut in_front = SharedString::from("transform");
    narrow_the_inspector(&state, visual, |visual, frame| {
        steps += 1;
        let drawn: Vec<(&SharedString, Bounds<Pixels>, f32)> = weights
            .iter()
            .filter_map(|(id, weight)| {
                let selector: &'static str = format!("insp-tab-{id}").leak();
                visual
                    .debug_bounds(selector)
                    .map(|bounds| (id, bounds, *weight))
            })
            .collect();
        assert!(drawn.len() > 1, "the strip draws {} tabs", drawn.len());
        // Layout rounds every edge to the device pixel grid, so a share
        // differs from its fraction of the strip by up to one device pixel.
        let pixel = 1.0 / visual.update(|window, _| window.scale_factor());
        let strip = f32::from(drawn[drawn.len() - 1].1.right() - drawn[0].1.left());
        let total: f32 = drawn.iter().map(|(_, _, weight)| weight).sum();
        let widest = drawn
            .iter()
            .map(|(_, _, weight)| *weight)
            .fold(0.0, f32::max);
        if strip >= total && strip < widest * drawn.len() as f32 {
            decisive += 1;
        }
        for (id, bounds, weight) in &drawn {
            let width = f32::from(bounds.size.width);
            assert!(
                (width - strip * weight / total).abs() <= pixel,
                "frame edge {frame}: tab {id} is {width} of a {strip} strip, not its \
                 share {weight}/{total}"
            );
            assert!(
                strip < total || width >= weight - pixel,
                "frame edge {frame}: tab {id} is {width}, narrower than its label and \
                 padding {weight}, in a {strip} strip that holds every label"
            );
        }
        for pair in drawn.windows(2) {
            let ((_, left, _), (id, right, _)) = (&pair[0], &pair[1]);
            assert!(
                (right.left() - left.right()).abs() < gpui::px(0.5),
                "frame edge {frame}: tab {id} at {right:?} does not follow {left:?}"
            );
        }
        let last = drawn[drawn.len() - 1].1;
        assert!(
            f32::from(last.right()) <= frame + 0.5,
            "frame edge {frame}: the last tab {last:?} is past the frame"
        );
        let at = drawn
            .iter()
            .position(|(id, _, _)| **id == in_front)
            .unwrap_or_else(|| panic!("the tab in front, {in_front}, is drawn"));
        let front = drawn[at].1;
        let line = visual
            .debug_bounds("insp-tab-underline")
            .expect("the strip draws an underline");
        assert!(
            f32::from((line.left() - front.left()).abs()) <= pixel
                && f32::from((line.right() - front.right()).abs()) <= pixel,
            "frame edge {frame}: the underline {line:?} is not under {in_front} {front:?}"
        );
        in_front = drawn[(at + 1) % drawn.len()].0.clone();
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.inspector_tab = in_front.to_string();
                cx.notify();
            });
        });
        false
    });
    assert!(steps > 1, "the walk drew {steps} widths");
    assert!(
        decisive > 0,
        "the walk drew no width that holds every label but not an equal share for the \
         longest, so it cannot tell weighted shares from equal ones"
    );
}
