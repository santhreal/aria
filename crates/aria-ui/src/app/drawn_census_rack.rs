//! Driving the effects rack: adding, removing, reordering, presetting and
//! editing every slot of an effect, through the registry the rack is built from.
//!
//! # Why
//!
//! The rack's rows already came from the model, so a plugin's effect drew its own
//! fields. The writes were six closures, which left five commands unmeasurable
//! and every field's write unchecked. The rack is now
//! [`effects_rack`](crate::surfaces::effects_rack), and this census presses each
//! gesture on a chain dressed to have something to act on.
//!
//! The scalar and vector slots are driven for every slot the effect exposes and
//! every colour in the palette, not one representative: an effect with two
//! colours or a vec2 whose y axis was wired to x is exactly the defect this
//! closes.

use std::collections::BTreeSet;

use aria_models::Preset;

use crate::app::AppState;
use crate::app::clip_effects::{PARAM_COLORS, color_params, scalar_params, vec2_params};
use crate::app::fixtures::{the_pair, twin_selected};
use crate::app::sent_census::Watch;
use crate::surfaces::effects_rack::{self, Gesture};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "effects_rack.rs";

/// The name the census saves a preset under.
const PRESET: &str = "Census preset";

/// One gesture, with a chain dressed so it has something to act on.
struct Staged {
    gesture: Gesture,
    dress: fn() -> AppState,
}

/// Every gesture the rack offers, built from the model's own catalogs.
fn staged() -> Vec<Staged> {
    let mut out = Vec::new();
    let mut push = |gesture: Gesture, dress: fn() -> AppState| out.push(Staged { gesture, dress });

    for (spec, _) in a_clip().effect_browse_list() {
        push(Gesture::Add(spec.id.clone()), a_clip);
    }
    push(Gesture::Remove(0), one_effect);
    push(Gesture::Bypass(0), one_effect);
    push(Gesture::Bypass(1), two_effects);
    push(Gesture::MoveUp(1), two_effects);
    push(Gesture::MoveDown(0), two_effects);
    push(
        Gesture::Drop {
            from: 0,
            before: None,
        },
        two_effects,
    );
    push(Gesture::Reset(0), a_modified_effect);
    push(Gesture::Preset(PRESET.to_string()), a_saved_preset);

    let (colored, slot) = a_color_slot();
    for (_, _, rgba) in PARAM_COLORS {
        push(
            Gesture::Color {
                index: 0,
                slot,
                rgba,
            },
            colored,
        );
    }
    for slot in scalar_slots() {
        push(
            Gesture::NudgeScalar {
                index: 0,
                slot,
                by: 0.05,
            },
            a_scalar_effect,
        );
    }
    for slot in vec2_slots() {
        for axis in [0usize, 1] {
            push(
                Gesture::NudgeVec2 {
                    index: 0,
                    slot,
                    axis,
                    by: 0.05,
                },
                a_vec2_effect,
            );
        }
    }
    push(
        Gesture::EditSlot {
            index: 0,
            slot: 0,
            edit: crate::surfaces::effects_rack::SlotEdit::Reset,
        },
        a_modified_effect,
    );
    out
}

/// The gestures that change the preset file rather than the project.
///
/// Kept apart from [`staged`] because the census of commands is a census of
/// edits: saving a chain under a name and forgetting a name send nothing to the
/// engine, and pressing them in that loop would assert they did.
fn staged_locally() -> Vec<Staged> {
    vec![
        Staged {
            gesture: Gesture::SavePreset,
            dress: a_named_draft,
        },
        Staged {
            gesture: Gesture::TogglePresetEffect(0),
            dress: a_named_draft,
        },
        Staged {
            gesture: Gesture::TogglePresetAudioEffects,
            dress: a_named_draft,
        },
        Staged {
            gesture: Gesture::TogglePresetProperty("opacity".to_string()),
            dress: a_named_draft,
        },
        Staged {
            gesture: Gesture::DeletePreset(PRESET.to_string()),
            dress: a_saved_preset,
        },
        Staged {
            gesture: Gesture::ToggleGroup {
                index: 0,
                group: "advanced".to_string(),
            },
            dress: one_effect,
        },
    ]
}

