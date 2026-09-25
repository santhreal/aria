//! Contracts for title text and the text-effect catalog.
//!
//! The class closed here is the recurring one: `SetGenerator` carries the whole
//! generator, so an edit that rebuilds it from scratch silently discards every
//! sibling field. Retyping a title used to send `Generator::text(content)`, which
//! dropped the font, size, colour, alignment, stroke, shadow, background, and
//! preset. Both directions are asserted — a content edit keeps the preset, and a
//! preset edit keeps the content — because only asserting one lets the other rot.
//!
//! Presets are enumerated from `text_effect_catalog()` at run time, so a new entry
//! is exercised without a second list here.

use aria_models::text_effect_catalog;

use super::AppState;
use super::premiere_tests::with_clip;

/// A project whose selection is a title.
pub(super) fn with_title() -> AppState {
    let (mut state, _) = with_clip();
    state
        .add_generated(aria_models::Generator::text("Hello"))
        .expect("place a title");
    let title = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| {
            clip.generator
                .as_ref()
                .is_some_and(|generator| generator.kind == "text")
        })
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .expect("the title landed");
    state.select_clip(title);
    assert!(state.selected_is_title());
    state
}

fn content(state: &AppState) -> String {
    state
        .selected_ui_clip()
        .and_then(|clip| clip.generator)
        .map(|generator| generator.content.to_string())
        .expect("a title is selected")
}

#[test]
fn every_catalog_preset_applies_reads_back_and_clears() {
    let mut state = with_title();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
    assert!(!text_effect_catalog().is_empty());

    for spec in text_effect_catalog() {
        state
            .set_selected_text_effect(&spec.id)
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert_eq!(
            state.selected_ui_clip().unwrap().text_effect.as_deref(),
            Some(spec.id.as_str()),
            "{} did not read back",
            spec.id
        );
    }

    state.clear_selected_text_effect().unwrap();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
}

#[test]
fn retyping_a_title_keeps_its_preset() {
    let mut state = with_title();
    let first = text_effect_catalog()[0].id.as_str();
    state.set_selected_text_effect(first).unwrap();

    state.set_selected_text("Second line".into()).unwrap();
    assert_eq!(content(&state), "Second line");
    assert_eq!(
        state.selected_ui_clip().unwrap().text_effect.as_deref(),
        Some(first),
        "the content edit flattened the style"
    );
}

#[test]
fn applying_a_preset_keeps_the_text() {
    let mut state = with_title();
    state.set_selected_text("Kept".into()).unwrap();
    for spec in text_effect_catalog() {
        state.set_selected_text_effect(&spec.id).unwrap();
        assert_eq!(
            content(&state),
            "Kept",
            "{} replaced the title's content",
            spec.id
        );
    }
    state.clear_selected_text_effect().unwrap();
    assert_eq!(content(&state), "Kept");
}

#[test]
fn a_no_op_text_edit_leaves_no_undo_entry() {
    // Draining undo here would delete the title and with it the selection, so the
    // depth is probed instead: one undo after two no-ops must land on the state
    // before the single real edit.
    let mut state = with_title();
    let first = text_effect_catalog()[0].id.as_str();
    state.set_selected_text_effect(first).unwrap();

    let held = content(&state);
    state.set_selected_text(held).unwrap();
    state.set_selected_text_effect(first).unwrap();

    assert!(state.undo());
    assert_eq!(
        state.selected_ui_clip().unwrap().text_effect,
        None,
        "a no-op text edit pushed an undo entry"
    );
}

#[test]
fn text_controls_are_inert_on_a_clip_that_is_not_a_title() {
    // The media fixture is selected, not a title: every setter is a no-op rather
    // than an error the operator has to read.
    let (mut state, _) = with_clip();
    assert!(!state.selected_is_title());
    state.set_selected_text("nope".into()).unwrap();
    state
        .set_selected_text_effect(&text_effect_catalog()[0].id)
        .unwrap();
    state.clear_selected_text_effect().unwrap();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
}

#[test]
fn every_bundled_sticker_places_a_clip_that_the_model_accepts() {
    // The Stickers tab is generated from this catalog, so a bundled asset the model
    // would refuse would be a dead tile. Each id gets its own project: placement is
    // at the playhead, so placing the whole catalog into one lane would fail on
    // overlap and prove nothing about the asset.
    let catalog = aria_models::sticker_catalog();
    assert!(!catalog.is_empty());

    for spec in catalog {
        let (mut state, _) = with_clip();
        let before = clip_count(&state);
        state
            .add_generated(aria_models::Generator::sticker(&spec.id))
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert_eq!(clip_count(&state), before + 1, "{} placed no clip", spec.id);
    }
}

fn clip_count(state: &AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum()
}
