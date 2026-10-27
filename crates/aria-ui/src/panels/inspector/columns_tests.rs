//! The Inspector's rows in one set of columns: every line an Effect Controls
//! tab draws stands its label and its value where a keyed row stands them, at
//! every width the Inspector narrows to.
//!
//! Closes the class "an Inspector row laid out in columns of its own". The
//! fades, Reverse, Pitch lock and the crop insets printed their values at the
//! frame's right edge while Volume's and Opacity's stood in the value column,
//! and the audio role, the filters and an effect's switch, choice, text, font
//! and layer slots drew lines of full-width buttons. Every line the row
//! widgets build while the Inspector's body is built is recorded, so a row
//! added to a tab is held to the columns without being listed here. An effect
//! and a transition with a slot of every [`EffectParamKind`] are drawn, and
//! [`slot_spec`] has no wildcard arm, so a kind added to the model does not
//! build here until it is given a slot. A tab added to the Inspector fails
//! [`every_tab_is_held_to_the_columns_or_named_as_not`] until it is placed.
//! The Look tab is drawn with every layer style in `clip_styles::STYLES` on,
//! and each style's catalog colors must draw a line.
//!
//! Also closes the class "a choice row whose mark disagrees with its value".
//! An effect's layer slot left at None showed None and marked no chip. Every
//! choice row records the choice its value column shows and each chip's mark,
//! so a picker of any kind marks the choice it shows, one at most, and none
//! while it shows a mixed value.
//!
//! Also closes the class "an Inspector control stretched across its block".
//! Reset grade, Remove key, Clear styles, the blend modes, the key colors and
//! the matte sources each drew a line as wide as the block, starting left of
//! every label. Every ghost button, chip and switch built while the
//! Inspector's body is built is recorded, so a control added to a held tab is
//! held to what it draws without being listed here: a ghost button or a chip
//! to its label and padding, a switch to its track.
//!
//! Also closes the class "an Inspector control off the columns". A switch's
//! On or Off ended a ghost inset short of the numbers above it and stood its
//! row a ghost inset taller than a number row's; the chips under a choice
//! started a ghost inset right of the labels above them; and the mask shapes,
//! the Effects tab's lists, the curve easing presets and an insert chain's
//! Add button started left of the label column. A control's frame is what
//! stands on a column. Every recorded control on a row's line ends its frame
//! where the values end. Every other starts its frame on or right of the
//! leftmost column, and the first of each line of them starts on a column:
//! the stopwatches' or the labels' in the keyed rows' columns, the labels' in
//! the free ones. Every line's value is no taller than a number's.
//!
//! Also closes the class "a tab in free columns of its own". The Info tab's
//! facts stood at the block's edges, and its template buttons a ghost inset
//! right of the label column, while the free rows' labels and values stood a
//! row's padding inside them; the Speed tab's graph spanned the block. On the
//! tabs in [`FREE`], every line's label starts where the reference line's
//! does, every line's value ends where its value ends, the graph spans the
//! two, and every control stands its frame on those columns.
//!
//! Also closes the class "two Inspector controls that share a hover". The
//! Keyframes block built each row's navigator, diamond, Driver and Flatten,
//! and the Replace media list each entry, under one id per kind of button, so
//! the pointer on one lit that button on every row. Every control built into
//! one Inspector body is recorded with its kind and id, and a kind and id
//! built twice fails the tab that drew it.
//!
//! Not covered: a row drawn without the row widgets, the tabs in [`NOT_HELD`],
//! a picker drawn outside a choice row, a line whose label another line in the
//! same columns shares, of which the last drawn is read, a control not built
//! with `components::ghost_button`, `components::chip` or `components::switch`,
//! such as an icon button or a note's text, a control whose caller replaces
//! its debug selector, and a control only drawn after a press, such as a list
//! a button opens.

use aria_models::{
    AudioInsertTarget, AudioRole, ClipParam, EffectChoiceOption, EffectCurvePoint,
    EffectGradientStop, EffectParamKind, EffectParamSpec, EffectSpec, LookParam, MaskKind,
    MatteSource, TransitionSpec,
};
use gpui::{Bounds, FontWeight, Pixels, TestAppContext, VisualTestContext};