/// Every command the rack asked for, one press per staged gesture.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in staged() {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        let what = stage.gesture.what();
        effects_rack::apply(&mut state, stage.gesture)
            .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_gesture_the_rack_offers_is_driven() {
    let driven: BTreeSet<String> = staged()
        .iter()
        .chain(staged_locally().iter())
        .map(|stage| stage.gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !driven.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "the rack offers gestures no dressed chain drives: {missing:?}"
    );
}

#[test]
fn every_gesture_driven_asks_the_model_for_something() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        let what = stage.gesture.what();
        effects_rack::apply(&mut state, stage.gesture)
            .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
        assert!(
            !watch.take().is_empty(),
            "{what} sent nothing, so either the button is inert or its chain is \
             dressed with nothing for it to act on"
        );
    }
}

/// With a second clip selected that holds the same chain, every gesture leaves
/// the two clips alike, in at most one undo step, and that step takes the
/// gesture back on both. The gestures are the staged ones, so a gesture added
/// to the rack that reaches the drawn clip alone is red here with no edit to
/// this test.
#[test]
fn every_gesture_reaches_a_second_selected_clip_holding_the_same_chain() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let (drawn, twin) = twin_selected(&mut state);
        let before = the_pair(&state, drawn, twin);
        assert_eq!(
            before.1, before.0,
            "the copy differs from the clip it copies"
        );
        let depth = state.history_depth();
        let what = stage.gesture.what();
        effects_rack::apply(&mut state, stage.gesture)
            .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
        let (after, twin_after) = the_pair(&state, drawn, twin);
        assert_eq!(
            twin_after, after,
            "{what} reached the drawn clip and not the other selected clip"
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

/// Saving is only real if the next launch reads it back, so the assertion is
/// against the file the loader reads and not only against the list in memory.
#[test]
fn saving_a_named_chain_grows_the_presets_and_writes_them() {
    let mut state = a_named_draft();
    assert!(state.presets().is_empty());

    effects_rack::apply(&mut state, Gesture::SavePreset).expect("the named chain saves");

    let names: Vec<&str> = state
        .presets()
        .iter()
        .map(|preset| preset.name.as_str())
        .collect();
    assert_eq!(names, [PRESET]);
    assert!(!state.preset_draft_open(), "a saved name leaves the field");
    let (on_disk, broken) = crate::app::presets::load_presets(&state.data_dir);
    assert!(broken.is_empty());
    assert_eq!(on_disk.len(), 1, "the save must reach the file");
    assert_eq!(on_disk[0].name, PRESET);
}

/// A name already taken is refused with the field still open and still holding
/// what was typed: the operator's next act is to change the name.
#[test]
fn a_name_already_taken_is_refused_without_losing_the_typing() {
    let mut state = a_named_draft();
    effects_rack::apply(&mut state, Gesture::SavePreset).expect("the first save");
    state.open_preset_draft();
    for ch in PRESET.chars() {
        state.preset_draft_push(ch);
    }

    let refused = effects_rack::apply(&mut state, Gesture::SavePreset)
        .expect_err("the same name twice is refused");
    assert!(
        matches!(refused, aria_engine::EngineError::Refused(_)),
        "a taken name is a refusal, not a failure: {refused:?}"
    );
    assert_eq!(state.preset_draft_text(), PRESET);
    assert_eq!(state.presets().len(), 1);
}

/// Deleting shrinks the list and the file together, so a deleted preset cannot
/// come back on the next launch.
#[test]
fn deleting_a_preset_shrinks_the_presets_and_the_file() {
    let mut state = a_named_draft();
    effects_rack::apply(&mut state, Gesture::SavePreset).expect("the save");

    effects_rack::apply(&mut state, Gesture::DeletePreset(PRESET.to_string()))
        .expect("the saved preset deletes");

    assert!(state.presets().is_empty());
    assert!(
        crate::app::presets::load_presets(&state.data_dir)
            .0
            .is_empty(),
        "the delete must reach the file"
    );
}

#[test]
fn the_census_reaches_every_effect_command() {
    let sent = dispatched();
    for label in [
        "Add effect",
        "Remove effect",
        "Reorder effect",
        "Set effect parameter",
        "Update effect parameters",
        "Bypass effect",
        "Apply preset",
    ] {
        assert!(
            sent.contains(label),
            "the rack sent {sent:?}, which does not include `{label}`"
        );
    }
}

#[test]
fn the_rack_calls_no_setter_of_its_own() {
    super::no_setter_escapes(REGISTRY, "inspector");
}

/// A row names an effect by where it sits in the chain, and a chain can be
/// shortened by an undo between the frame that drew the row and the click that
/// pressed it. That press must be inert rather than an error or a panic.
#[test]
fn a_gesture_past_the_end_of_the_chain_is_inert() {
    for gesture in [
        Gesture::Remove(9),
        Gesture::Bypass(9),
        Gesture::MoveUp(9),
        Gesture::MoveDown(9),
        Gesture::Reset(9),
        Gesture::Drop {
            from: 9,
            before: None,
        },
        Gesture::Color {
            index: 9,
            slot: 0,
            rgba: [1, 2, 3, 4],
        },
        Gesture::NudgeScalar {
            index: 9,
            slot: 0,
            by: 0.1,
        },
        Gesture::NudgeVec2 {
            index: 9,
            slot: 0,
            axis: 0,
            by: 0.1,
        },
        Gesture::EditSlot {
            index: 9,
            slot: 0,
            edit: crate::surfaces::effects_rack::SlotEdit::Reset,
        },
        Gesture::ToggleGroup {
            index: 9,
            group: "advanced".to_string(),
        },
    ] {
        let mut state = one_effect();
        let watch = Watch::on(&state);
        let what = gesture.what();
        effects_rack::apply(&mut state, gesture)
            .unwrap_or_else(|err| panic!("{what} past the end of the chain: {err}"));
        let sent = watch.take();
        assert!(
            sent.is_empty(),
            "{what} on an effect that is not there sent {sent:?}"
        );
    }
}

/// A move that would leave the effect where it is must be inert rather than
/// reorder something else: the first effect up and the last one down, which the
/// rack does not draw, and a drag onto the effect itself, onto the effect just
/// below it, or to the end of a chain it already ends.
#[test]
fn a_move_that_goes_nowhere_does_nothing() {
    for (gesture, edge) in [
        (Gesture::MoveUp(0), "the first effect up"),
        (Gesture::MoveDown(1), "the last effect down"),
        (
            Gesture::Drop {
                from: 0,
                before: Some(0),
            },
            "an effect onto itself",
        ),
        (
            Gesture::Drop {
                from: 0,
                before: Some(1),
            },
            "an effect onto the one below it",
        ),
        (
            Gesture::Drop {
                from: 1,
                before: None,
            },
            "the last effect to the end",
        ),
    ] {
        let mut state = two_effects();
        let watch = Watch::on(&state);
        effects_rack::apply(&mut state, gesture).expect("an edge move is not an error");
        let sent = watch.take();
        assert!(sent.is_empty(), "moving {edge} sent {sent:?}");
    }
}

// --- dressed states ------------------------------------------------------

fn a_clip() -> AppState {
    crate::app::premiere_tests::with_clip().0
}

/// The first effect the browse list offers, on the selected clip.
fn one_effect() -> AppState {
    let mut state = a_clip();
    let id = first_effect();
    state
        .add_effect_to_selected(id.as_str())
        .unwrap_or_else(|err| panic!("adding {id}: {err}"));
    state
}

fn two_effects() -> AppState {
    let mut state = one_effect();
    let id = first_effect();
    state
        .add_effect_to_selected(id.as_str())
        .unwrap_or_else(|err| panic!("adding a second {id}: {err}"));
    state
}

/// A chain plus a preset holding it, put in the list the rack draws from rather
/// than saved to disk: a census writes no files.
fn a_saved_preset() -> AppState {
    let mut state = one_effect();
    let effects = {
        let clip = state.selected_clip().expect("the fixture selects a clip");
        let engine = state.engine.lock();
        engine
            .project()
            .clip(clip)
            .expect("the selected clip is in the project")
            .effects
            .to_vec()
    };
    state.presets = vec![Preset::new(
        PRESET.to_string(),
        effects,
        Vec::new(),
        Vec::new(),
    )];
    state
}

/// A chain, plus the name field open over it with a name typed in.
///
/// Dressed through the product path (the button opens the field, the keys fill
/// it) so a field that stopped accepting characters fails here rather than
/// passing on a name the census wrote past it.
fn a_named_draft() -> AppState {
    let mut state = one_effect();
    state.open_preset_draft();
    for ch in PRESET.chars() {
        state.preset_draft_push(ch);
    }
    assert_eq!(
        state.preset_draft_text(),
        PRESET,
        "the field must hold what was typed into it"
    );
    state
}

/// The first effect in the catalog that has a colour slot, and which slot.
///
/// Read out of the model rather than named, so an effect losing its colour or a
/// new one gaining a second colour changes what is pressed without an edit here.
fn a_color_slot() -> (fn() -> AppState, usize) {
    let slot = effect_with(|id| color_params(id).first().map(|(slot, _)| slot))
        .expect("some effect in the catalog has a colour slot")
        .1;
    (a_color_effect, slot)
}

fn scalar_slots() -> Vec<usize> {
    let (id, _) = effect_with(|id| scalar_params(id).first().map(|(slot, _)| slot))
        .expect("some effect in the catalog has a number");
    scalar_params(&id).slots().collect()
}

fn vec2_slots() -> Vec<usize> {
    match effect_with(|id| vec2_params(id).first().map(|(slot, _)| slot)) {
        Some((id, _)) => vec2_params(&id).slots().collect(),
        None => Vec::new(),
    }
}

/// The first effect the browse list offers for which `pick` answers, and that
/// answer.
fn effect_with(pick: impl Fn(&str) -> Option<usize>) -> Option<(String, usize)> {
    a_clip()
        .effect_browse_list()
        .into_iter()
        .find_map(|(spec, _)| pick(&spec.id).map(|found| (spec.id.clone(), found)))
}

fn first_effect() -> String {
    a_clip()
        .effect_browse_list()
        .first()
        .expect("the effect catalog ships an effect")
        .0
        .id
        .clone()
}

fn a_color_effect() -> AppState {
    with_effect(|id| !color_params(id).is_empty())
}

fn a_scalar_effect() -> AppState {
    with_effect(|id| !scalar_params(id).is_empty())
}

fn a_vec2_effect() -> AppState {
    with_effect(|id| !vec2_params(id).is_empty())
}
fn a_modified_effect() -> AppState {
    let mut state = a_scalar_effect();
    let slot = scalar_slots()[0];
    state
        .nudge_selected_effect_param(0, slot, 0.05)
        .expect("nudge scalar slot");
    state
}

/// A clip carrying the first effect that satisfies `wanted`.
fn with_effect(wanted: impl Fn(&str) -> bool) -> AppState {
    let mut state = a_clip();
    let id = state
        .effect_browse_list()
        .into_iter()
        .find(|(spec, _)| wanted(&spec.id))
        .map(|(spec, _)| spec.id.clone())
        .expect("the effect catalog holds an effect with the slot kind wanted");
    state
        .add_effect_to_selected(id.as_str())
        .unwrap_or_else(|err| panic!("adding {id}: {err}"));
    state
}
