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
//! Not covered: a row drawn without the row widgets, the tabs in [`NOT_HELD`],
//! a picker drawn outside a choice row, and a line whose label another line in
//! the keyed columns shares, of which the last drawn is read.

use aria_models::{
    AudioInsertTarget, AudioRole, ClipParam, EffectChoiceOption, EffectCurvePoint,
    EffectGradientStop, EffectParamKind, EffectParamSpec, EffectSpec, LookParam, TransitionSpec,
};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

use super::blocks::widgets::{
    DrawnChoice, LABEL_RESERVE, MIXED, drawn_choices, drawn_lines, picked, record_lines,
};
use super::strip_tests::{drawn_with, middle, narrow_the_inspector};
use crate::app::AppState;

/// How far apart two edges may be and still stand in one column.
const EPS: f32 = 0.51;

/// The id of the effect and of the transition with a slot of every kind.
const EVERY_KIND: &str = "test.ui.every_kind";

/// The tabs held to the columns here.
const HELD: [&str; 4] = ["transform", "look", "effects", "audio"];

/// The tabs not held to the columns, and why.
const NOT_HELD: [(&str, &str); 4] = [
    (
        "speed",
        "its rows draw no time area and stand in free columns",
    ),
    (
        "info",
        "its rows draw no time area and stand in free columns",
    ),
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

/// Draw the shell with the fixture `media` selected and the Inspector on
/// `tab`, after `then` edits the state, then narrow the Inspector a step at a
/// time. Every line drawn into the Inspector stands in the keyed rows'
/// columns, and at every step its label starts where the label of
/// `reference`, a keyed row with a time area, starts and is as wide, and its
/// value stands in that row's value column, on the label's line. The walk
/// ends once the reference's label gives up width, which it must reach. Every
/// choice row drawn marks the choice it shows. Returns the debug selectors of
/// the lines' labels, and the choice rows.
fn every_line_stands_in_the_columns(
    cx: &mut TestAppContext,
    (media, tab): (&str, &str),
    reference: ClipParam,
    then: impl FnOnce(&mut AppState),
) -> (Vec<String>, Vec<DrawnChoice>) {
    record_lines();
    let (state, visual) = drawn_with(cx, &format!("columns-{tab}"), media, tab, then);
    let (lines, choices) = (drawn_lines(), drawn_choices());
    every_choice_marks_what_it_shows(tab, &choices);
    let free: Vec<&str> = lines
        .iter()
        .filter(|(_, keyed)| !keyed)
        .map(|(label, _)| label.as_str())
        .collect();
    assert!(
        free.is_empty(),
        "{tab}: lines drawn outside the keyed rows' columns: {free:?}"
    );
    let leak = |text: String| -> &'static str { text.leak() };
    let cells: Vec<(&'static str, &'static str)> = lines
        .iter()
        .map(|(label, _)| {
            (
                leak(label.clone()),
                leak(label.replacen("insp-label-", "insp-value-", 1)),
            )
        })
        .collect();
    let (label_of, value_of) = (
        leak(format!("insp-label-{reference:?}")),
        leak(format!("insp-value-{reference:?}")),
    );
    assert!(
        cells.contains(&(label_of, value_of)),
        "{tab}: no line of {reference:?} among {lines:?}"
    );
    let strip = leak(format!("insp-strip-{reference:?}"));
    let (mut step, mut yielded) = (0, false);
    narrow_the_inspector(&state, visual, |visual, _| {
        step += 1;
        let at = format!("{tab}, step {step}");
        assert!(
            visual.debug_bounds(strip).is_some(),
            "{at}: {reference:?} draws no time area, so its columns are no keyed row's"
        );
        let (label, value) = (drawn(visual, label_of, &at), drawn(visual, value_of, &at));
        for &(row_label, row_value) in &cells {
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
        }
        yielded = f32::from(label.size.width) < LABEL_RESERVE - 1.0;
        yielded
    });
    assert!(
        yielded,
        "{tab}: the walk ended before {reference:?}'s label gave up width"
    );
    (lines.into_iter().map(|(label, _)| label).collect(), choices)
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
    every_line_stands_in_the_columns(cx, ("texture.png", "transform"), ClipParam::Opacity, |_| {});
}

#[gpui::test]
async fn every_audio_line_stands_in_the_columns(cx: &mut TestAppContext) {
    let role = AudioRole::ALL[0];
    let (lines, choices) = every_line_stands_in_the_columns(
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
        // drawn and held to the columns too.
        let mut colors = Vec::new();
        let (lines, choices) = every_line_stands_in_the_columns(
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
        let (lines, choices) = every_line_stands_in_the_columns(
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
    });
}
