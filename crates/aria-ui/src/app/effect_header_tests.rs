//! The effect header's verbs, driven through the calls its menu rows make.
//!
//! The class closed here is a verb that lands on the wrong effect, writes half
//! of an edit, or spends an undo step it did not earn. Each verb runs on a chain
//! of two effects and the chain is read back whole from the model, so a verb
//! that touched the effect beside its target is red. A verb that writes several
//! commands is checked to be one named undo step that undoes whole, and a verb
//! with nothing to act on is checked to write nothing.
//!
//! Not covered: drawing the header and opening its menu, which needs a window.
//! The menu's rows are pressed by `drawn_census_menu`, and the rename prompt by
//! `drawn_census_rename`.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, EffectInstance};

use super::effect_header::{DUPLICATE_EFFECT_LABEL, PASTE_EFFECT_LABEL};
use crate::app::AppState;
use crate::app::fixtures::{blended_junction, blurred_clip, linked_pair};
use crate::surfaces::context_menu::{EFFECT_ITEMS, EffectMenu, MenuItem, MenuRow};
use crate::types::RenameTarget;

/// The model's chain on `clip`, whole.
fn chain(state: &AppState, clip: ClipId) -> Vec<EffectInstance> {
    let engine = state.engine().lock();
    engine
        .project()
        .timeline()
        .clip(clip)
        .map(|model| model.effects.clone())
        .unwrap_or_default()
}

/// The names the chain on `clip` draws, in render order.
fn names(state: &AppState, clip: ClipId) -> Vec<String> {
    chain(state, clip)
        .iter()
        .map(EffectInstance::display_name)
        .collect()
}

/// The blurred fixture clip with a vignette after the blur, so every verb on
/// one effect has a neighbour it must leave alone.
fn two_effects() -> (AppState, ClipId) {
    let (mut state, clip) = blurred_clip();
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "vignette".into(),
        }))
        .expect("add a vignette");
    state.sync_from_engine();
    (state, clip)
}

/// The effect menu's row `id`.
fn row(id: &str) -> &'static MenuRow<EffectMenu> {
    EFFECT_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("the effect menu has no {id} row"))
}

/// Whether the menu opened on the effect at `index` offers the row `id`.
fn offered(state: &AppState, clip: ClipId, index: usize, id: &str) -> bool {
    (row(id).when)(&EffectMenu::of(state, clip.raw(), index))
}

/// Press the effect menu row `id` on the effect at `index`, with the facts
/// the menu would gather.
fn press(state: &mut AppState, clip: ClipId, index: usize, id: &str) {
    assert!(offered(state, clip, index, id), "{id} is not offered here");
    let facts = EffectMenu::of(state, clip.raw(), index);
    (row(id).run)(state, &facts).unwrap_or_else(|err| panic!("{id} was refused: {err}"));
}

#[test]
fn reset_puts_the_parameters_back_and_keeps_the_name_and_the_switch() {
    let (mut state, clip) = two_effects();
    state.rename_effect(clip, 0, "Soft").expect("rename");
    state.toggle_selected_effect_bypass(0).expect("switch off");
    let before = chain(&state, clip);
    assert!(
        !before[0].params.is_empty(),
        "the fixture turned the radius up"
    );
    let depth = state.history_depth();

    press(&mut state, clip, 0, "ctx-fx-reset");
    let after = chain(&state, clip);
    assert!(after[0].params.is_empty(), "the radius was not put back");
    assert_eq!(after[0].name.as_deref(), Some("Soft"));
    assert!(after[0].bypass, "a reset switched the effect back on");
    assert_eq!(after[1], before[1], "the reset touched the vignette");
    assert_eq!(state.history_depth(), depth + 1);

    state.reset_effect(clip, 0).expect("a second reset");
    assert_eq!(
        state.history_depth(),
        depth + 1,
        "a reset with nothing to reset spent a step"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Soft is already at its defaults")
    );
}

