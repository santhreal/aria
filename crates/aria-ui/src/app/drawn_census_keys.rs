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
//! Two of the six move the playhead rather than the project, so this census
//! splits its assertion on `Row::edits` rather than on a list of exceptions: an
//! editing button must send a command, and a navigating one must move the
//! playhead and send nothing.

use std::collections::BTreeSet;

use aria_models::ClipParam;

use crate::app::AppState;
use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};
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

    // Both faces of the toggle: the button is drawn from the state the operator
    // saw, so the census presses it on a row without a keyframe and on one with.
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

#[test]
fn the_census_reaches_every_keyframe_command() {
    let sent = dispatched();
    for label in [
        "Set keyframe",
        "Remove keyframe",
        "Set parameter",
        "Apply easing",
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

/// The key button is a toggle on the state the operator saw: pressing it twice
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
/// the press is inert rather than an error: the operator can press it on a row
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