use super::blocks::widgets::{
    DrawnChoice, DrawnLine, LABEL_RESERVE, MIXED, drawn_choices, drawn_lines, picked, record_lines,
};
use super::strip_tests::{drawn_with, middle, narrow_the_inspector};
use crate::app::AppState;
use crate::app::clip_keys::PIECEWISE_PRESETS;
use crate::app::clip_speed::speed_presets;
use crate::app::clip_template::SLOT_MEDIA;
use crate::components::{Drawn, DrawnControl, SWITCH_WIDTH, control_selector, drawn_controls};
use crate::motion::{ControlId, MotionKey};
use crate::surfaces::clip_pick::{self, Pick as ClipPick};
use crate::surfaces::inspector_pick::{self, KEY_COLORS, Pick};
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;

/// How far apart two edges may be and still stand in one column.
pub(super) const EPS: f32 = 0.51;

/// The id of the effect and of the transition with a slot of every kind.
const EVERY_KIND: &str = "test.ui.every_kind";

/// The tabs held to the keyed rows' columns here.
const HELD: [&str; 4] = ["transform", "look", "effects", "audio"];

/// The tabs whose rows draw no time area, held to free columns here.
const FREE: [&str; 2] = ["speed", "info"];

/// The tabs not held to any columns, and why.
const NOT_HELD: [(&str, &str); 2] = [
    ("text", "not drawn here: it needs a title clip"),
    ("shape", "not drawn here: it needs a shape clip"),
];

/// A slot of `kind`, labelled `{kind} {holder}`. No wildcard arm: a new kind
/// does not build until it is given a slot.
fn slot_spec(kind: EffectParamKind, holder: &str) -> EffectParamSpec {
    let name = kind.as_str();
    let label = format!("{name} {holder}");
    match kind {
        EffectParamKind::Scalar => EffectParamSpec::scalar(name, label, 0.25, 0.0, 1.0),
        EffectParamKind::Int => EffectParamSpec::int(name, label, 3, -5, 10),
        EffectParamKind::Bool => EffectParamSpec::boolean(name, label, true),
        EffectParamKind::Choice => EffectParamSpec::choice(
            name,
            label,
            vec![
                EffectChoiceOption::new("fast", "Fast"),
                EffectChoiceOption::new("best", "Best"),
            ],
            "best",
        ),
        EffectParamKind::Color => EffectParamSpec::color(name, label, [255, 0, 0, 255]),
        EffectParamKind::Vec2 => EffectParamSpec::vec2(name, label, [0.25, -0.25], -1.0, 1.0),
        EffectParamKind::Point3 => EffectParamSpec::point3(name, label, [0.0, 0.5, 1.0], -2.0, 2.0),
        EffectParamKind::Size => EffectParamSpec::size(name, label, [640.0, 360.0], 1.0, 8192.0),
        EffectParamKind::Angle => EffectParamSpec::angle(name, label, 45.0, -360.0, 360.0),
        EffectParamKind::Curve => EffectParamSpec::curve(
            name,
            label,
            vec![
                EffectCurvePoint::new(0.0, 0.0),
                EffectCurvePoint::new(1.0, 1.0),
            ],
        ),
        EffectParamKind::Gradient => EffectParamSpec::gradient(
            name,
            label,
            vec![
                EffectGradientStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
                EffectGradientStop::new(1.0, [1.0; 4]),
            ],
        ),
        EffectParamKind::Text => EffectParamSpec::text(name, label, "x"),
        EffectParamKind::Font => EffectParamSpec::font(name, label, "Inter"),
        EffectParamKind::File => EffectParamSpec::file(name, label, ""),
        EffectParamKind::Layer => EffectParamSpec::layer(name, label),
    }
}

/// A slot of every kind, in declaration order, labelled for their `holder`.
fn every_kind(holder: &str) -> Vec<EffectParamSpec> {
    EffectParamKind::ALL
        .into_iter()
        .map(|kind| slot_spec(kind, holder))
        .collect()
}

/// Whether a transition's slot of `kind` draws a row in the Inspector. No
/// wildcard arm: a new kind does not build until it is placed.
fn a_transition_draws(kind: EffectParamKind) -> bool {
    match kind {
        EffectParamKind::Scalar
        | EffectParamKind::Int
        | EffectParamKind::Bool
        | EffectParamKind::Choice
        | EffectParamKind::Color
        | EffectParamKind::Vec2
        | EffectParamKind::Point3
        | EffectParamKind::Size
        | EffectParamKind::Angle => true,
        EffectParamKind::Curve
        | EffectParamKind::Gradient
        | EffectParamKind::Text
        | EffectParamKind::Font
        | EffectParamKind::File
        | EffectParamKind::Layer => false,
    }
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() < EPS
}

