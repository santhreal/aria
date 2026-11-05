//! Effect menu category submenus and clear guards.
//!
//! # Why
//!
//! A catalog entry with no row cannot be applied from the bar, a row under the
//! wrong category is not where the effect is looked for, and a row that applies
//! a different effect than it names edits the wrong thing. The expected rows are
//! derived from the catalogs at run time, so an effect added to either catalog
//! is checked without editing this file.
//!
//! What this does not catch: how the flyouts draw, which `menu_keys` drives.

use std::collections::BTreeMap;

use aria_models::{AudioInsertTarget, OTHER_AUDIO_CATEGORY, OTHER_VIDEO_CATEGORY};
use gpui::AppContext;

use super::{APPLY_AUDIO_PREFIX, APPLY_PREFIX};
use crate::app::AppState;
use crate::app::premiere_tests::{with_clip, with_media};
use crate::chrome::menu_items::{DropEntry, MenuItem, drop_entries, items_of};
use crate::types::AppMenu;

/// The Effect menu's catalog rows as id -> (category, label).
fn offered(items: &[MenuItem]) -> BTreeMap<&'static str, (&'static str, &'static str)> {
    items
        .iter()
        .filter(|item| item.id.starts_with(APPLY_PREFIX) || item.id.starts_with(APPLY_AUDIO_PREFIX))
        .map(|item| {
            (
                item.id,
                (
                    item.under.expect("a catalog row sits in a submenu"),
                    item.label,
                ),
            )
        })
        .collect()
}

#[test]
fn every_effect_is_offered_once_under_its_category_and_no_generator_is() {
    let (state, _) = with_clip();
    let items = items_of(AppMenu::Effect, &state);
    let rows = offered(&items);
    let mut expected = BTreeMap::new();
    for spec in aria_models::effect_catalog() {
        if spec.generator {
            continue;
        }
        expected.insert(
            format!("{APPLY_PREFIX}{}", spec.id),
            (spec.category().to_owned(), spec.label.clone()),
        );
    }
    for spec in aria_models::audio_effect_catalog() {
        expected.insert(
            format!("{APPLY_AUDIO_PREFIX}{}", spec.id),
            (spec.category().to_owned(), spec.label.clone()),
        );
    }
    let got: BTreeMap<String, (String, String)> = rows
        .iter()
        .map(|(id, (category, label))| {
            (
                (*id).to_owned(),
                ((*category).to_owned(), (*label).to_owned()),
            )
        })
        .collect();
    assert_eq!(got, expected);
    assert_eq!(
        items
            .iter()
            .filter(|item| item.id.starts_with("fx-apply"))
            .count(),
        expected.len(),
        "no catalog row repeats"
    );
}

#[test]
fn categories_are_sorted_with_the_other_groups_last() {
    let (state, _) = with_clip();
    let items = items_of(AppMenu::Effect, &state);
    let captions: Vec<&str> = drop_entries(&items)
        .iter()
        .filter_map(|entry| match entry {
            DropEntry::Sub { caption, rows }
                if rows.iter().all(|&at| items[at].id.starts_with("fx-apply")) =>
            {
                Some(*caption)
            }
            _ => None,
        })
        .collect();
    let mut video: Vec<&str> = aria_models::effect_catalog()
        .iter()
        .filter(|spec| !spec.generator)
        .map(|spec| crate::interned::intern(spec.category()))
        .collect();
    video.sort_unstable_by_key(|category| (*category == OTHER_VIDEO_CATEGORY, *category));
    video.dedup();
    let mut audio: Vec<&str> = aria_models::audio_effect_catalog()
        .iter()
        .map(|spec| crate::interned::intern(spec.category()))
        .collect();
    audio.sort_unstable_by_key(|category| (*category == OTHER_AUDIO_CATEGORY, *category));
    audio.dedup();
    let expected: Vec<&str> = video.into_iter().chain(audio).collect();
    assert_eq!(captions, expected);
    assert!(
        captions.len() > 2,
        "the shipped catalogs name several categories: {captions:?}"
    );
}

/// Run the Effect row `id` against `state`, then hand the state to `check`.
fn run(state: AppState, id: &str, check: impl FnOnce(&mut AppState)) {
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            let row = items_of(AppMenu::Effect, state)
                .into_iter()
                .find(|item| item.id == id)
                .unwrap_or_else(|| panic!("{id} is not offered"));
            assert_eq!(row.blocked_by(state), None, "{id}");
            (row.run)(row.id, state, cx);
            check(state);
        })
    });
}

/// The ids of the effects on `clip`, in chain order.
fn video_chain(state: &AppState, clip: aria_models::ClipId) -> Vec<String> {
    state
        .engine()
        .lock()
        .project()
        .clip(clip)
        .expect("the clip")
        .effects
        .iter()
        .map(|effect| effect.effect_id.clone())
        .collect()
}

