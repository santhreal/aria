//! Contracts for the effect rows the palette offers.
//!
//! The class closed here is "an effect can only be added with a pointer". The
//! rack's Add gesture and the sidebar catalog are both clicks, and the palette
//! derived its offer from two `const` registries, so nothing on the keyboard
//! reached the effect catalog at all.
//!
//! What is asserted:
//!
//! - Every effect in the catalog is offered by name, walked from the catalog at
//!   run time, so an effect added to the data file is covered here with no edit
//!   and a collision that hid one is red. A generator is offered as a generator
//!   row, which places it, and any other effect as an effect row, which adds it
//!   to the selection.
//! - Running the row adds that effect to the selection, in catalog order, and
//!   twice in a row stacks rather than replaces, which is what the chain is.
//! - A row that finds no selection says so and adds nothing.

use super::super::premiere_tests::with_clip;
use super::{Offered, PaletteOffer};
use crate::app::decline::Decline;

/// The offer for one effect id, as an effect row or a generator row, or
/// nothing.
fn row(state: &crate::app::AppState, id: &str) -> Option<PaletteOffer> {
    state
        .palette_catalog()
        .iter()
        .find(|offer| {
            matches!(&offer.action, Offered::Effect(at) | Offered::Generator(at) if at == id)
        })
        .cloned()
}

/// The first catalog effect that is not a generator.
fn first_effect() -> std::sync::Arc<aria_models::EffectSpec> {
    aria_models::effect_catalog()
        .into_iter()
        .find(|spec| !spec.generator)
        .expect("the model ships an effect that is not a generator")
}

#[test]
fn every_catalog_effect_is_offered_by_its_own_name() {
    let (state, _clip) = with_clip();
    let catalog = aria_models::effect_catalog();
    assert!(!catalog.is_empty(), "the model ships no effects");
    for spec in catalog {
        let offer = row(&state, &spec.id)
            .unwrap_or_else(|| panic!("{} is in the catalog and not in the palette", spec.id));
        assert_eq!(offer.label, spec.label.as_str());
        assert_eq!(
            matches!(offer.action, Offered::Generator(_)),
            spec.generator,
            "{} is offered as the wrong kind of row",
            spec.id
        );
        assert!(
            offer.source.is_none(),
            "an effect row comes from the catalog, not from a menu"
        );
    }
}

#[test]
fn running_an_effect_row_adds_it_to_the_selection() {
    let (mut state, clip) = with_clip();
    state.select_clip(clip);
    let spec = first_effect();
    let index = state
        .palette_catalog()
        .iter()
        .position(|offer| matches!(&offer.action, Offered::Effect(at) if *at == spec.id))
        .expect("the effect is offered");

    assert!(
        state.run_palette_offer(index).is_none(),
        "adding an effect needs no window"
    );

    let chain: Vec<String> = state
        .selected_ui_clip()
        .expect("a clip is selected")
        .effects
        .iter()
        .map(|effect| effect.id.to_string())
        .collect();
    assert_eq!(chain, vec![spec.id.clone()]);

    // A chain, so the same row twice is two entries.
    state.run_palette_offer(index);
    assert_eq!(
        state
            .selected_ui_clip()
            .expect("a clip is selected")
            .effects
            .len(),
        2
    );
}

#[test]
fn an_effect_row_with_no_selection_says_so_and_adds_nothing() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.status_message = None;
    let spec = first_effect();
    let index = state
        .palette_catalog()
        .iter()
        .position(|offer| matches!(&offer.action, Offered::Effect(at) if *at == spec.id))
        .expect("the effect is offered");

    state.run_palette_offer(index);

    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
    assert_eq!(state.last_error, None);
    let effects: usize = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.effects.len())
        .sum();
    assert_eq!(effects, 0, "an effect landed with nothing selected");
}