/// Where the last frame drew `selector`.
fn drawn(visual: &mut VisualTestContext, selector: &'static str, at: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{at}: the Inspector draws no {selector}"))
}

/// Every choice row marks, of its chips, the one its value column shows and
/// no other, and marks none while it shows a mixed value.
fn every_choice_marks_what_it_shows(tab: &str, choices: &[DrawnChoice]) {
    for choice in choices {
        let marked: Vec<&str> = choice
            .chips
            .iter()
            .filter(|(_, on)| *on)
            .map(|(label, _)| &**label)
            .collect();
        let shown: Vec<&str> = choice
            .chips
            .iter()
            .filter(|(label, _)| &*choice.current != MIXED && *label == choice.current)
            .map(|(label, _)| &**label)
            .collect();
        assert!(
            marked == shown && marked.len() <= 1,
            "{tab}: {} shows {:?} and marks {marked:?} of {:?}",
            choice.row,
            choice.current,
            choice.chips
        );
    }
}

/// The choice row whose label's debug selector is `row` shows `shown` and
/// marks the chip of that name.
fn shows_and_marks(tab: &str, choices: &[DrawnChoice], row: &str, shown: &str) {
    let choice = choices
        .iter()
        .find(|choice| choice.row == row)
        .unwrap_or_else(|| panic!("{tab}: no choice row {row} among {choices:?}"));
    assert!(
        &*choice.current == shown
            && choice
                .chips
                .iter()
                .any(|(label, on)| *on && &**label == shown),
        "{tab}: {row} does not show and mark {shown:?}: {choice:?}"
    );
}

/// Every control built since the record began, once each: its debug selector
/// and what it draws. No two of one kind built into one Inspector body share
/// an id, and so a hover.
fn recorded_controls(tab: &str) -> Vec<(&'static str, Drawn)> {
    let (built, shared) = drawn_controls();
    assert!(
        shared.is_empty(),
        "{tab}: controls built with the kind and id of another control of the same Inspector body, so the pointer on one lights both: {shared:?}"
    );
    built
        .into_iter()
        .map(|DrawnControl { selector, drawn }| (&*selector.leak(), drawn))
        .collect()
}

/// What a walk's held controls name the switch `id` by: its debug selector.
fn switch(id: impl Into<ControlId>) -> String {
    control_selector("switch", id.into(), &Drawn::Switch)
}

