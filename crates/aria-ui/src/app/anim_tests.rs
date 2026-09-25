//! Contracts for clip animations and audio roles.
//!
//! The class closed here is "a chip offers a preset for the wrong slip, the wrong
//! content, or a knob the preset does not expose". Presets and knobs are
//! enumerated from `aria_models::animation_catalog()` at run time, so a new
//! preset fails until it is covered.

use aria_models::{AnimationSlot, AudioRole, animation_catalog, animation_spec};

use super::premiere_tests::with_clip;
use crate::app::clip_anim::{ANIM_KNOBS, SLOTS, anim_knob, anim_knob_available};

#[test]
fn every_non_text_preset_applies_to_its_own_slot() {
    let (mut state, _) = with_clip();
    for slot in SLOTS {
        assert!(state.selected_animation(slot).is_none());
    }

    let presets: Vec<_> = animation_catalog()
        .iter()
        .filter(|spec| !spec.text_only)
        .collect();
    assert!(!presets.is_empty());

    for spec in presets {
        state
            .set_selected_animation(&spec.id)
            .unwrap_or_else(|err| panic!("apply {}: {err:?}", spec.id));
        let applied = state
            .selected_animation(spec.slot)
            .unwrap_or_else(|| panic!("{} did not land in {:?}", spec.id, spec.slot));
        assert_eq!(applied.id, spec.id);
        assert_eq!(
            applied.label,
            spec.label.as_str(),
            "the label comes from the catalog"
        );
    }

    for slot in SLOTS {
        state.clear_selected_animation(slot).expect("clear");
        assert!(
            state.selected_animation(slot).is_none(),
            "{slot:?} survived"
        );
    }
}

#[test]
fn a_text_only_preset_is_not_offered_on_a_media_clip() {
    let (state, _) = with_clip();
    for slot in SLOTS {
        let offered = state.animation_choices(slot);
        for choice in &offered {
            let spec = animation_spec(choice.id).expect("offered preset is in the catalog");
            assert_eq!(
                spec.slot, slot,
                "{} was offered for the wrong slot",
                choice.id
            );
            assert!(
                !spec.text_only,
                "{} is text-only and must not be offered on a media clip",
                choice.id
            );
            // Every shipped preset has a sampler, so the browser must not be
            // labelling one as unrenderable.
            assert!(
                choice.rendered,
                "{} is offered without a renderer",
                choice.id
            );
        }
        // Every non-text preset for this slot must be offered: a missing one is
        // a feature the operator cannot reach.
        let expected = animation_catalog()
            .iter()
            .filter(|spec| spec.slot == slot && !spec.text_only)
            .count();
        assert_eq!(offered.len(), expected, "{slot:?} hid a usable preset");
    }
}

#[test]
fn an_unknown_preset_id_is_inert() {
    let (mut state, _) = with_clip();
    state
        .set_selected_animation("no-such-preset")
        .expect("inert");
    for slot in SLOTS {
        assert!(state.selected_animation(slot).is_none());
    }
}

#[test]
fn setting_an_edge_clears_a_combo_and_the_reverse() {
    let (mut state, _) = with_clip();
    state.set_selected_animation("pulse").expect("combo");
    assert!(state.selected_animation(AnimationSlot::Combo).is_some());

    state.set_selected_animation("fade_in").expect("entrance");
    assert!(
        state.selected_animation(AnimationSlot::Combo).is_none(),
        "an edge animation must retire the combo"
    );
    assert!(state.selected_animation(AnimationSlot::In).is_some());

    state.set_selected_animation("rock").expect("combo again");
    assert!(
        state.selected_animation(AnimationSlot::In).is_none(),
        "a combo must retire the edge animations"
    );
    assert!(state.selected_animation(AnimationSlot::Combo).is_some());
}

#[test]
fn animation_knobs_move_alone_clamp_and_respect_the_preset() {
    let (mut state, _) = with_clip();
    state.set_selected_animation("fade_in").expect("entrance");
    let applied = state
        .selected_animation(AnimationSlot::In)
        .cloned()
        .expect("applied");
    let spec = animation_spec(&applied.id).expect("spec");

    for (id, _, _, _, (min, max), available) in ANIM_KNOBS {
        let exposed = available(&spec.knobs);
        assert_eq!(
            anim_knob_available(&applied, id),
            exposed,
            "{id} availability must come from the preset"
        );

        let target = 1.5_f32.clamp(*min, *max);
        state
            .set_selected_animation_knob(AnimationSlot::In, id, target)
            .unwrap_or_else(|err| panic!("set {id}: {err:?}"));
        let now = state
            .selected_animation(AnimationSlot::In)
            .expect("applied");
        if exposed {
            assert!(
                (anim_knob(&now, id) - target).abs() < 1e-3,
                "{id} did not take its value"
            );
        } else {
            // A knob the preset does not expose must be dropped, not sent and
            // rejected by the model.
            assert!(
                (anim_knob(&now, id) - 1.0).abs() < 1e-3,
                "{id} was written even though the preset has no such knob"
            );
        }

        state
            .set_selected_animation_knob(AnimationSlot::In, id, 1e6)
            .expect("high");
        let now = state
            .selected_animation(AnimationSlot::In)
            .expect("applied");
        assert!(anim_knob(&now, id) <= *max + 1e-3, "{id} exceeded {max}");

        state
            .set_selected_animation_knob(AnimationSlot::In, id, f32::NAN)
            .expect("nan");
        let now = state
            .selected_animation(AnimationSlot::In)
            .expect("applied");
        assert!(anim_knob(&now, id).is_finite(), "{id} took a NaN");
    }

    // A knob edit must not move the preset out of its slot or drop it.
    let now = state
        .selected_animation(AnimationSlot::In)
        .expect("applied");
    assert_eq!(now.id, "fade_in");
}

