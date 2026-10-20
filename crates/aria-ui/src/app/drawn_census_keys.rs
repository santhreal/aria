//! Driving the keyframe list: every button on a property's row, pressed on a
//! curve dressed so the press has something to shape.
//!
//! # Why
//!
//! The rows already came from `key_params`, so the list offers what the selected
//! clip can animate and nothing else. The six buttons on each row were closures,
//! and one of them held a decision the panel should never have owned: the key
//! button adds a keyframe or removes the one under the playhead. That branch now
//! lives in [`keyframe_rows`](crate::surfaces::keyframe_rows) with the rest.
//!
//! Two of them move the playhead rather than the project, so this census
//! splits its assertion on `Row::edits` rather than on a list of exceptions: an
//! editing button must send a command, and a navigating one must move the
//! playhead and send nothing.

use std::collections::BTreeSet;

use aria_models::{ClipParam, ParamValue, TemporalEase};

use crate::app::AppState;
use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};
use crate::app::fixtures::{the_pair, twin_selected};
use crate::app::sent_census::Watch;
use crate::surfaces::keyframe_rows::{self, Row};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "keyframe_rows.rs";

/// One press, with a curve dressed so it has something to act on.
struct Staged {
    row: Row,
    dress: fn() -> AppState,
}

/// Every press the list offers, over the presets the model ships.
fn staged() -> Vec<Staged> {
    let param = a_param();
    let mut out = Vec::new();
    let mut push = |row: Row, dress: fn() -> AppState| out.push(Staged { row, dress });

    // Both faces of the toggle: the button is drawn from the state on screen,
    // so the census presses it on a row without a keyframe and on one with.
    push(
        Row::Key {
            param,
            keyed: false,
        },
        a_clip,
    );
    push(Row::Key { param, keyed: true }, a_keyed_curve);
    for preset in easing_presets() {
        push(
            Row::Easing {
                param,
                preset: preset.id.as_str(),
            },
            a_keyed_curve,
        );
    }
    for (preset, _) in PIECEWISE_PRESETS {
        push(Row::Piecewise { param, preset }, a_segment);
    }
    push(Row::Flatten(param), a_keyed_curve);
    push(Row::Previous(param), a_curve_with_a_key_behind);
    push(Row::Next(param), a_curve_with_a_key_ahead);
    // Both faces of the driver toggle: adding one, and removing it again.
    push(Row::Driver(param), a_clip_with_drivers);
    push(Row::Driver(param), a_driven_property);
    push(
        Row::TemporalEase {
            param,
            in_ease: Some(TemporalEase::EASY_EASE),
            out_ease: Some(TemporalEase::EASY_EASE),
            hold: false,
        },
        a_keyed_curve,
    );
    push(
        Row::Roving {
            param,
            roving: true,
        },
        an_interior_keyed_curve,
    );
    // Both landings of a reset: a constant, and a keyframe on a curve.
    push(Row::Reset(param), a_moved_property);
    push(Row::Reset(param), a_moved_animated_property);
    out
}

/// Every command the keyframe list asked for, one press per staged row.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in staged() {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        let what = stage.row.what();
        keyframe_rows::apply(&mut state, stage.row)
            .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_press_the_row_offers_is_driven() {
    let driven: BTreeSet<String> = staged()
        .iter()
        .map(|stage| stage.row.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !driven.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "the keyframe row offers presses no dressed curve drives: {missing:?}"
    );
}

/// The split the registry declares: a button that edits sends a command, and a
/// button that navigates moves the playhead and sends none.
#[test]
fn an_editing_press_sends_and_a_navigating_press_moves_the_playhead() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let before = state.playhead.value;
        let watch = Watch::on(&state);
        let what = stage.row.what();
        let edits = stage.row.edits();
        keyframe_rows::apply(&mut state, stage.row)
            .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
        let sent = watch.take();
        if edits {
            assert!(
                !sent.is_empty(),
                "{what} claims to edit and sent nothing, so either the button is \
                 inert or its curve is dressed with nothing to shape"
            );
        } else {
            assert!(
                sent.is_empty(),
                "{what} claims to navigate but sent {sent:?}"
            );
            assert_ne!(
                state.playhead.value, before,
                "{what} claims to navigate and left the playhead at {before}"
            );
        }
    }
}

/// With a second clip selected that holds the same curve, every press leaves
/// the two clips alike, in at most one undo step, and that step takes the
/// press back on both. The presses are the staged ones, so a button added to
/// the row that edits the drawn clip alone is red here with no edit to this
/// test.
#[test]
fn every_press_reaches_a_second_selected_clip_holding_the_same_curve() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let (drawn, twin) = twin_selected(&mut state);
        let before = the_pair(&state, drawn, twin);
        assert_eq!(
            before.1, before.0,
            "the copy differs from the clip it copies"
        );
        let depth = state.history_depth();
        let what = stage.row.what();
        keyframe_rows::apply(&mut state, stage.row)
            .unwrap_or_else(|err| panic!("pressing {what} was refused: {err}"));
        let (after, twin_after) = the_pair(&state, drawn, twin);
        assert_eq!(
            twin_after, after,
            "{what} edited the drawn clip and not the other selected clip"
        );
        let steps = state.history_depth() - depth;
        assert!(steps <= 1, "{what} on two clips took {steps} undo steps");
        if steps == 1 {
            assert!(state.undo(), "{what} left nothing to undo");
            assert_eq!(
                the_pair(&state, drawn, twin),
                before,
                "one undo did not take {what} back on both clips"
            );
        }
    }
}

