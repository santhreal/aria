//! Contracts for landing a dragged sidebar catalog entry.
//!
//! The class closed here is "a drop applies to whatever happens to be selected".
//! Every setter in this crate addresses the selection, so a drop that forgot to
//! retarget would silently edit a different clip. Each kind is asserted to land on
//! the clip named by the drop, and each kind is asserted inert on the target class
//! it does not belong to, so a payload can never half-apply.
//!
//! Kinds are enumerated from a table built out of the model catalogs at run time, so
//! a new `CatalogDragKind` fails here until it is given a landing.

use aria_models::{ClipId, MaskKind, TrackId, mask_catalog};

use super::AppState;
use super::premiere_tests::with_clip;
use crate::types::{CatalogDrag, CatalogDragKind};

/// One drag payload per kind, each id taken from the model catalog it browses.
fn payloads() -> Vec<CatalogDrag> {
    vec![
        CatalogDrag {
            kind: CatalogDragKind::Sticker,
            id: aria_models::sticker_catalog()[0].id.clone().into(),
            label: "sticker".into(),
        },
        CatalogDrag {
            kind: CatalogDragKind::Animation,
            id: aria_models::animation_catalog()[0].id.clone().into(),
            label: "animation".into(),
        },
        CatalogDrag {
            kind: CatalogDragKind::Mask,
            id: mask_catalog()[0].kind.id().into(),
            label: "mask".into(),
        },
        CatalogDrag {
            kind: CatalogDragKind::Transition,
            id: aria_models::shipped_transition_catalog()[0]
                .id
                .clone()
                .into(),
            label: "transition".into(),
        },
        // Only plugins register generators, and a lib test may not register, so
        // this id resolves to nothing; the landing is in `tests/registered_catalog.rs`.
        CatalogDrag {
            kind: CatalogDragKind::Generator,
            id: "test.unregistered_generator".into(),
            label: "generator".into(),
        },
    ]
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

#[test]
fn a_sticker_lands_on_the_lane_and_tick_it_was_dropped_at() {
    let (mut state, _) = with_clip();
    let before = clip_count(&state);
    let drag = &payloads()[0];

    // A tick well past the existing clip, so placement cannot be confused with the
    // playhead's own position.
    let at = 4_000;
    state
        .drop_catalog_on_track(drag, None, at)
        .expect("place the sticker");

    assert_eq!(clip_count(&state), before + 1, "no sticker was placed");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the sticker is projected");
    assert_eq!(
        placed.timeline_start_ticks, at,
        "the sticker ignored the drop tick"
    );
}

#[test]
fn a_negative_drop_tick_lands_at_zero_rather_than_being_refused() {
    let (mut state, _) = with_clip();
    state
        .drop_catalog_on_track(&payloads()[0], None, -500)
        .expect("clamped, not refused");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the sticker is projected");
    assert_eq!(placed.timeline_start_ticks, 0);
}

#[test]
fn a_sticker_dropped_on_a_lane_that_cannot_hold_it_still_lands_at_that_tick() {
    // The operator aimed at a time. A video lane cannot hold a generated sticker, so
    // the lane is corrected and the tick is kept.
    let (mut state, _) = with_clip();
    let video_lane = state
        .store
        .sequence
        .tracks
        .first()
        .map(|track| TrackId::from_raw(track.id))
        .expect("the fixture has a lane");
    let at = 3_000;
    state
        .drop_catalog_on_track(&payloads()[0], Some(video_lane), at)
        .expect("placed");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the sticker is projected");
    assert_eq!(placed.timeline_start_ticks, at);
}

#[test]
fn a_generator_drop_with_no_generator_behind_the_id_is_refused_and_places_nothing() {
    // An id nothing registers, and a shipped effect that changes its input
    // rather than drawing a picture of its own.
    let ids = [
        "test.unregistered_generator".to_owned(),
        aria_models::shipped_effect_catalog()[0].id.clone(),
    ];
    for id in ids {
        let (mut state, _) = with_clip();
        let before = clip_count(&state);
        let drag = CatalogDrag {
            kind: CatalogDragKind::Generator,
            id: id.clone().into(),
            label: "generator".into(),
        };
        let err = state
            .drop_catalog_on_track(&drag, None, 1_000)
            .expect_err("no generator renders this id");
        assert!(
            err.to_string().contains(&format!("no generator `{id}`")),
            "{id}: {err}"
        );
        assert_eq!(clip_count(&state), before, "{id} placed a clip");
    }
}

#[test]
fn every_clip_payload_lands_on_the_dropped_clip_and_selects_it() {
    for drag in payloads().into_iter().filter(CatalogDrag::targets_a_clip) {
        let (mut state, _) = with_clip();
        let target = state.selected_clip().expect("the fixture selects a clip");
        // Deselect, so a drop that forgot to retarget would apply to nothing.
        state.selection.clear();
        state.rebuild_store();

        state
            .drop_catalog_on_clip(&drag, target)
            .unwrap_or_else(|err| panic!("{:?} {}: {err:?}", drag.kind, drag.id));

        assert_eq!(
            state.selected_clip(),
            Some(target),
            "{:?} did not select the clip it landed on",
            drag.kind
        );
        let ui = state.selected_ui_clip().expect("the clip is projected");
        match drag.kind {
            CatalogDragKind::Animation => assert!(
                ui.animation_in.is_some()
                    || ui.animation_out.is_some()
                    || ui.animation_combo.is_some(),
                "{} left no animation on the clip",
                drag.id
            ),
            CatalogDragKind::Mask => assert_eq!(
                ui.mask.map(|mask| mask.kind.id()),
                Some(drag.id.as_str()),
                "{} left no mask on the clip",
                drag.id
            ),
            CatalogDragKind::Transition => {
                // A junction the model refuses carries no transition; the projected
                // capability is what the sidebar row is allowed to promise.
                if ui.can_transition {
                    assert!(
                        ui.transition.is_some(),
                        "{} left no transition at an accepted junction",
                        drag.id
                    );
                }
            }
            CatalogDragKind::Sticker | CatalogDragKind::Generator => unreachable!("filtered out"),
        }
    }
}

#[test]
fn each_payload_is_inert_on_the_target_class_it_does_not_belong_to() {
    for drag in payloads() {
        let (mut state, _) = with_clip();
        let target = state.selected_clip().expect("clip");
        let before = clip_count(&state);

        if drag.targets_a_clip() {
            // A preset has nothing to change on empty lane space, and must not invent
            // a clip to hold it.
            state
                .drop_catalog_on_track(&drag, None, 2_000)
                .expect("inert");
            assert_eq!(
                clip_count(&state),
                before,
                "{:?} placed a clip on empty lane space",
                drag.kind
            );
        } else {
            state.drop_catalog_on_clip(&drag, target).expect("inert");
            assert_eq!(
                clip_count(&state),
                before,
                "{:?} placed a clip when dropped onto one",
                drag.kind
            );
        }
    }
}

#[test]
fn an_unknown_mask_id_is_inert_rather_than_applying_a_default_shape() {
    let (mut state, _) = with_clip();
    let target = state.selected_clip().expect("clip");
    let stale = CatalogDrag {
        kind: CatalogDragKind::Mask,
        id: "not_a_shape".into(),
        label: "stale".into(),
    };
    state.drop_catalog_on_clip(&stale, target).expect("inert");
    assert!(state.selected_ui_clip().expect("clip").mask.is_none());
    // The catalog is the only source of legal ids, so the guard is the resolver.
    assert_eq!(MaskKind::from_id("not_a_shape"), None);
}

#[test]
fn a_drop_on_a_clip_that_no_longer_exists_changes_nothing() {
    // A clip can be deleted while a drag is in flight. The drop must not select a
    // ghost, and must not surface an `UnknownClip` error the operator did not cause.
    let (mut state, _) = with_clip();
    let held = state.selected_clip().expect("clip");
    let ghost = ClipId::from_raw(u64::MAX);
    state
        .drop_catalog_on_clip(&payloads()[1], ghost)
        .expect("a stale target is a no-op");
    assert_eq!(state.selected_clip(), Some(held), "the selection moved");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(
        ui.animation_in.is_none() && ui.animation_out.is_none() && ui.animation_combo.is_none(),
        "a stale drop applied to the held selection"
    );
}