#[test]
fn animation_edits_need_a_slot_and_commands_are_inert_with_no_selection() {
    let (mut state, _) = with_clip();
    for (id, ..) in ANIM_KNOBS {
        state
            .set_selected_animation_knob(AnimationSlot::In, id, 2.0)
            .expect("inert knob");
        state
            .nudge_selected_animation_knob(AnimationSlot::Out, id, 0.5)
            .expect("inert nudge");
    }
    for slot in SLOTS {
        assert!(state.selected_animation(slot).is_none());
    }

    state.clear_selection();
    state.set_selected_animation("fade_in").expect("inert");
    state
        .clear_selected_animation(AnimationSlot::In)
        .expect("inert");
    state
        .set_selected_audio_role(Some(AudioRole::Music))
        .expect("inert");

    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert!(clip.animation_in.is_none());
    assert!(clip.audio_role.is_none());
}

/// The role tag is audio-lane only, so on a video clip the command layer refuses
/// it. Either way the surface must not panic and must not show a tag that was
/// never stored.
#[test]
fn every_audio_role_round_trips_or_is_refused_consistently() {
    let (mut state, _) = with_clip();
    assert!(state.selected_ui_clip().expect("clip").audio_role.is_none());

    for role in AudioRole::ALL {
        match state.set_selected_audio_role(Some(role)) {
            Ok(()) => assert_eq!(
                state.selected_ui_clip().expect("clip").audio_role,
                Some(role),
                "{role:?} did not reach the projection"
            ),
            Err(_) => assert!(
                state.selected_ui_clip().expect("clip").audio_role.is_none(),
                "a refused role must leave the clip untagged"
            ),
        }
    }

    let _ = state.set_selected_audio_role(None);
    assert!(state.selected_ui_clip().expect("clip").audio_role.is_none());
}

/// Hovering a catalog entry is a preview, not an edit. The class closed here is "a
/// preview leaves state behind": a history entry, a revision bump, a slot on the clip,
/// or an override that survives the pointer leaving.
#[test]
fn hovering_a_preset_previews_it_without_editing_the_clip() {
    let (mut state, clip) = with_clip();
    let revision = state.engine().lock().revision();

    let preset = animation_catalog()
        .iter()
        .find(|spec| !spec.text_only)
        .expect("the catalog offers a preset for a media clip");

    assert!(state.preview_animation(&preset.id), "hover set no override");
    assert!(
        state.engine().lock().has_live_overrides(),
        "the engine renders the same frame as before the hover"
    );
    assert_eq!(
        state.animation_preview,
        Some((
            clip,
            preset.slot,
            Some(aria_models::AnimationRef::new(&preset.id))
        ))
    );
    assert!(
        state.selected_animation(preset.slot).is_none(),
        "a preview reached the clip"
    );
    assert_eq!(state.engine().lock().revision(), revision);

    // Re-entering the row the pointer is already on costs no frame.
    assert!(!state.preview_animation(&preset.id));

    assert!(state.clear_animation_preview(), "leaving cleared nothing");
    assert!(!state.engine().lock().has_live_overrides());
    assert!(
        !state.clear_animation_preview(),
        "a second leave asked for a frame"
    );
    assert!(state.selected_animation(preset.slot).is_none());
    assert_eq!(state.engine().lock().revision(), revision);

    // The fixture's own placement is the top of the stack: undoing once lands before
    // it, which it could not if the hover had pushed anything.
    assert!(state.undo());
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .all(|track| track.clips.is_empty()),
        "the hover pushed an undo entry of its own"
    );
}

/// Two ways a hover has nothing to preview, and one way a preview must yield: it is
/// dropped by the commit, so a later slot change is not hidden behind a stale override.
#[test]
fn a_preview_is_inert_without_a_target_and_yields_to_a_commit() {
    let (mut state, _) = with_clip();
    assert!(!state.preview_animation("no-such-preset"));
    assert!(!state.engine().lock().has_live_overrides());

    let preset = animation_catalog()
        .iter()
        .find(|spec| !spec.text_only)
        .expect("a preset");
    assert!(state.preview_animation(&preset.id));
    state.set_selected_animation(&preset.id).expect("apply");
    assert!(
        !state.engine().lock().has_live_overrides(),
        "the commit left its own preview on top of it"
    );
    assert!(state.animation_preview.is_none());
    assert_eq!(
        state.selected_animation(preset.slot).expect("applied").id,
        preset.id
    );

    state.selection.clear();
    assert!(
        !state.preview_animation(&preset.id),
        "previewed with no selection"
    );
    assert!(!state.engine().lock().has_live_overrides());
}