#[test]
fn a_name_is_drawn_until_it_is_taken_off() {
    let (mut state, clip) = two_effects();
    state.rename_effect(clip, 0, "  Soft  ").expect("rename");
    state.sync_from_engine();
    assert_eq!(chain(&state, clip)[0].name.as_deref(), Some("Soft"));
    let drawn = state.selected_ui_clip().expect("the clip is drawn");
    assert_eq!(drawn.effects[0].name.to_string(), "Soft");
    assert_eq!(drawn.effects[1].name.to_string(), "Vignette");

    // The catalog's own label and a blank name both take the name off, so
    // the header follows the label again.
    for given in ["Gaussian Blur", "   "] {
        state.rename_effect(clip, 0, "Soft").expect("rename");
        state
            .rename_effect(clip, 0, given)
            .expect("take the name off");
        assert_eq!(chain(&state, clip)[0].name, None, "{given:?} kept a name");
    }

    let depth = state.history_depth();
    state
        .rename_effect(clip, 0, "Gaussian Blur")
        .expect("an unchanged name");
    assert_eq!(
        state.history_depth(),
        depth,
        "an unchanged name spent a step"
    );

    let refused = state.rename_effect(clip, 0, "Soft\u{7}");
    assert!(
        refused.is_err(),
        "a name with a control character was taken"
    );
    assert_eq!(chain(&state, clip)[0].name, None);
}

#[test]
fn the_rename_bar_takes_a_name_as_long_as_it_can_type() {
    // The bar bounds a draft by characters. A name the bar lets through and
    // the model refuses would drop what was typed, so the longest draft is
    // committed in characters that take two bytes each.
    let (mut state, clip) = two_effects();
    let target = RenameTarget::Effect {
        clip: clip.raw(),
        index: 1,
    };
    state.begin_rename(target);
    assert_eq!(
        state.rename_draft, "Vignette",
        "the draft is seeded with the drawn name"
    );
    while !state.rename_draft.is_empty() {
        state.rename_pop();
    }
    for _ in 0..crate::app::rename::RENAME_MAX_LEN * 2 {
        state.rename_push('é');
    }
    state.commit_rename().expect("the longest draft is a name");
    let name = chain(&state, clip)[1]
        .name
        .clone()
        .expect("the name landed");
    assert_eq!(name.chars().count(), crate::app::rename::RENAME_MAX_LEN);
    assert!(name.chars().all(|ch| ch == 'é'));
}

#[test]
fn duplicate_lands_below_with_every_value_as_one_named_step() {
    let (mut state, clip) = two_effects();
    let before = chain(&state, clip);
    let depth = state.history_depth();

    press(&mut state, clip, 0, "ctx-fx-duplicate");
    let after = chain(&state, clip);
    assert_eq!(
        names(&state, clip),
        ["Gaussian Blur", "Gaussian Blur 2", "Vignette"]
    );
    assert_eq!(
        after[1],
        EffectInstance {
            name: Some("Gaussian Blur 2".into()),
            ..before[0].clone()
        },
        "the copy lost a value"
    );
    assert_eq!(state.history_depth(), depth + 1, "a duplicate is one step");
    assert_eq!(
        state.next_undo_label().as_deref(),
        Some(DUPLICATE_EFFECT_LABEL)
    );

    // A copy of a copy takes the next free number rather than a second one.
    state.duplicate_effect(clip, 1).expect("duplicate the copy");
    assert_eq!(
        names(&state, clip),
        [
            "Gaussian Blur",
            "Gaussian Blur 2",
            "Gaussian Blur 3",
            "Vignette"
        ]
    );

    assert!(state.undo());
    assert!(state.undo());
    assert_eq!(
        chain(&state, clip),
        before,
        "undo left part of a duplicate behind"
    );
}

#[test]
fn a_duplicate_name_is_numbered_within_the_limit() {
    let (mut state, clip) = two_effects();
    state.rename_effect(clip, 0, "Blur 2").expect("rename");
    state.duplicate_effect(clip, 0).expect("duplicate");
    assert_eq!(names(&state, clip)[1], "Blur 3");

    let long = "x".repeat(aria_models::EFFECT_NAME_MAX_CHARS);
    state
        .rename_effect(clip, 0, &long)
        .expect("the longest name");
    state
        .duplicate_effect(clip, 0)
        .expect("duplicate the longest name");
    let copy = &names(&state, clip)[1];
    assert_eq!(copy.chars().count(), aria_models::EFFECT_NAME_MAX_CHARS);
    assert!(copy.ends_with(" 2"), "{copy:?}");
}

