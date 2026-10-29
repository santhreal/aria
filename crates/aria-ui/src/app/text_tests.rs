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

use aria_models::{ClipParam, TextAlignH, TextAlignV, TextCase, TextParam, text_effect_catalog};

use super::AppState;
use super::clip_text::TextSwitch;
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
    // than an error in the status line.
    let (mut state, _) = with_clip();
    assert!(!state.selected_is_title());
    state.set_selected_text("nope".into()).unwrap();
    state
        .set_selected_text_effect(&text_effect_catalog()[0].id)
        .unwrap();
    state.clear_selected_text_effect().unwrap();
    assert_eq!(state.selected_ui_clip().unwrap().text_effect, None);
}

/// Every switch the Character and Paragraph panels draw flips its own property
/// and no other, and flips it back, read off the projected style. A switch
/// wired to the wrong field reads back a neighbour changed. Enumerated from
/// `TextSwitch::ALL`, so a new switch is covered here with no edit.
#[test]
fn every_type_switch_flips_its_own_property_and_flips_back() {
    let mut state = with_title();
    for switch in TextSwitch::ALL {
        let before = state.selected_text_face().expect("a title is selected");
        state.toggle_selected_text_switch(switch).unwrap();
        let flipped = state.selected_text_face().expect("a title is selected");
        for other in TextSwitch::ALL {
            let expected = before.is_on(other) != (other == switch);
            assert_eq!(
                flipped.is_on(other),
                expected,
                "flipping {switch:?} left {other:?} wrong"
            );
        }
        state.toggle_selected_text_switch(switch).unwrap();
        assert_eq!(
            state.selected_text_face(),
            Some(before),
            "{switch:?} did not flip back"
        );
    }
}

/// A treatment switch takes the preset mark off, because the treatments no
/// longer read as that preset; a face or paragraph switch leaves it.
#[test]
fn a_treatment_switch_clears_the_preset_mark_and_no_other_switch_does() {
    let first = text_effect_catalog()[0].id.as_str();
    for switch in TextSwitch::ALL {
        let mut state = with_title();
        state.set_selected_text_effect(first).unwrap();
        state.toggle_selected_text_switch(switch).unwrap();
        let marked = state.selected_ui_clip().unwrap().text_effect.is_some();
        assert_eq!(
            marked,
            !TextSwitch::TREATMENTS.contains(&switch),
            "{switch:?}"
        );
    }
}

/// Case and both alignments read back every choice, and choosing the one in
/// force pushes no undo entry: one undo after a repeat lands on the choice
/// before it.
#[test]
fn case_and_alignment_read_back_and_a_repeat_is_inert() {
    let mut state = with_title();
    let face = |state: &AppState| state.selected_text_face().expect("a title is selected");
    for case in TextCase::ALL {
        state.set_selected_text_case(case).unwrap();
        assert_eq!(face(&state).case, case);
    }
    for align in TextAlignH::ALL {
        state.set_selected_text_align_h(align).unwrap();
        assert_eq!(face(&state).align_h, align);
    }
    for align in TextAlignV::ALL {
        state.set_selected_text_align_v(align).unwrap();
        assert_eq!(face(&state).align_v, align);
    }
    let [.., earlier, last] = TextAlignV::ALL;
    state.set_selected_text_align_v(last).unwrap();
    assert!(state.undo());
    assert_eq!(
        face(&state).align_v,
        earlier,
        "a repeated alignment pushed an undo entry"
    );
}

/// A swatch writes the colour through the catalog's property path and reads
/// back through the rows the panel draws; the colour in force is inert, and the
/// stroke's colour row arrives with the stroke.
#[test]
fn a_colour_swatch_sets_the_colour_and_a_repeat_is_inert() {
    let mut state = with_title();
    let colour = |state: &AppState, param: ClipParam| {
        state
            .param_colors("text")
            .into_iter()
            .find(|row| row.param == param)
            .map(|row| row.value)
    };
    let fill = ClipParam::Text {
        param: TextParam::Fill,
    };
    let stroke = ClipParam::Text {
        param: TextParam::StrokeColor,
    };
    let held = colour(&state, fill).expect("a title draws its fill");
    assert_eq!(colour(&state, stroke), None, "no stroke, no stroke color");
    let picked = crate::app::clip_effects::PARAM_COLORS
        .iter()
        .map(|(_, _, rgba)| *rgba)
        .find(|rgba| *rgba != held)
        .expect("a swatch other than the fill");
    state.set_param_color(fill, picked).unwrap();
    assert_eq!(colour(&state, fill), Some(picked));
    state.set_param_color(fill, picked).unwrap();
    assert!(state.undo());
    assert_eq!(
        colour(&state, fill),
        Some(held),
        "a repeated swatch pushed an undo entry"
    );
    state
        .toggle_selected_text_switch(TextSwitch::Stroke)
        .unwrap();
    assert!(
        colour(&state, stroke).is_some(),
        "switching the stroke on drew no stroke color"
    );
}

/// Window > Character and Window > Paragraph dock panels that draw the
/// selected title's controls, and a second press closes each.
#[gpui::test]
fn the_character_and_paragraph_panels_dock_from_the_window_menu(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "text-panels");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .add_generated(aria_models::Generator::text("Hello"))
                .expect("place a title");
            let title = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .find(|clip| clip.generator.is_some())
                .map(|clip| aria_models::ClipId::from_raw(clip.id))
                .expect("the title landed");
            state.select_clip(title);
            cx.notify();
        })
    });
    for (panel, drawn) in [
        (crate::dock::PANEL_CHARACTER, "character-panel"),
        (crate::dock::PANEL_PARAGRAPH, "paragraph-panel"),
    ] {
        let row = format!("{}{panel}", crate::chrome::menu_window::PANEL_PREFIX);
        let press = |visual: &mut gpui::VisualTestContext| {
            let found = visual.update(|_, app| {
                state.update(app, |state, cx| {
                    crate::chrome::menu_items::run_item(
                        crate::types::AppMenu::Window,
                        &row,
                        state,
                        cx,
                    )
                })
            });
            assert!(found, "Window has no row for {panel}");
            visual.run_until_parked();
            visual.read(|app| state.read(app).dock_tree.is_open(panel))
        };
        if visual.read(|app| state.read(app).dock_tree.is_open(panel)) {
            assert!(!press(visual), "{panel} did not close");
        }
        assert!(press(visual), "{panel} did not open");
        assert!(
            visual.debug_bounds(drawn).is_some(),
            "{panel} is open and not drawn"
        );
        assert!(!press(visual), "{panel} did not close");
        assert!(
            visual.debug_bounds(drawn).is_none(),
            "{panel} is closed and still drawn"
        );
    }
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
        let before = state.store.sequence.clip_count();
        state
            .add_generated(aria_models::Generator::sticker(&spec.id))
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert_eq!(
            state.store.sequence.clip_count(),
            before + 1,
            "{} placed no clip",
            spec.id
        );
    }
}