#[test]
fn a_video_row_applies_the_effect_it_names() {
    for spec in aria_models::effect_catalog()
        .iter()
        .filter(|spec| !spec.generator)
    {
        let id = format!("{APPLY_PREFIX}{}", spec.id);
        let (state, clip) = with_clip();
        run(state, &id, |state| {
            assert_eq!(
                video_chain(state, clip),
                std::slice::from_ref(&spec.id),
                "{id}"
            );
            state.undo();
            assert!(video_chain(state, clip).is_empty(), "one Undo removes {id}");
        });
    }
}

#[test]
fn an_audio_row_applies_the_effect_it_names_to_every_clip_with_sound() {
    for spec in aria_models::audio_effect_catalog().iter() {
        let id = format!("{APPLY_AUDIO_PREFIX}{}", spec.id);
        let (state, clip) = with_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/fixture-sine-48k-2s.wav"),
        );
        run(state, &id, |state| {
            let chain = |state: &AppState| -> Vec<String> {
                state
                    .engine()
                    .lock()
                    .project()
                    .audio_inserts(AudioInsertTarget::Clip(clip))
                    .expect("the chain")
                    .iter()
                    .map(|insert| insert.effect_id.clone())
                    .collect()
            };
            assert_eq!(chain(state), std::slice::from_ref(&spec.id), "{id}");
            state.undo();
            assert!(chain(state).is_empty(), "one Undo removes {id}");
        });
    }
}

#[test]
fn reset_grade_and_remove_chroma_key_require_a_grade_or_key_and_write_no_noop_edit() {
    use aria_models::{ClipParam, ColorAdjustments, LookParam};

    let (mut state, clip) = with_clip();
    let row = |state: &AppState, id: &str| -> MenuItem {
        items_of(AppMenu::Effect, state)
            .into_iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("{id} is not offered"))
    };

    state.clear_selection();
    assert_eq!(
        row(&state, "seq-grade-reset").blocked_by(&state),
        Some("No clip is selected")
    );
    assert_eq!(
        row(&state, "seq-chroma-clear").blocked_by(&state),
        Some("No clip is selected")
    );

    state.select_clip(clip);
    assert_eq!(
        row(&state, "seq-grade-reset").blocked_by(&state),
        Some("The selected clip has no grade")
    );
    assert_eq!(
        row(&state, "seq-chroma-clear").blocked_by(&state),
        Some("The selected clip has no chroma key")
    );

    let depth = state.history_depth();
    state.status_message = None;
    state
        .clear_selected_adjustments()
        .expect("clearing an ungraded clip succeeds");
    state
        .clear_selected_chroma()
        .expect("clearing an unkeyed clip succeeds");
    assert_eq!(state.history_depth(), depth);
    assert_eq!(state.status_message, None);

    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            for id in ["seq-grade-reset", "seq-chroma-clear"] {
                let item = row(state, id);
                (item.run)(item.id, state, cx);
            }
            assert_eq!(state.history_depth(), depth);
            assert_eq!(state.status_message, None);

            state
                .set_param_scalar(
                    ClipParam::Look {
                        param: LookParam::AdjustExposure,
                    },
                    None,
                    0.75,
                )
                .expect("grade exposure lands");
            state
                .set_selected_chroma_color([0, 177, 64])
                .expect("chroma key lands");
            assert_eq!(row(state, "seq-grade-reset").blocked_by(state), None);
            assert_eq!(row(state, "seq-chroma-clear").blocked_by(state), None);

            let grade_item = row(state, "seq-grade-reset");
            (grade_item.run)(grade_item.id, state, cx);
            assert_eq!(
                state.selected_model().expect("selected clip").adjust,
                ColorAdjustments::default()
            );
            assert_eq!(state.status_message.as_deref(), Some("Cleared grade"));
            assert_eq!(
                row(state, "seq-grade-reset").blocked_by(state),
                Some("The selected clip has no grade")
            );
            assert!(state.undo());
            assert_eq!(
                state
                    .selected_model()
                    .expect("selected clip")
                    .adjust
                    .exposure
                    .constant(),
                Some(0.75)
            );

            let chroma_item = row(state, "seq-chroma-clear");
            (chroma_item.run)(chroma_item.id, state, cx);
            assert!(
                state
                    .selected_model()
                    .expect("selected clip")
                    .chroma_key
                    .is_none()
            );
            assert_eq!(state.status_message.as_deref(), Some("Cleared chroma key"));
            assert_eq!(
                row(state, "seq-chroma-clear").blocked_by(state),
                Some("The selected clip has no chroma key")
            );
            assert!(state.undo());
            assert_eq!(
                state
                    .selected_model()
                    .expect("selected clip")
                    .chroma_key
                    .as_ref()
                    .map(|key| key.rgb),
                Some([0, 177, 64])
            );
        });
    });
}
