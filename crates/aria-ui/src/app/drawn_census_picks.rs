//! Driving the inspector's picture picks: every chip, swatch, toggle and clear
//! button in the Look and Frame tabs, pressed through the registry the panels
//! are built from.
//!
//! # Why
//!
//! The numbers in a block are generated from the property catalog, so a property
//! the model gains is drawn without anyone editing a panel. The picks were the
//! other half and had no such floor: a filter chip, a mask shape, a key colour,
//! a matte source, a style toggle, a blend mode, a stabilization level, the two
//! flips, motion blur, noise reduction and seven clears were each a `div()` with
//! its setter sealed inside a gpui listener. Twelve of the model's commands were
//! reachable only that way, nothing could enumerate them, and a chip wired to
//! the wrong setter would not have turned anything red.
//!
//! [`Pick`](crate::surfaces::inspector_pick::Pick) is now the registry and
//! `apply` is the only writer. This census reads the arms out of that file's
//! source at run time, so an arm added to the enum has no dressed state until
//! someone writes one and `every_pick_the_registry_offers_is_pressed` goes red
//! until they do. Each member of every catalog is pressed, not one
//! representative: the defect this closes is a mechanism wired for the case
//! someone had in mind and not for its siblings.
//!
//! What this file cannot see: whether the chip is reachable through a tab, which
//! `tab_host`'s own tests hold, and what the pick looks like on screen.

use std::collections::BTreeSet;

use aria_models::{BlendMode, MaskKind, MatteSource, StabilizeLevel, mask_catalog};

use crate::app::AppState;
use crate::app::sent_census::Watch;
use crate::surfaces::inspector_pick::{self, KEY_COLORS, Pick, STYLE_COLORS};

/// One pick, with a project dressed so that pick has something to act on.
///
/// Clearing a mask on an unmasked clip is inert in the product's own terms, so
/// the dressing is the whole reason a stage exists: the census presses the clear
/// on a clip that has the thing.
struct Staged {
    pick: Pick,
    dress: fn() -> AppState,
}

/// Every pick the picture blocks offer, in the order a reader meets them.
///
/// Built from the same catalogs the panels loop over, so a filter the model
/// gains is pressed here without an edit, and a catalog that empties fails
/// `every_catalog_the_blocks_loop_over_has_members`.
fn staged() -> Vec<Staged> {
    let mut out = Vec::new();
    let mut push = |pick: Pick, dress: fn() -> AppState| out.push(Staged { pick, dress });

    for spec in aria_models::filter_catalog() {
        push(Pick::Filter(spec.id.clone()), a_clip);
    }
    push(Pick::ClearFilter, a_filtered_clip);

    for spec in mask_catalog() {
        push(Pick::Mask(spec.kind), a_clip);
    }
    push(Pick::ToggleMaskInvert, a_masked_clip);
    push(Pick::ClearMask, a_masked_clip);

    for (_, _, rgb) in KEY_COLORS {
        push(Pick::Chroma(*rgb), a_clip);
    }
    push(Pick::ClearChroma, a_keyed_clip);

    for source in MatteSource::ALL {
        push(Pick::Matte(source), a_clip);
    }
    push(Pick::ToggleMatteInvert, a_matted_clip);
    push(Pick::ClearMatte, a_matted_clip);

    for (style, _) in crate::app::clip_styles::STYLES {
        let style = *style;
        push(Pick::Style(style), a_clip);
        for (_, _, rgba) in STYLE_COLORS {
            push(Pick::StyleColor { style, rgba: *rgba }, a_styled_clip);
        }
    }
    push(Pick::ClearStyles, a_styled_clip);
    push(Pick::ClearLut, a_graded_clip);

    for mode in BlendMode::ALL {
        push(Pick::Blend(*mode), a_clip);
    }
    for level in StabilizeLevel::ALL {
        push(Pick::Stabilize(Some(level)), a_moving_clip);
    }
    push(Pick::Stabilize(None), a_stabilized_clip);
    push(Pick::ClearGrade, a_clip);
    push(Pick::ToggleDenoise, a_clip);

    push(Pick::FlipHorizontal, a_clip);
    push(Pick::FlipVertical, a_clip);
    push(Pick::ClearCrop, a_clip);
    push(Pick::ToggleMotionBlur, a_clip);

    out
}

/// Every command the picture picks asked for, one press per staged pick.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in staged() {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        let what = stage.pick.what();
        inspector_pick::apply(&mut state, stage.pick)
            .unwrap_or_else(|err| panic!("picking {what} was refused: {err}"));
        out.extend(watch.take());
    }
    out
}

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "inspector_pick.rs";

#[test]
fn every_pick_the_registry_offers_is_pressed() {
    let pressed: BTreeSet<String> = staged()
        .iter()
        .map(|stage| stage.pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "the pick registry offers picks no dressed state presses: {missing:?}. \
         The answer is a stage in `staged`, dressed so the pick has something \
         to act on, never an exception"
    );
}