#[test]
fn a_paste_lands_below_the_effect_the_menu_was_opened_on() {
    let (mut state, clip) = two_effects();
    assert!(
        !offered(&state, clip, 0, "ctx-fx-paste"),
        "paste was offered with nothing copied"
    );
    press(&mut state, clip, 0, "ctx-fx-copy");
    let copied = chain(&state, clip)[0].clone();
    let depth = state.history_depth();

    press(&mut state, clip, 0, "ctx-fx-paste");
    let after = chain(&state, clip);
    assert_eq!(after.len(), 3);
    assert_eq!(
        after[1], copied,
        "the paste lost a value or landed elsewhere"
    );
    assert_eq!(after[2].effect_id, "vignette");
    assert_eq!(state.history_depth(), depth + 1, "a paste is one step");
    assert_eq!(state.next_undo_label().as_deref(), Some(PASTE_EFFECT_LABEL));
}

#[test]
fn a_copied_effect_pastes_onto_another_clip() {
    let (mut state, left) = blended_junction();
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip: left,
            effect_id: "vignette".into(),
        }))
        .expect("add a vignette");
    state
        .apply_command(Command::Edit(EditCommand::SetEffectParam {
            clip: left,
            index: 0,
            param: 0,
            value: 0.9,
        }))
        .expect("turn the vignette up");
    state.sync_from_engine();
    let right = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != left)
        .expect("the junction fixture holds two clips");
    state.copy_effect(left, 0);
    state.select_clip(right);
    state.paste_effect_to_selection().expect("paste");
    assert_eq!(chain(&state, right), chain(&state, left));
}

#[test]
fn a_paste_onto_the_selection_passes_over_sound() {
    let mut state = linked_pair();
    let clips: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect();
    let kind = |state: &AppState, clip: ClipId| {
        let engine = state.engine().lock();
        let timeline = engine.project().timeline();
        timeline
            .track_of(clip)
            .and_then(|track| timeline.track(track))
            .map(|track| track.kind)
            .expect("every clip is on a lane")
    };
    let (visual, audio): (Vec<ClipId>, Vec<ClipId>) = clips
        .iter()
        .partition(|clip| kind(&state, **clip).is_visual());
    assert!(
        !visual.is_empty() && !audio.is_empty(),
        "the fixture holds both kinds"
    );

    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip: visual[0],
            effect_id: "gaussian_blur".into(),
        }))
        .expect("add a blur");
    state.copy_effect(visual[0], 0);
    state.selection = clips;
    let depth = state.history_depth();
    state.paste_effect_to_selection().expect("paste");

    assert_eq!(
        state.history_depth(),
        depth + 1,
        "a paste onto many clips is one step"
    );
    assert_eq!(chain(&state, visual[0]).len(), 2);
    for clip in &visual[1..] {
        assert_eq!(
            chain(&state, *clip).len(),
            1,
            "{clip:?} did not take the paste"
        );
    }
    for clip in &audio {
        assert!(
            chain(&state, *clip).is_empty(),
            "{clip:?} on a sound lane took a picture effect"
        );
    }
}

#[test]
fn a_paste_with_nothing_copied_writes_nothing() {
    let (mut state, clip) = two_effects();
    let depth = state.history_depth();
    state.paste_effect(clip, 0).expect("an empty paste");
    state.paste_effect_to_selection().expect("an empty paste");
    assert_eq!(state.history_depth(), depth);
    assert_eq!(
        state.status_message.as_deref(),
        Some("Copy an effect first")
    );
}

