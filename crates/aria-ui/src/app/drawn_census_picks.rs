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

use std::collections::{BTreeMap, BTreeSet};

use aria_engine::EngineError;
use aria_models::{
    BlendMode, ClipParam, MaskKind, MaskMode, MatteSource, StabilizeLevel, mask_catalog,
};

use crate::app::AppState;
use crate::app::fixtures::{the_pair, twin_selected};
use crate::app::sent_census::Watch;
use crate::surfaces::inspector_pick::{self, KEY_COLORS, Pick};

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
    // The second mask of two, so an index that never leaves the first mask
    // edits the wrong one and the command it sends says so.
    for mode in MaskMode::ALL {
        push(Pick::MaskMode { mask: 1, mode }, a_masked_clip);
    }
    push(Pick::ToggleMaskInvert(1), a_masked_clip);
    push(Pick::MoveMask { from: 1, to: 0 }, a_masked_clip);
    push(Pick::RemoveMask(1), a_masked_clip);

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
        push(Pick::Style(*style), a_clip);
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
    push(Pick::DetachClipType, a_typed_clip);

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

/// With a second clip selected that holds what the drawn clip holds, every
/// pick leaves the two alike, in at most one undo step, and that step takes
/// the pick back on both. The picks are the staged ones, so a pick added to the
/// picture blocks that reaches the drawn clip alone is red here with no edit
/// to this test.
#[test]
fn every_pick_reaches_a_second_selected_clip_holding_the_same_thing() {
    for stage in staged() {
        let mut state = (stage.dress)();
        let (drawn, twin) = twin_selected(&mut state);
        let before = the_pair(&state, drawn, twin);
        let what = stage.pick.what();
        assert_eq!(
            before.1, before.0,
            "the copy for {what} differs from its clip"
        );
        let depth = state.history_depth();
        inspector_pick::apply(&mut state, stage.pick)
            .unwrap_or_else(|err| panic!("picking {what} on two clips was refused: {err}"));
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

#[test]
fn the_census_reaches_every_picture_command() {
    let sent = dispatched();
    for label in [
        "Set filter",
        "Add mask",
        "Set mask",
        "Remove mask",
        "Reorder masks",
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
        "Set clip type",
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

/// The catalog groups whose keys go with the block a pick removes. No wildcard
/// arm: a pick added to the registry states what it clears before it compiles.
fn clears(pick: &Pick) -> &'static [&'static str] {
    match pick {
        Pick::ClearFilter => &["filter"],
        Pick::ClearChroma => &["chroma"],
        Pick::ClearMatte => &["matte"],
        Pick::ClearStyles => &["shadow", "glow", "outline", "background"],
        Pick::ClearLut => &["lut"],
        Pick::ClearGrade => &["grade"],
        Pick::ClearCrop => &["framing"],
        Pick::Filter(_)
        | Pick::Mask(_)
        | Pick::MaskMode { .. }
        | Pick::ToggleMaskInvert(_)
        | Pick::MoveMask { .. }
        | Pick::RemoveMask(_)
        | Pick::Chroma(_)
        | Pick::Matte(_)
        | Pick::ToggleMatteInvert
        | Pick::Style(_)
        | Pick::Blend(_)
        | Pick::Stabilize(_)
        | Pick::ToggleDenoise
        | Pick::FlipHorizontal
        | Pick::FlipVertical
        | Pick::ToggleMotionBlur
        | Pick::DetachClipType => &[],
    }
}

/// Every keyed property of the selected clip: its catalog group and the ticks
/// of its keys, by id.
fn keys_by_id(state: &AppState) -> BTreeMap<String, (String, Vec<i64>)> {
    state
        .key_params()
        .iter()
        .map(|row| {
            let ticks = row.marks.iter().map(|mark| mark.tick).collect();
            (row.id.to_string(), (row.spec.group.to_string(), ticks))
        })
        .collect()
}

/// Run `write`, require every key outside the groups in `cleared` to be where
/// it was on each property still offered, then undo back to where it started.
fn assert_keeps_keys(
    state: &mut AppState,
    what: &str,
    cleared: &[&str],
    write: impl FnOnce(&mut AppState) -> Result<(), EngineError>,
) {
    let before = keys_by_id(state);
    let depth = state.history_depth();
    write(state).unwrap_or_else(|err| panic!("{what} was refused: {err}"));
    let after = keys_by_id(state);
    for (id, (group, ticks)) in &before {
        if cleared.contains(&group.as_str()) {
            continue;
        }
        if let Some((_, now)) = after.get(id) {
            assert_eq!(now, ticks, "{what} moved the keys on {id}");
        }
    }
    while state.history_depth() > depth {
        assert!(state.undo(), "{what} left a step undo could not take");
    }
    assert_eq!(keys_by_id(state), before, "undoing {what} lost keys");
}

/// A structural pick sends a whole block: a filter, a key, a matte, the four
/// layer styles, the framing. Each setter used to rebuild that block from the
/// projection, which holds one value per property, so a pick flattened every
/// keyframe inside the block it wrote: a new key color erased an animated
/// strength, a style toggle every animated style knob, a mirror an animated
/// crop. Here every property the clip offers is keyed twice, and every staged
/// pick, then a LUT reload, must leave each key where it was outside the groups
/// `clears` names for it. The picks are the staged ones, so a pick added to
/// the registry is pressed here with no edit to this test. What this does not
/// see: a writer reached outside the registry other than the LUT load, such as
/// the crop edges, which `frame_tests` holds.
#[test]
fn no_pick_moves_a_key_it_does_not_clear() {
    let mut state = a_clip_keyed_in_every_block();
    let keyed = keys_by_id(&state);
    for (id, (_, ticks)) in &keyed {
        assert_eq!(ticks.len(), 2, "the fixture did not key {id} twice");
    }
    let groups: BTreeSet<&str> = keyed.values().map(|(group, _)| group.as_str()).collect();
    for stage in staged() {
        for group in clears(&stage.pick) {
            assert!(
                groups.contains(group),
                "{} clears {group}, which the fixture never keyed",
                stage.pick.what()
            );
        }
    }

    for stage in staged() {
        let what = stage.pick.what();
        let cleared = clears(&stage.pick);
        let pick = stage.pick;
        assert_keeps_keys(&mut state, what, cleared, |state| {
            inspector_pick::apply(state, pick)
        });
    }
    assert_keeps_keys(&mut state, "a LUT reload", &[], |state| {
        state.set_selected_lut("/luts/cool.cube")
    });
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
        ("key colors", KEY_COLORS.len()),
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

/// Two masks, so a pick on the second one has a first one to leave alone.
fn a_masked_clip() -> AppState {
    let mut state = a_clip();
    for kind in [MaskKind::Circle, MaskKind::Rectangle] {
        state.add_selected_mask(kind).expect("the mask lands");
    }
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

/// Every layer style on, so the clear has something to act on whichever style
/// it belongs to, and every style's color row is drawn for the swatch census.
pub(super) fn a_styled_clip() -> AppState {
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

/// A clip holding every block a pick reaches, every property it offers keyed
/// at its first frame and its middle, and the playhead between the two. Video
/// media, so the stabilizer picks act on it too.
fn a_clip_keyed_in_every_block() -> AppState {
    let mut state = a_moving_clip();
    state.set_selected_filter("warm").expect("the filter lands");
    for kind in [MaskKind::Circle, MaskKind::Rectangle] {
        state.add_selected_mask(kind).expect("the mask lands");
    }
    state
        .set_selected_chroma_color([0, 177, 64])
        .expect("the key lands");
    state
        .set_selected_matte_source(MatteSource::Luma)
        .expect("the matte lands");
    for (id, _) in crate::app::clip_styles::STYLES {
        state
            .toggle_selected_style(id)
            .unwrap_or_else(|err| panic!("turning on {id}: {err}"));
    }
    state
        .set_selected_lut("/luts/warm.cube")
        .expect("the LUT lands");

    let clip = state.selected_clip().expect("a selected clip");
    let model = state.selected_model().expect("the selected model");
    let start = model.timeline.start.value;
    let mid = start + (model.timeline.end_tick() - start) / 2;
    let params: Vec<ClipParam> = state.key_params().iter().map(|row| row.param()).collect();
    // One undo step: a history at its limit drops its oldest step as each pick
    // lands, so the undo back to the starting depth would take nothing.
    state
        .one_step(|state| {
            for param in params {
                for tick in [start, mid] {
                    state
                        .add_param_keyframe_at(clip, param, tick)
                        .unwrap_or_else(|err| panic!("keying {param:?} at {tick}: {err}"));
                }
            }
            Ok::<(), EngineError>(())
        })
        .expect("the keys land");
    state.set_playhead(start + (mid - start) / 2);
    state
}

/// A clip holding a clip type. The model takes any well-formed id, and no
/// plugin need list it: a project opened without the plugin holds one too.
fn a_typed_clip() -> AppState {
    let mut state = a_clip();
    let clip = state.selected_clip().expect("a selected clip");
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetClipType {
                clip,
                clip_type: Some("test.census.lower-third".into()),
            },
        ))
        .expect("a clip type");
    state
}