#[test]
fn the_census_reaches_every_keyframe_command() {
    let sent = dispatched();
    for label in [
        "Set keyframe",
        "Remove keyframe",
        "Set parameter",
        "Apply easing",
        "Set driver",
        "Set keyframe ease",
        "Set roving keyframe",
    ] {
        assert!(
            sent.contains(label),
            "the keyframe list sent {sent:?}, which does not include `{label}`"
        );
    }
}

#[test]
fn the_keyframe_list_calls_no_setter_of_its_own() {
    super::no_setter_escapes(REGISTRY, "inspector");
}

/// The key button is a toggle on the state on screen: pressing it twice
/// leaves the curve as it was found.
#[test]
fn the_key_button_takes_a_keyframe_and_takes_it_back() {
    let param = a_param();
    let mut state = a_clip();
    let before = state.keyframes_of(param).len();

    keyframe_rows::apply(
        &mut state,
        Row::Key {
            param,
            keyed: false,
        },
    )
    .expect("taking a keyframe");
    assert_eq!(
        state.keyframes_of(param).len(),
        before + 1,
        "the key button did not take a keyframe"
    );

    keyframe_rows::apply(&mut state, Row::Key { param, keyed: true }).expect("taking it back");
    assert_eq!(
        state.keyframes_of(param).len(),
        before,
        "the key button did not remove the keyframe under the playhead"
    );
}

/// An easing preset shapes the keyframe under the playhead, so with none there
/// the press is inert rather than an error: it can be pressed on a row
/// whose keyframe an undo has just removed.
#[test]
fn shaping_a_segment_with_no_keyframe_under_the_playhead_is_inert() {
    let param = a_param();
    let preset = easing_presets()
        .first()
        .expect("the model ships an easing preset")
        .id
        .as_str();
    let mut state = a_clip();
    let watch = Watch::on(&state);
    keyframe_rows::apply(&mut state, Row::Easing { param, preset }).expect("an inert press");
    let sent = watch.take();
    assert!(
        sent.is_empty(),
        "easing a segment with no keyframe under the playhead sent {sent:?}"
    );
}

/// A reset puts a constant property back to its default with no curve, and an
/// animated one back to its default at the playhead without dropping the rest
/// of its curve. A second press has nothing left to do.
#[test]
fn the_reset_button_returns_a_property_to_its_default_where_an_edit_lands() {
    let param = a_param();

    let mut constant = a_moved_property();
    keyframe_rows::apply(&mut constant, Row::Reset(param)).expect("resetting a constant");
    assert!(
        !constant.param_off_default(param),
        "a reset left the property at {:?}, not {:?}",
        constant.param_value(param),
        constant.param_default(param)
    );
    assert!(
        constant.keyframes_of(param).is_empty(),
        "resetting a constant property gave it a curve"
    );

    // Keyed a quarter and half of the way in, both off the default, with the
    // playhead on the second.
    let mut animated = a_moved_property();
    let (start, end) = span(&animated);
    for at in [start + (end - start) / 4, start + (end - start) / 2] {
        animated.set_playhead(at);
        animated
            .add_keyframe_at_playhead(param)
            .expect("a keyframe lands");
    }
    let keys = animated.keyframes_of(param).len();
    keyframe_rows::apply(&mut animated, Row::Reset(param)).expect("resetting a curve");
    assert!(
        !animated.param_off_default(param),
        "a reset on a curve left {:?} at the playhead",
        animated.param_value(param)
    );
    assert_eq!(
        animated.keyframes_of(param).len(),
        keys,
        "a reset on a curve added or dropped keyframes instead of keying the one \
         under the playhead"
    );
    animated.set_playhead(start + (end - start) / 4);
    assert!(
        animated.param_off_default(param),
        "a reset at the playhead moved the keyframe a quarter of the way in too"
    );

    animated.set_playhead(start + (end - start) / 2);
    let watch = Watch::on(&animated);
    keyframe_rows::apply(&mut animated, Row::Reset(param)).expect("an inert press");
    let sent = watch.take();
    assert!(
        sent.is_empty(),
        "resetting a property already at its default sent {sent:?}"
    );
}

/// The first property the list would draw a row for.
///
/// Read out of `key_params`, which is what the panel reads, so a lane that
/// refuses a property cannot be pressed here either.
fn a_param() -> ClipParam {
    a_clip()
        .key_params()
        .first()
        .expect("the selected clip can animate something")
        .param()
}