#[test]
fn save_as_preset_opens_the_draft_on_that_effect_alone() {
    let (mut state, clip) = two_effects();
    state.rename_effect(clip, 0, "Soft").expect("rename");
    state.clear_selection();
    press(&mut state, clip, 0, "ctx-fx-save-preset");
    assert_eq!(
        state.selected_clip(),
        Some(clip),
        "the draft opened over another clip"
    );
    let draft = state.preset_draft().cloned().expect("the draft opened");
    assert_eq!(draft.name, "Soft");
    assert_eq!(
        draft.effects,
        [
            (0, "Soft".to_string(), true),
            (1, "Vignette".to_string(), false)
        ]
    );
    assert!(!draft.include_audio_effects);
    assert!(draft.properties.iter().all(|(_, _, on)| !on));
    assert_eq!(state.inspector_tab, "effects");
}

/// The chain a header carried from `from` and dropped above `before` must
/// leave, worked out on names rather than indices: the carried effect lands
/// just above the effect it was dropped on, or last when it was dropped past
/// the chain, and dropping it on itself leaves it where it was.
fn dropped(chain: &[String], from: usize, before: Option<usize>) -> Vec<String> {
    let carried = chain[from].clone();
    let target = before.and_then(|at| chain.get(at)).cloned();
    if target.as_ref() == Some(&carried) {
        return chain.to_vec();
    }
    let mut rest: Vec<String> = chain
        .iter()
        .filter(|name| **name != carried)
        .cloned()
        .collect();
    let at = target
        .and_then(|name| rest.iter().position(|other| *other == name))
        .unwrap_or(rest.len());
    rest.insert(at, carried);
    rest
}

/// The fixture chain with a sharpen after the vignette, so a drop can land
/// between two effects as well as at either end.
fn three_effects() -> (AppState, ClipId) {
    let (mut state, clip) = two_effects();
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "sharpen".into(),
        }))
        .expect("add a sharpen");
    state.sync_from_engine();
    (state, clip)
}

#[test]
fn a_header_dropped_on_the_chain_lands_above_its_target_as_one_step() {
    let (probe, clip) = three_effects();
    let start = names(&probe, clip);
    assert_eq!(start.len(), 3);
    let befores = (0..=start.len()).map(Some).chain([None]);
    for before in befores {
        for from in 0..start.len() {
            let (mut state, clip) = three_effects();
            let depth = state.history_depth();
            state
                .move_effect_before(clip, from, before)
                .unwrap_or_else(|err| panic!("dropping {from} above {before:?}: {err}"));
            let want = dropped(&start, from, before);
            assert_eq!(
                names(&state, clip),
                want,
                "dropping {from} above {before:?}"
            );
            let moved = want != start;
            assert_eq!(
                state.history_depth(),
                depth + usize::from(moved),
                "dropping {from} above {before:?} spent the wrong number of steps"
            );
            if moved {
                assert!(state.undo());
                assert_eq!(names(&state, clip), start, "undo after dropping {from}");
            }
        }
    }
}

#[test]
fn remove_takes_out_that_effect_alone() {
    let (mut state, clip) = two_effects();
    let vignette = chain(&state, clip)[1].clone();
    press(&mut state, clip, 0, "ctx-fx-remove");
    assert_eq!(chain(&state, clip), [vignette]);
}

#[test]
fn a_verb_on_an_effect_that_is_gone_writes_nothing() {
    // A menu stays open while the chain changes under it; its rows then name
    // an index the chain no longer holds.
    let (mut state, clip) = two_effects();
    let before = chain(&state, clip);
    let depth = state.history_depth();
    state.reset_effect(clip, 9).expect("reset");
    state.rename_effect(clip, 9, "Gone").expect("rename");
    state.duplicate_effect(clip, 9).expect("duplicate");
    state.copy_effect(clip, 9);
    state.save_effect_as_preset(clip, 9);
    state.remove_effect(clip, 9).expect("remove");
    state.begin_rename(RenameTarget::Effect {
        clip: clip.raw(),
        index: 9,
    });
    assert_eq!(chain(&state, clip), before);
    assert_eq!(state.history_depth(), depth);
    assert!(state.copied_effect.is_none());
    assert!(state.preset_draft().is_none());
    assert_eq!(state.rename, None);
}
