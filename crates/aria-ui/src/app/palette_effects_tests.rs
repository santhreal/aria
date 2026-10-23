//! Contracts for the effect rows the palette offers.
//!
//! The class closed here is "one effect, two rows". The Effect menu lists every
//! effect under its category and the palette offers every menu row by name, so
//! a second row per effect read from the catalog put each effect on offer
//! twice: `Blur & Sharpen: Gaussian Blur` beside a bare `Gaussian Blur`.
//!
//! What is asserted, walked from the catalogs at run time, so an effect added to
//! a data file is covered with no edit here:
//!
//! - Every video effect that changes its input and every audio effect is
//!   offered once, as its Effect menu row under its category, and no row
//!   offers it by its bare id or name.
//! - Every generator is offered once, by its own name, as a row that places it.
//! - An effect a plugin registers after the offer was built is offered on the
//!   next read, from each catalog the offer reads.
//! - An effect row hands its Effect menu row back to the caller, which runs it
//!   through the press the bar uses: it applies the effect to the selection, and
//!   with nothing selected it reports why and adds nothing.
//!
//! Not covered: drawing, which `panels::palette` does from the same offer.

use std::path::PathBuf;

use gpui::AppContext;

use super::{Offered, PaletteOffer, Pending};
use crate::app::AppState;
use crate::app::palette_tests::state;
use crate::app::premiere_tests::place_media;
use crate::chrome::menu_effect::{APPLY_AUDIO_PREFIX, APPLY_PREFIX};
use crate::types::AppMenu;

/// One effect as the palette should offer it.
#[derive(Debug)]
struct Expected {
    /// The Effect menu row's id.
    row: String,
    /// The label the row is offered under: the category, then the effect.
    label: String,
    /// The effect's own id and name, which no row may be offered under.
    id: String,
    name: String,
}

/// Every effect the Effect menu applies, from both catalogs.
fn expected() -> Vec<Expected> {
    let videos = aria_models::effect_catalog();
    let audios = aria_models::audio_effect_catalog();
    let video = videos
        .iter()
        .filter(|spec| !spec.generator)
        .map(|spec| Expected {
            row: format!("{APPLY_PREFIX}{}", spec.id),
            label: format!("{}: {}", spec.category(), spec.label),
            id: spec.id.clone(),
            name: spec.label.clone(),
        });
    let audio = audios.iter().map(|spec| Expected {
        row: format!("{APPLY_AUDIO_PREFIX}{}", spec.id),
        label: format!("{}: {}", spec.category(), spec.label),
        id: spec.id.clone(),
        name: spec.label.clone(),
    });
    video.chain(audio).collect()
}

/// The registries mounted, a project open and one picture clip on the
/// timeline, selected.
fn with_clip() -> (AppState, aria_models::ClipId) {
    let mut state = state();
    let clip = place_media(
        &mut state,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png"),
    );
    (state, clip)
}

/// The first video effect that changes its input.
fn first_effect() -> std::sync::Arc<aria_models::EffectSpec> {
    aria_models::effect_catalog()
        .into_iter()
        .find(|spec| !spec.generator)
        .expect("the model ships an effect that is not a generator")
}