fn a_clip() -> AppState {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    let (start, end) = span(&state);
    state.set_playhead(start + (end - start) / 2);
    state
}

/// The fixture with the Expressions plugin mounted, so a driver has a language
/// to be written in.
fn a_clip_with_drivers() -> AppState {
    let state = a_clip();
    state
        .ctx
        .mount(&aria_plugin::ExpressionsPlugin)
        .expect("mount Expressions");
    state
}

/// A property that already has a driver, so the driver button removes it.
fn a_driven_property() -> AppState {
    let mut state = a_clip_with_drivers();
    state.toggle_driver(a_param()).expect("the driver lands");
    state
}

/// The property moved off its reset value, so the reset button has something
/// to put back.
fn a_moved_property() -> AppState {
    let mut state = a_clip();
    move_off_default(&mut state);
    state
}

/// An animated property moved off its reset value under the playhead, so the
/// reset lands as a keyframe.
fn a_moved_animated_property() -> AppState {
    let mut state = a_keyed_curve();
    move_off_default(&mut state);
    state
}

/// Move the property off its reset value where a row edit lands.
fn move_off_default(state: &mut AppState) {
    let param = a_param();
    let default = state
        .param_default(param)
        .expect("the property has a reset value");
    state
        .set_param_value(param, elsewhere(default))
        .expect("the property moves");
    assert!(
        state.param_off_default(param),
        "moving the property left it at its reset value"
    );
}

/// `value` moved to somewhere it was not, inside any range a property holds.
fn elsewhere(value: ParamValue) -> ParamValue {
    let half = |v: f32| if v == 0.0 { 0.25 } else { v * 0.5 };
    match value {
        ParamValue::Scalar(v) => ParamValue::Scalar(half(v)),
        ParamValue::Vec2([x, y]) => ParamValue::Vec2([half(x), half(y)]),
        ParamValue::Vec3([x, y, z]) => ParamValue::Vec3([half(x), half(y), half(z)]),
        ParamValue::Color([r, g, b, a]) => ParamValue::Color([255 - r, 255 - g, 255 - b, a]),
        ParamValue::Rect([x, y, w, h]) => ParamValue::Rect([x, y, w * 0.5, h * 0.5]),
    }
}

/// The selected clip's first and last tick on the timeline.
fn span(state: &AppState) -> (i64, i64) {
    let clip = state
        .selected_ui_clip()
        .expect("the fixture selects a clip");
    (clip.timeline_start_ticks, clip.timeline_end_ticks)
}

/// A keyframe exactly under the playhead, which is what an easing preset, a
/// piecewise preset and the flatten button all act on.
fn a_keyed_curve() -> AppState {
    let mut state = a_clip();
    let param = a_param();
    state
        .add_keyframe_at_playhead(param)
        .expect("the first keyframe lands");
    state
}

/// Two keyframes with the playhead on the first, which is the shape a piecewise
/// preset needs: it rewrites the segment between the key under the playhead and
/// the one after it, so a lone keyframe has no segment to fill.
fn a_segment() -> AppState {
    let mut state = keyed_at(3, 4);
    let (start, end) = span(&state);
    let middle = start + (end - start) / 2;
    state.set_playhead(middle);
    state
        .add_keyframe_at_playhead(a_param())
        .expect("the second keyframe lands");
    state
}

/// Three keyframes with the playhead on the middle one, which is the shape a
/// roving toggle needs (first and last keyframes are anchored).
fn an_interior_keyed_curve() -> AppState {
    let mut state = keyed_at(1, 4);
    let (start, end) = span(&state);
    state.set_playhead(start + (end - start) * 3 / 4);
    state
        .add_keyframe_at_playhead(a_param())
        .expect("the outer keyframe lands");
    state.set_playhead(start + (end - start) / 2);
    state
        .add_keyframe_at_playhead(a_param())
        .expect("the interior keyframe lands");
    state
}

/// A keyframe behind the playhead, so the previous arrow has somewhere to go.
fn a_curve_with_a_key_behind() -> AppState {
    let mut state = keyed_at(1, 2);
    let (start, end) = span(&state);
    state.set_playhead(start + (end - start) * 3 / 4);
    state
}

/// A keyframe ahead of the playhead, so the next arrow has somewhere to go.
fn a_curve_with_a_key_ahead() -> AppState {
    let mut state = keyed_at(3, 4);
    let (start, end) = span(&state);
    state.set_playhead(start + (end - start) / 2);
    state
}

/// A keyframe at `num/den` of the way through the clip.
fn keyed_at(num: i64, den: i64) -> AppState {
    let mut state = a_clip();
    let (start, end) = span(&state);
    state.set_playhead(start + (end - start) * num / den);
    state
        .add_keyframe_at_playhead(a_param())
        .expect("the keyframe lands");
    state
}