/// Every control of `controls` the last frame drew into the Inspector is as
/// wide as what it draws and no wider: a ghost button or a chip its label and
/// its padding, a switch its track. A text element is as wide as its advance
/// at its size rounded up to a whole pixel, and layout rounds each edge to a
/// device pixel. Returns the controls held: a labelled one by its label, a
/// switch by its debug selector.
fn every_control_hugs_what_it_draws(
    visual: &mut VisualTestContext,
    tab: &str,
    controls: &[(&'static str, Drawn)],
) -> Vec<String> {
    let (pixel, hugs): (f32, Vec<f32>) = visual.update(|window, app| {
        let font = Theme::of(app).ui_font(FontWeight::NORMAL);
        let hugs = controls
            .iter()
            .map(|(_, drawn)| match drawn {
                Drawn::Labelled { label, inset, size } => {
                    f32::from(app.text_system().advance_for_text(label, &font, *size)).ceil()
                        + 2.0 * inset
                }
                Drawn::Switch => SWITCH_WIDTH,
            })
            .collect();
        (1.0 / window.scale_factor(), hugs)
    });
    let mut held = Vec::new();
    for (&(selector, ref drawn), hug) in controls.iter().zip(hugs) {
        // A control built for an earlier frame, or built and not drawn, has
        // no bounds in the last one.
        let Some(bounds) = visual.debug_bounds(selector) else {
            continue;
        };
        let width = f32::from(bounds.size.width);
        match drawn {
            Drawn::Labelled { label, .. } => {
                assert!(
                    width <= hug + pixel,
                    "{tab}: the control {label:?} ({selector}) is {width} wide, past the {hug} its label and padding take"
                );
                held.push(label.to_string());
            }
            Drawn::Switch => {
                assert!(
                    (width - hug).abs() < EPS,
                    "{tab}: the switch {selector} is {width} wide, not its track's {hug}"
                );
                held.push(selector.to_owned());
            }
        }
    }
    held
}

/// The debug selectors of each of `lines`' label, value and whole line.
fn cells(lines: &[DrawnLine]) -> Vec<(&'static str, &'static str, &'static str)> {
    lines
        .iter()
        .map(|line| {
            (
                &*line.label.clone().leak(),
                &*line.label.replacen("insp-label-", "insp-value-", 1).leak(),
                &*line.line.clone().leak(),
            )
        })
        .collect()
}

/// Every control of `controls` the last frame drew stands its frame on the
/// columns. One drawn inside a row's line, where `lines` are the rows' whole
/// lines, ends where the values end, at the reference row's `value`'s right.
/// Every other starts on or right of the leftmost of `edges`, and the first
/// of each line of them, the controls whose frames share height, starts on
/// one of `edges`. Returns how many were drawn.
fn every_control_stands_on_the_columns(
    visual: &mut VisualTestContext,
    at: &str,
    controls: &[(&'static str, Drawn)],
    lines: &[Bounds<Pixels>],
    edges: &[Pixels],
    value: Bounds<Pixels>,
) -> usize {
    let leftmost = edges
        .iter()
        .map(|edge| f32::from(*edge))
        .fold(f32::INFINITY, f32::min);
    let (mut drawn, mut off_lines) = (0, Vec::<(&str, Bounds<Pixels>)>::new());
    for &(selector, _) in controls {
        let Some(bounds) = visual.debug_bounds(selector) else {
            continue;
        };
        drawn += 1;
        if lines.iter().any(|line| {
            f32::from(bounds.top()) > f32::from(line.top()) - EPS
                && f32::from(bounds.bottom()) < f32::from(line.bottom()) + EPS
        }) {
            assert!(
                near(bounds.right(), value.right()),
                "{at}: {selector} on a row's line ends its frame at {:?}, not where the values end, at {:?}",
                bounds.right(),
                value.right()
            );
        } else {
            assert!(
                f32::from(bounds.left()) > leftmost - EPS,
                "{at}: {selector} starts its frame at {:?}, left of the leftmost column at {leftmost}",
                bounds.left()
            );
            off_lines.push((selector, bounds));
        }
    }
    off_lines.sort_by(|a, b| f32::from(a.1.top()).total_cmp(&f32::from(b.1.top())));
    let mut start = 0;
    while start < off_lines.len() {
        let mut bottom = f32::from(off_lines[start].1.bottom());
        let mut end = start + 1;
        while end < off_lines.len() && f32::from(off_lines[end].1.top()) < bottom {
            bottom = bottom.max(f32::from(off_lines[end].1.bottom()));
            end += 1;
        }
        let (selector, first) = off_lines[start..end]
            .iter()
            .min_by(|a, b| f32::from(a.1.left()).total_cmp(&f32::from(b.1.left())))
            .expect("a line holds a control");
        assert!(
            edges.iter().any(|edge| near(first.left(), *edge)),
            "{at}: {selector}, the first control of its line, starts its frame at {:?}, on none of the columns at {edges:?}",
            first.left()
        );
        start = end;
    }
    drawn
}

/// Every control of `wanted` is among the controls `held`.
fn holds(tab: &str, held: &[String], wanted: &[String]) {
    for control in wanted {
        assert!(
            held.contains(control),
            "{tab}: no control {control:?} was held to what it draws among {held:?}"
        );
    }
}

/// Draw the shell with the fixture `media` selected and the Inspector on
/// `tab`, after `then` edits the state, then narrow the Inspector a step at a
/// time. Every line drawn into the Inspector stands in the keyed rows'
/// columns, and at every step its label starts where the label of
/// `reference`, a keyed row with a time area, starts and is as wide, and its
/// value stands in that row's value column, on the label's line, no taller
/// than the reference's number. Every control stands its frame on those
/// columns at every step, and every step draws one or more of them. The walk
/// ends once the reference's label gives up width, which it must reach. Every
/// choice row drawn marks the choice it shows, and every control drawn before
/// the walk is as wide as what it draws and has a hover of its own.
/// Returns the debug selectors of the lines' labels, the choice rows, and the
/// controls held.
fn every_line_stands_in_the_columns(
    cx: &mut TestAppContext,
    (media, tab): (&str, &str),
    reference: ClipParam,
    then: impl FnOnce(&mut AppState),
) -> (Vec<String>, Vec<DrawnChoice>, Vec<String>) {
    record_lines();
    let (state, visual) = drawn_with(cx, &format!("columns-{tab}"), media, tab, then);
    let (lines, choices) = (drawn_lines(), drawn_choices());
    every_choice_marks_what_it_shows(tab, &choices);
    let controls = recorded_controls(tab);
    let held = every_control_hugs_what_it_draws(visual, tab, &controls);
    let free: Vec<&str> = lines
        .iter()
        .filter(|line| !line.keyed)
        .map(|line| line.label.as_str())
        .collect();
    assert!(
        free.is_empty(),
        "{tab}: lines drawn outside the keyed rows' columns: {free:?}"
    );
    let leak = |text: String| -> &'static str { text.leak() };
    let cells = cells(&lines);
    let (label_of, value_of) = (
        leak(format!("insp-label-{reference:?}")),
        leak(format!("insp-value-{reference:?}")),
    );
    assert!(
        cells
            .iter()
            .any(|&(label, value, _)| (label, value) == (label_of, value_of)),
        "{tab}: no line of {reference:?} among {lines:?}"
    );
    let (strip, watch) = (
        leak(format!("insp-strip-{reference:?}")),
        leak(format!("insp-key-{reference:?}")),
    );
    let (mut step, mut yielded) = (0, false);
    narrow_the_inspector(&state, visual, |visual, _| {
        step += 1;
        let at = format!("{tab}, step {step}");
        assert!(
            visual.debug_bounds(strip).is_some(),
            "{at}: {reference:?} draws no time area, so its columns are no keyed row's"
        );
        let (label, value) = (drawn(visual, label_of, &at), drawn(visual, value_of, &at));
        let edges = [drawn(visual, watch, &at).left(), label.left()];
        let mut rows = Vec::with_capacity(cells.len());
        for &(row_label, row_value, whole) in &cells {
            let (row, cell) = (drawn(visual, row_label, &at), drawn(visual, row_value, &at));
            let context = format!(
                "{at}: {row_label} at {row:?}, its value at {cell:?}, beside {reference:?}'s label at {label:?} and value at {value:?}"
            );
            assert!(
                near(row.left(), label.left()) && near(row.size.width, label.size.width),
                "{context}: the label is out of the label column"
            );
            assert!(
                near(cell.left(), value.left()) && near(cell.right(), value.right()),
                "{context}: the value is out of the value column"
            );
            assert!(
                (middle(row) - middle(cell)).abs() < 1.0,
                "{context}: the value is off its label's line"
            );
            assert!(
                f32::from(cell.size.height) < f32::from(value.size.height) + EPS,
                "{context}: the value is taller than a number, so its row is taller than a number row"
            );
            rows.push(drawn(visual, whole, &at));
        }
        let found =
            every_control_stands_on_the_columns(visual, &at, &controls, &rows, &edges, value);
        assert!(
            found > 0,
            "{at}: the Inspector draws none of the {} controls recorded before the walk",
            controls.len()
        );
        yielded = f32::from(label.size.width) < LABEL_RESERVE - 1.0;
        yielded
    });
    assert!(
        yielded,
        "{tab}: the walk ended before {reference:?}'s label gave up width"
    );
    (
        lines.into_iter().map(|line| line.label).collect(),
        choices,
        held,
    )
}

/// Draw the shell with the fixture `media` selected and the Inspector on
/// `tab`, whose rows draw no time area, after `then` edits the state, then
/// narrow the Inspector a step at a time. Every line drawn into the Inspector
/// stands in free columns, and at every step its label starts where the label
/// of the line labelled `reference` starts and its value ends where that
/// line's value ends. Every one of `boxes` spans the two, every control stands
/// its frame on those columns, and every step draws one or more of them.
/// Every choice row drawn marks the choice it shows, and every control drawn
/// before the walk is as wide as what it draws and has a hover of its own.
/// Returns the debug selectors of the lines' labels, the choice rows, and the
/// controls held.
fn every_line_stands_in_the_free_columns(
    cx: &mut TestAppContext,
    (media, tab): (&str, &str),
    (reference, boxes): (&str, &[&'static str]),
    then: impl FnOnce(&mut AppState),
) -> (Vec<String>, Vec<DrawnChoice>, Vec<String>) {
    record_lines();
    let (state, visual) = drawn_with(cx, &format!("columns-{tab}"), media, tab, then);
    let (lines, choices) = (drawn_lines(), drawn_choices());
    every_choice_marks_what_it_shows(tab, &choices);
    let controls = recorded_controls(tab);
    let held = every_control_hugs_what_it_draws(visual, tab, &controls);
    let keyed: Vec<&str> = lines
        .iter()
        .filter(|line| line.keyed)
        .map(|line| line.label.as_str())
        .collect();
    assert!(
        keyed.is_empty(),
        "{tab}: lines drawn in the keyed rows' columns: {keyed:?}"
    );
    let leak = |text: String| -> &'static str { text.leak() };
    let cells = cells(&lines);
    let (label_of, value_of) = (
        leak(format!("insp-label-free-{reference}")),
        leak(format!("insp-value-free-{reference}")),
    );
    assert!(
        cells
            .iter()
            .any(|&(label, value, _)| (label, value) == (label_of, value_of)),
        "{tab}: no line of {reference} among {lines:?}"
    );
    let mut step = 0;
    narrow_the_inspector(&state, visual, |visual, _| {
        step += 1;
        let at = format!("{tab}, step {step}");
        let (label, value) = (drawn(visual, label_of, &at), drawn(visual, value_of, &at));
        let mut rows = Vec::with_capacity(cells.len());
        for &(row_label, row_value, whole) in &cells {
            let (row, cell) = (drawn(visual, row_label, &at), drawn(visual, row_value, &at));
            let context = format!(
                "{at}: {row_label} at {row:?}, its value at {cell:?}, beside {reference}'s label at {label:?} and value at {value:?}"
            );
            assert!(
                near(row.left(), label.left()),
                "{context}: the label is off the label column"
            );
            assert!(
                near(cell.right(), value.right()),
                "{context}: the value does not end where the values end"
            );
            rows.push(drawn(visual, whole, &at));
        }
        for &selector in boxes {
            let spans = drawn(visual, selector, &at);
            assert!(
                near(spans.left(), label.left()) && near(spans.right(), value.right()),
                "{at}: {selector} spans {spans:?}, not the labels' start at {:?} to the values' end at {:?}",
                label.left(),
                value.right()
            );
        }
        let found = every_control_stands_on_the_columns(
            visual,
            &at,
            &controls,
            &rows,
            &[label.left()],
            value,
        );
        assert!(
            found > 0,
            "{at}: the Inspector draws none of the {} controls recorded before the walk",
            controls.len()
        );
        false
    });
    assert!(step > 0, "{tab}: the walk drew no step");
    (
        lines.into_iter().map(|line| line.label).collect(),
        choices,
        held,
    )
}

#[test]
fn a_choice_in_force_is_marked_and_no_other() {
    assert_eq!(&*picked("Best", true), "• Best");
    assert_eq!(&*picked("Best", false), "Best");
}

#[test]
fn every_tab_is_held_to_the_columns_or_named_as_not() {
    let mut shipped: Vec<&str> = super::host_of()
        .tabs()
        .iter()
        .map(|tab| tab.id.as_ref())
        .collect();
    let mut placed: Vec<&str> = HELD
        .into_iter()
        .chain(FREE)
        .chain(NOT_HELD.into_iter().map(|(tab, _)| tab))
        .collect();
    placed.sort_unstable();
    shipped.sort_unstable();
    assert_eq!(
        shipped, placed,
        "every Inspector tab is held to the columns here or named in NOT_HELD"
    );
}

#[gpui::test]
async fn every_transform_line_stands_in_the_columns(cx: &mut TestAppContext) {
    // Two properties keyed at the playhead, so the keyframes block draws two
    // rows, each with its navigator and easing presets.
    let (_, choices, held) = every_line_stands_in_the_columns(
        cx,
        ("texture.png", "transform"),
        ClipParam::Opacity,
        |state| {
            for param in [ClipParam::Opacity, ClipParam::Rotation] {
                keyframe_rows::apply(
                    state,
                    Row::Key {
                        param,
                        keyed: false,
                    },
                )
                .expect("key the property at the playhead");
            }
        },
    );
    // A clip with no parent shows None and marks the None chip.
    shows_and_marks("transform", &choices, "insp-label-Parent", "None");
    holds(
        "transform",
        &held,
        &[
            "Flatten".into(),
            PIECEWISE_PRESETS[0].1.into(),
            switch("flip-h"),
            switch("flip-v"),
            switch("blur-toggle"),
        ],
    );
}

#[gpui::test]
async fn every_audio_line_stands_in_the_columns(cx: &mut TestAppContext) {
    let role = AudioRole::ALL[0];
    let (lines, choices, held) = every_line_stands_in_the_columns(
        cx,
        ("fixture-sine-48k-2s.wav", "audio"),
        ClipParam::Volume,
        |state| {
            let clip = state.selected_clip().expect("the sound is selected");
            state
                .add_audio_insert(AudioInsertTarget::Clip(clip), "gain".into())
                .expect("add a gain insert");
            state
                .set_selected_audio_role(Some(role))
                .expect("tag the sound");
        },
    );
    assert!(
        lines.iter().any(|line| line == "insp-label-Gain (dB)"),
        "the insert's parameter drew no line: {lines:?}"
    );
    shows_and_marks("audio", &choices, "insp-label-Role", role.label());
    holds(
        "audio",
        &held,
        &[
            AudioRole::ALL[1].label().into(),
            "+ Audio effect".into(),
            switch("inspector-reverse"),
            switch("inspector-pitch"),
            switch("denoise-toggle"),
        ],
    );
}

#[gpui::test]
async fn every_look_line_stands_in_the_columns(cx: &mut TestAppContext) {
    // Registers and drops a transition every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let _registration = aria_models::register_transition(
            TransitionSpec::new(EVERY_KIND, "Every Kind").with_params(every_kind("transition")),
        )
        .expect("register the transition");
        let filter = crate::catalog_text::filters()
            .first()
            .expect("the catalog lists a filter");
        // Every layer style on, so each style's color and number rows are
        // drawn and held to the columns too, and a mask, a key and a matte, so
        // their rows and their Remove buttons are.
        let mut colors = Vec::new();
        let (lines, choices, held) = every_line_stands_in_the_columns(
            cx,
            ("texture.png", "look"),
            ClipParam::Look {
                param: LookParam::FilterIntensity,
            },
            |state| {
                let first = state.selected_clip().expect("the texture is selected");
                state
                    .set_selected_filter(filter.id.to_string())
                    .expect("apply a filter");
                crate::app::fixtures::blend_right_junction(state, first);
                state.select_clip(first);
                state
                    .set_clip_transition(first, EVERY_KIND)
                    .expect("apply the transition");
                for (style, _) in crate::app::clip_styles::STYLES {
                    state.toggle_selected_style(style).expect("a layer style");
                    let rows = state.param_colors(style);
                    assert!(!rows.is_empty(), "the {style} style has no color");
                    colors.extend(rows.into_iter().map(|row| (*style, row.param)));
                }
                for pick in [
                    Pick::Mask(MaskKind::ALL[0]),
                    Pick::Chroma(KEY_COLORS[0].2),
                    Pick::Matte(MatteSource::ALL[0]),
                ] {
                    inspector_pick::apply(state, pick).expect("dress the look");
                }
                state.sync_from_engine();
            },
        );
        for kind in EffectParamKind::ALL {
            let label = format!("insp-label-{} transition", kind.as_str());
            assert_eq!(
                lines.iter().any(|line| line.starts_with(&label)),
                a_transition_draws(kind),
                "the transition's {kind:?} slot: {lines:?}"
            );
        }
        for (style, param) in &colors {
            let label = format!("insp-label-{param:?}");
            assert!(
                lines.contains(&label),
                "the {style} style's color drew no line {label}: {lines:?}"
            );
        }
        shows_and_marks("look", &choices, "insp-label-Filter", &filter.label);
        shows_and_marks("look", &choices, "insp-label-choice transition", "Best");
        let (style, _) = crate::app::clip_styles::STYLES[0];
        holds(
            "look",
            &held,
            &[
                "Clear styles".into(),
                "Load .cube…".into(),
                "Remove transition".into(),
                "Remove key".into(),
                "Remove matte".into(),
                switch(("style-toggle", style)),
                switch("matte-invert"),
            ],
        );
    });
}

#[gpui::test]
async fn every_effects_line_stands_in_the_columns(cx: &mut TestAppContext) {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let _registration = aria_models::register_effect(EffectSpec {
            id: EVERY_KIND.into(),
            label: "Every Kind".into(),
            category: "Stylize".into(),
            params: every_kind("effect"),
            ..Default::default()
        })
        .expect("register the effect");
        let slot = |at: usize| ClipParam::Effect {
            effect: 0,
            param: at as u32,
        };
        let scalar = EffectParamKind::ALL
            .into_iter()
            .position(|kind| kind == EffectParamKind::Scalar)
            .expect("a scalar kind");
        let (lines, choices, held) = every_line_stands_in_the_columns(
            cx,
            ("texture.png", "effects"),
            slot(scalar),
            |state| {
                state
                    .add_effect_to_selected(EVERY_KIND)
                    .expect("add the effect");
            },
        );
        for (at, kind) in EffectParamKind::ALL.into_iter().enumerate() {
            let (keyed, named) = (
                format!("insp-label-{:?}", slot(at)),
                format!("insp-label-{} effect", kind.as_str()),
            );
            assert!(
                lines
                    .iter()
                    .any(|line| *line == keyed || line.starts_with(&named)),
                "the effect's {kind:?} slot drew no line: {lines:?}"
            );
        }
        // A layer slot left at None shows None and marks the None chip.
        shows_and_marks("effects", &choices, "insp-label-layer effect", "None");
        shows_and_marks("effects", &choices, "insp-label-choice effect", "Best");
        let boolean = EffectParamKind::ALL
            .into_iter()
            .position(|kind| kind == EffectParamKind::Bool)
            .expect("a bool kind");
        holds(
            "effects",
            &held,
            &[
                "Save as preset".into(),
                switch(MotionKey::new("inspector-fx-bypass", 0)),
                switch(MotionKey::new("fx-param-bool", 0).with(boolean)),
            ],
        );
    });
}

/// Drawn over a clip with a ramp, so its points, its length and Clear ramp
/// draw.
#[gpui::test]
async fn every_speed_line_stands_in_the_free_columns(cx: &mut TestAppContext) {
    let preset = speed_presets()
        .first()
        .expect("the model ships a speed preset");
    let (_, choices, held) = every_line_stands_in_the_free_columns(
        cx,
        ("texture.png", "speed"),
        ("Ramp", &["speed-graph"]),
        |state| {
            clip_pick::apply(state, ClipPick::SpeedPreset(preset.id.to_string()))
                .expect("ramp the clip");
        },
    );
    shows_and_marks("speed", &choices, "insp-label-free-Ramp", &preset.label);
    let mut wanted: Vec<String> = speed_presets()
        .iter()
        .map(|spec| spec.label.to_string())
        .collect();
    wanted.push("Clear ramp".into());
    holds("speed", &held, &wanted);
}

/// Drawn over a picture marked as a template slot with a hint, whose file is
/// gone, beside two videos to replace it with, so every fact and button of
/// the tab draws and the Replace media list draws more than one entry.
#[gpui::test]
async fn every_info_line_stands_in_the_free_columns(cx: &mut TestAppContext) {
    let staged = crate::scratch_pool::scratch("columns-info-media").join("gone.png");
    std::fs::copy(crate::app::fixtures::asset("texture.png"), &staged)
        .expect("stage a picture that can be lost");
    let media = staged.to_str().expect("a UTF-8 scratch path").to_owned();
    let others = ["fixture-640x360-24fps-3s", "fixture-640x360-30fps-3s"];
    let (lines, choices, held) = every_line_stands_in_the_free_columns(
        cx,
        (media.as_str(), "info"),
        ("Name", &[]),
        |state| {
            std::fs::remove_file(&staged).expect("lose the picture");
            crate::projection::invalidate_media_statuses();
            for other in others {
                state
                    .import_media(crate::app::fixtures::asset(&format!("{other}.mp4")))
                    .expect("a video to replace with, and the edit that rebuilds the pool");
            }
            clip_pick::apply(state, ClipPick::ToggleReplaceable).expect("mark the slot");
            state
                .set_selected_slot_label("Logo here")
                .expect("hint the slot");
            state.sync_from_engine();
            assert!(
                state.store.media.iter().any(|item| item.missing),
                "the pool still draws the lost picture as online, so Relink is not drawn"
            );
        },
    );
    for fact in ["Name", "Kind", "In", "Out", "Media", "Fill order", "Hint"] {
        assert!(
            lines
                .iter()
                .any(|line| *line == format!("insp-label-free-{fact}")),
            "info: {fact} drew no line: {lines:?}"
        );
    }
    // A new slot on a picture accepts the first restriction offered.
    let (_, accepts) = SLOT_MEDIA[0];
    shows_and_marks("info", &choices, "insp-label-free-Accepts", accepts);
    for other in others {
        assert!(
            held.iter().any(|label| label.starts_with(other)),
            "info: no button replaces the media with {other} among {held:?}"
        );
    }
    holds(
        "info",
        &held,
        &[
            "Relink".into(),
            accepts.into(),
            "Edit hint…".into(),
            "Clear hint".into(),
            switch("inspector-slot-toggle"),
        ],
    );
}