#[test]
fn every_effect_is_offered_once_as_its_effect_menu_row() {
    let state = state();
    // Read before the offer: a catalog only grows, so every effect listed here
    // is in the catalog the offer reads.
    let effects = expected();
    let offer = state.palette_catalog();
    assert!(effects.len() > 2, "the catalogs ship effects: {effects:?}");
    let mut wrong = Vec::new();
    for effect in &effects {
        let rows: Vec<&PaletteOffer> = offer
            .iter()
            .filter(|row| row.id == effect.row.as_str())
            .collect();
        match rows.as_slice() {
            [row] => {
                if row.label != effect.label.as_str() {
                    wrong.push(format!(
                        "{} is offered as {:?}, not {:?}",
                        effect.row, row.label, effect.label
                    ));
                }
                let menu_row = matches!(
                    row.action,
                    Offered::Menu(AppMenu::Effect, at) if at == effect.row
                );
                if !menu_row || row.source != Some(AppMenu::Effect) {
                    wrong.push(format!(
                        "{} is not offered as its Effect menu row",
                        effect.row
                    ));
                }
            }
            rows => wrong.push(format!("{} is offered {} times", effect.row, rows.len())),
        }
        let again: Vec<&str> = offer
            .iter()
            .filter(|row| row.id == effect.id.as_str() || row.label == effect.name.as_str())
            .map(|row| row.id.as_str())
            .collect();
        if !again.is_empty() {
            wrong.push(format!(
                "{} is offered again by its bare id or name, as {again:?}",
                effect.row
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_generator_is_offered_once_by_its_own_name() {
    let state = state();
    let generators: Vec<_> = aria_models::effect_catalog()
        .into_iter()
        .filter(|spec| spec.generator)
        .collect();
    let offer = state.palette_catalog();
    assert!(!generators.is_empty(), "the model ships no generator");
    for spec in generators {
        let rows: Vec<&PaletteOffer> = offer
            .iter()
            .filter(|row| matches!(&row.action, Offered::Generator(at) if *at == spec.id))
            .collect();
        let [row] = rows.as_slice() else {
            panic!("{} is offered {} times", spec.id, rows.len());
        };
        assert_eq!(row.label, spec.label.as_str(), "{}", spec.id);
        assert_eq!(row.source, None, "{} is offered as a menu row", spec.id);
    }
}

#[test]
fn an_effect_registered_after_the_offer_was_built_is_offered() {
    // Registers effects every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let state = state();
        let offered = |id: &str| -> Vec<String> {
            state
                .palette_catalog()
                .iter()
                .filter(|row| row.id == id)
                .map(|row| row.label.to_string())
                .collect()
        };
        // Each catalog is registered into alone, right after the offer was
        // read, so the offer follows every catalog it lists and not only the
        // one changed last.
        let audio_id = format!("{APPLY_AUDIO_PREFIX}test.palette.audio");
        assert_eq!(offered(&audio_id), NONE, "offered before it was registered");
        let (_audio, audio) = aria_models::register_audio_effect(aria_models::AudioEffectSpec {
            id: "test.palette.audio".into(),
            label: "Palette Probe Audio".into(),
            category: String::new(),
            params: Vec::new(),
            latency: 0,
            sidechain: false,
        })
        .expect("the audio effect registers");
        assert_eq!(
            offered(&audio_id),
            [format!("{}: {}", audio.category(), audio.label)]
        );

        let video_id = format!("{APPLY_PREFIX}test.palette.video");
        assert_eq!(offered(&video_id), NONE, "offered before it was registered");
        let (_video, video) = aria_models::register_effect(aria_models::EffectSpec {
            id: "test.palette.video".into(),
            label: "Palette Probe Video".into(),
            ..Default::default()
        })
        .expect("the effect registers");
        assert_eq!(
            offered(&video_id),
            [format!("{}: {}", video.category(), video.label)]
        );

        assert_eq!(
            offered("test.palette.generator"),
            NONE,
            "offered before it was registered"
        );
        let (_generator, generator) = aria_models::register_effect(aria_models::EffectSpec {
            id: "test.palette.generator".into(),
            label: "Palette Probe Generator".into(),
            generator: true,
            ..Default::default()
        })
        .expect("the generator registers");
        assert_eq!(offered(&generator.id), [generator.label.clone()]);
    });
}

/// No palette row.
const NONE: [String; 0] = [];

/// Run the palette row that applies `spec`, then run what it hands back as the
/// palette's caller does, and hand the state to `check`.
fn run_effect_row(
    mut state: AppState,
    spec: &aria_models::EffectSpec,
    check: impl FnOnce(&mut AppState),
) {
    let row = format!("{APPLY_PREFIX}{}", spec.id);
    let index = state
        .palette_catalog()
        .iter()
        .position(|offer| offer.id == row.as_str())
        .unwrap_or_else(|| panic!("{row} is not offered"));
    let pending = state.run_palette_offer(index);
    let Some(Pending::Menu(AppMenu::Effect, id)) = pending else {
        panic!("{row} ran here instead of handing back its Effect menu row");
    };
    assert_eq!(id, row);
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            assert!(
                crate::chrome::menu_items::run_item(AppMenu::Effect, id, state, cx),
                "{id} left the Effect menu"
            );
            check(state);
        })
    });
}

/// The effects applied to every clip on the timeline, in chain order.
fn applied(state: &AppState) -> Vec<String> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .flat_map(|clip| clip.effects.iter())
        .map(|effect| effect.id.to_string())
        .collect()
}

#[test]
fn an_effect_row_applies_the_effect_to_the_selection() {
    let (state, _clip) = with_clip();
    let spec = first_effect();
    run_effect_row(state, &spec, |state| {
        state.sync_from_engine();
        assert_eq!(applied(state), vec![spec.id.clone()]);
    });
}

#[test]
fn an_effect_row_with_no_selection_says_so_and_adds_nothing() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.status_message = None;
    let spec = first_effect();
    let reason = crate::chrome::menu_guards::selection("", &state);
    assert!(reason.is_some(), "the Effect rows need a selection");
    run_effect_row(state, &spec, |state| {
        assert_eq!(state.status_message.as_deref(), reason);
        assert_eq!(state.last_error, None);
        state.sync_from_engine();
        assert_eq!(
            applied(state),
            Vec::<String>::new(),
            "an effect landed with nothing selected"
        );
    });
}
