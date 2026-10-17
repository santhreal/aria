//! Edit > Remove Attributes from the editor: what the dialog offers, what a
//! tick commits, and that the commit is one undo step.
//!
//! Closes the class where the dialog offers a group the clip does not hold
//! (a reset that does nothing), removes an unticked group, or lands one entry
//! per group so Undo brings back only part of it. Which properties each group
//! holds is `aria-models`' `tests_attributes.rs`; the engine's inverse is
//! `aria-engine/tests/remove_attributes.rs`.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipAttribute, ClipId, ClipParam, ParamValue};

use crate::app::premiere_tests::with_clip;
use crate::app::{AppState, DialogKind};

/// The groups `clip` holds, read from the engine.
fn held(state: &AppState, clip: ClipId) -> Vec<ClipAttribute> {
    let engine = state.engine.lock();
    let clip = engine.project().clip(clip).expect("the clip exists");
    ClipAttribute::ALL
        .into_iter()
        .filter(|attribute| attribute.is_set(clip))
        .collect()
}

/// One selected clip with its opacity lowered and a blur applied.
fn dressed() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    state
        .apply_command(Command::Edit(EditCommand::SetParamConstant {
            clip,
            param: ClipParam::Opacity,
            value: ParamValue::Scalar(0.5),
        }))
        .expect("opacity");
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".into(),
        }))
        .expect("effect");
    state.sync_from_engine();
    state.select_clip(clip);
    (state, clip)
}

#[test]
fn the_dialog_offers_only_the_groups_the_clip_holds_each_ticked() {
    let (mut state, clip) = dressed();
    assert_eq!(
        held(&state, clip),
        [ClipAttribute::Opacity, ClipAttribute::Effects]
    );

    state.open_remove_attributes();

    assert_eq!(state.dialog, DialogKind::RemoveAttributes);
    assert_eq!(
        state.remove_attributes.rows,
        [
            (ClipAttribute::Opacity, true),
            (ClipAttribute::Effects, true)
        ]
    );
}

#[test]
fn an_unticked_group_stays_and_the_ticked_one_goes_in_one_undo_step() {
    let (mut state, clip) = dressed();
    state.open_remove_attributes();
    state.toggle_remove_attribute(ClipAttribute::Effects);

    state.apply_remove_attributes();

    assert_eq!(state.dialog, DialogKind::None, "a commit closes the dialog");
    assert_eq!(held(&state, clip), [ClipAttribute::Effects]);
    assert!(state.undo(), "the removal is on the history");
    assert_eq!(
        held(&state, clip),
        [ClipAttribute::Opacity, ClipAttribute::Effects],
        "one undo brings the removed group back"
    );
}

#[test]
fn every_ticked_group_is_removed_and_one_undo_restores_them_all() {
    let (mut state, clip) = dressed();
    state.open_remove_attributes();

    state.apply_remove_attributes();

    assert!(held(&state, clip).is_empty(), "every ticked group went");
    assert!(state.undo());
    assert_eq!(
        held(&state, clip),
        [ClipAttribute::Opacity, ClipAttribute::Effects],
        "a removal of two groups landed as more than one undo step"
    );
}

#[test]
fn nothing_ticked_keeps_the_dialog_open_and_the_clip_as_it_was() {
    let (mut state, clip) = dressed();
    state.open_remove_attributes();
    state.toggle_remove_attribute(ClipAttribute::Opacity);
    state.toggle_remove_attribute(ClipAttribute::Effects);

    state.apply_remove_attributes();

    assert_eq!(state.dialog, DialogKind::RemoveAttributes);
    assert_eq!(
        held(&state, clip),
        [ClipAttribute::Opacity, ClipAttribute::Effects]
    );
    assert!(
        state.status_message.is_some(),
        "the refusal is not reported"
    );
}

#[test]
fn a_clip_holding_nothing_opens_no_dialog_and_says_why() {
    let (mut state, _clip) = with_clip();

    state.open_remove_attributes();

    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.status_message.is_some());
}

#[test]
fn an_empty_selection_opens_no_dialog_and_says_why() {
    let (mut state, _clip) = dressed();
    state.selection.clear();

    state.open_remove_attributes();

    assert_eq!(state.dialog, DialogKind::None);
    assert!(state.status_message.is_some());
}