#[test]
fn every_pick_pressed_asks_the_model_for_something() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        let what = stage.pick.what();
        inspector_pick::apply(&mut state, stage.pick)
            .unwrap_or_else(|err| panic!("picking {what} was refused: {err}"));
        let sent = watch.take();
        assert!(
            !sent.is_empty(),
            "picking {what} sent nothing, so either the chip is inert or its \
             stage is dressed with nothing for it to act on"
        );
    }
}

#[test]
fn the_census_reaches_every_picture_command() {
    let sent = dispatched();
    for label in [
        "Set filter",
        "Set mask",
        "Set chroma key",
        "Set track matte",
        "Set layer styles",
        "Set LUT",
        "Set blend mode",
        "Stabilize",
        "Grade",
        "Set denoise",
        "Crop",
        "Set motion blur",
    ] {
        assert!(
            sent.contains(label),
            "the picture picks sent {sent:?}, which does not include `{label}`"
        );
    }
}

/// A pick made while nothing is selected is a state the window can be in: the
/// operator clicked as the selection was replaced. It is not an error and it is
/// not an undo entry.
#[test]
fn a_pick_with_nothing_selected_is_inert() {
    for stage in staged() {
        let mut state = (stage.dress)();
        state.clear_selection();
        let watch = Watch::on(&state);
        let what = stage.pick.what();
        inspector_pick::apply(&mut state, stage.pick)
            .unwrap_or_else(|err| panic!("picking {what} with no selection: {err}"));
        let sent = watch.take();
        assert!(
            sent.is_empty(),
            "picking {what} with nothing selected sent {sent:?}"
        );
    }
}

/// The contract this slice exists for. A picture block builds a payload and
/// hands it over; the setters belong to the registry.
#[test]
fn the_picture_blocks_call_no_setter_of_their_own() {
    super::no_setter_escapes(REGISTRY, "inspector");
}

#[test]
fn every_catalog_the_blocks_loop_over_has_members() {
    assert!(
        !aria_models::filter_catalog().is_empty(),
        "the filter catalog is empty, so the Look tab draws no chips"
    );
    assert!(
        !mask_catalog().is_empty(),
        "the mask catalog is empty, so the mask block draws no shapes"
    );
    for (label, count) in [
        ("key colours", KEY_COLORS.len()),
        ("style colours", STYLE_COLORS.len()),
        ("layer styles", crate::app::clip_styles::STYLES.len()),
        ("matte sources", MatteSource::ALL.len()),
        ("blend modes", BlendMode::ALL.len()),
        ("stabilization levels", StabilizeLevel::ALL.len()),
    ] {
        assert!(count > 0, "the {label} the blocks loop over are empty");
    }
}

// --- dressed states ------------------------------------------------------
//
// Each returns a project with one clip selected, holding whatever the pick acts
// on. A clear is pressed on a clip that has the thing, because that is the only
// press that proves the clear reached the model.

fn a_clip() -> AppState {
    crate::app::premiere_tests::with_clip().0
}

fn a_filtered_clip() -> AppState {
    let mut state = a_clip();
    state.set_selected_filter("warm").expect("the filter lands");
    state
}

fn a_graded_clip() -> AppState {
    let mut state = a_clip();
    state
        .set_selected_lut("/luts/warm.cube")
        .expect("the LUT lands");
    state
}

fn a_masked_clip() -> AppState {
    let mut state = a_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("the mask lands");
    state
}

fn a_keyed_clip() -> AppState {
    let mut state = a_clip();
    state
        .set_selected_chroma_color([0, 177, 64])
        .expect("the key lands");
    state
}

fn a_matted_clip() -> AppState {
    let mut state = a_clip();
    state
        .set_selected_matte_source(MatteSource::Luma)
        .expect("the matte lands");
    state
}

/// Every layer style on, so a swatch and the clear both have something to act
/// on whichever style they belong to.
fn a_styled_clip() -> AppState {
    let mut state = a_clip();
    for (id, _) in crate::app::clip_styles::STYLES {
        state
            .toggle_selected_style(id)
            .unwrap_or_else(|err| panic!("turning on {id}: {err}"));
    }
    state
}

/// Stabilization is offered on a clip that has motion to stabilize, which is
/// video media and nothing else, so the two stabilizer picks are the only ones
/// here that need a decoded file.
fn a_moving_clip() -> AppState {
    crate::app::fixtures::with_video().0
}

fn a_stabilized_clip() -> AppState {
    let mut state = a_moving_clip();
    let level = *StabilizeLevel::ALL
        .first()
        .expect("stabilization ships a level");
    state
        .set_selected_stabilize(Some(level))
        .expect("the level lands");
    state
}
