//! Driving the inspector's property blocks: every group in the model's property
//! catalog, nudged through the row the panel draws.
//!
//! # Why
//!
//! An inspector block used to list the properties it drew. That cost a property
//! every time the model gained one, and the losses were real: the transform's
//! anchor point, all seven shape properties and all twelve text properties were
//! drawn nowhere. Worse, a hand-listed row wrote a block-shaped command, which
//! carries the whole block, and the panel had only constants to put in it, so
//! moving the volume slider on a clip with a fade flattened the fade.
//!
//! A block now names its group and nothing else, and
//! [`AppState::param_rows`](crate::app::AppState::param_rows) answers with what
//! the selected clip owns. So the registry this census drives is the catalog
//! itself, read at run time: a group the model gains has no stage, no dressed
//! state and no panel block until someone writes them, and the tests below go
//! red until they exist.
//!
//! Two things this census does not cover, each for a stated reason. A group
//! whose properties are all colours or rectangles yields no stepper rows at all
//! — a swatch and the crop handles own those — so `framing` is skipped by that
//! derivation rather than by an exception. And `effect-<index>` is a group per
//! effect instance rather than a shipped block: the effects rack is its own
//! surface and its own slice of ledger row 140.

use std::collections::BTreeSet;

use aria_engine::EngineError;
use aria_models::{
    ClipId, Generator, MaskKind, MatteSource, Param, ParamKind, Shape, ShapeStroke,
    clip_param_catalog,
};

use crate::app::AppState;
use crate::app::clip_params::ParamRow;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::surfaces::param_units;

/// One property group, with a project dressed so that group owns rows.
///
/// A block's rows only exist when the thing they belong to does: an unmasked
/// clip has no mask rows, a clip with no glow has no glow rows. The dressing is
/// the whole reason a stage exists.
struct Stage {
    /// The catalog group the block draws.
    group: &'static str,
    dress: fn() -> AppState,
}

/// One dressed state per group that can yield a row.
///
/// `every_group_that_can_yield_a_row_has_a_stage` holds this list to the
/// catalog, so a new group is a red test rather than a silent gap, and
/// `every_stage_owns_rows_in_its_group` holds each dressing to actually
/// producing rows.
const STAGES: &[Stage] = &[
    Stage {
        group: "transform",
        dress: a_clip,
    },
    Stage {
        group: "grade",
        dress: a_clip,
    },
    Stage {
        group: "audio",
        dress: a_clip_with_sound,
    },
    Stage {
        group: "filter",
        dress: a_filtered_clip,
    },
    Stage {
        group: "lut",
        dress: a_graded_clip,
    },
    Stage {
        group: "mask",
        dress: a_masked_clip,
    },
    Stage {
        group: "chroma",
        dress: a_keyed_clip,
    },
    Stage {
        group: "matte",
        dress: a_matted_clip,
    },
    Stage {
        group: "shadow",
        dress: a_clip_with_a_shadow,
    },
    Stage {
        group: "glow",
        dress: a_clip_with_a_glow,
    },
    Stage {
        group: "outline",
        dress: a_clip_with_an_outline,
    },
    Stage {
        group: "background",
        dress: a_clip_with_a_card,
    },
    Stage {
        group: "text",
        dress: a_title,
    },
    Stage {
        group: "shape",
        dress: a_shape,
    },
];

/// Nudge one row the way the drawn row does.
///
/// A generated row hands the stepper `nudge_param`; a row with units of its own
/// hands it the write those units declare. Pressing the wrong one would measure
/// this file rather than the panel.
fn nudge(state: &mut AppState, group: &str, row: &ParamRow) -> Result<(), EngineError> {
    match param_units::of_param(group, row.param) {
        Some(units) => (units.write)(state, units.step),
        None => state.nudge_param(row.param, row.component, row.step()),
    }
}

/// Every command the property rows asked for, one nudge per row.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in STAGES {
        let mut state = (stage.dress)();
        let watch = Watch::on(&state);
        for row in state.param_rows(stage.group) {
            nudge(&mut state, stage.group, &row)
                .unwrap_or_else(|err| panic!("nudging {} was refused: {err}", row.id));
        }
        out.extend(watch.take());
    }
    out
}

/// Every catalog group holding at least one property a stepper can edit.
///
/// The derivation, not a list: a scalar or a vector becomes a row, a colour
/// wants a swatch and a rectangle wants handles.
fn groups_with_rows() -> BTreeSet<String> {
    clip_param_catalog()
        .iter()
        .filter(|spec| matches!(spec.kind, ParamKind::Scalar | ParamKind::Vec2))
        .map(|spec| spec.group.to_string())
        .collect()
}

/// Every group an inspector block draws, from the calls the panels make.
///
/// One spelling and one registry. A block that draws a group passes its name to
/// `param_rows`, so the name is in its source. The layer-style blocks are a loop
/// over `app::clip_styles::STYLES`, whose id *is* the group name, so the registry
/// answers for those four rather than the source text;
/// `the_style_blocks_draw_their_group` pins that the loop still passes the id.
fn drawn_groups() -> BTreeSet<String> {
    let mut out: BTreeSet<String> = crate::app::clip_styles::STYLES
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect();
    for source in inspector_sources() {
        for rest in source.split("param_rows(theme, \"").skip(1) {
            let (group, _) = rest
                .split_once('"')
                .expect("a group name is a quoted string");
            out.insert(group.to_string());
        }
    }
    out
}

/// The source of every file the inspector is built from.
fn inspector_sources() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/panels/inspector");
    let mut files = Vec::new();
    crate::test_support::collect_rs(&dir, &mut files);
    let mut out = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.ends_with("_tests.rs") {
            out.push(std::fs::read_to_string(&path).expect("an inspector file is readable"));
        }
    }
    assert!(!out.is_empty(), "the inspector has no source files");
    out
}

#[test]
fn every_group_that_can_yield_a_row_has_a_stage() {
    let staged: BTreeSet<&str> = STAGES.iter().map(|stage| stage.group).collect();
    let missing: Vec<String> = groups_with_rows()
        .into_iter()
        .filter(|group| !staged.contains(group.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "the model has property groups no dressed state reaches: {missing:?}. \
         Each wants a stage in STAGES holding a clip that owns its rows, not an \
         exception here"
    );
}

#[test]
fn every_stage_names_a_group_the_model_has() {
    let known = groups_with_rows();
    for stage in STAGES {
        assert!(
            known.contains(stage.group),
            "the stage for `{}` names a group the catalog does not hold, so it \
             measures nothing",
            stage.group
        );
    }
}

#[test]
fn every_stage_owns_rows_in_its_group() {
    for stage in STAGES {
        let state = (stage.dress)();
        let rows = state.param_rows(stage.group);
        assert!(
            !rows.is_empty(),
            "the state dressed for `{}` owns no rows in that group, so the \
             census presses nothing: the dressing is wrong, or the group's \
             properties are not reachable on that clip",
            stage.group
        );
    }
}

#[test]
fn every_row_a_block_draws_asks_for_a_parameter_edit() {
    for stage in STAGES {
        let mut state = (stage.dress)();
        for row in state.param_rows(stage.group) {
            let watch = Watch::on(&state);
            nudge(&mut state, stage.group, &row)
                .unwrap_or_else(|err| panic!("nudging {} was refused: {err}", row.id));
            let sent = watch.take();
            assert!(
                !sent.is_empty(),
                "nudging `{}` in the {} block sent nothing, so the row is inert",
                row.id,
                stage.group
            );
        }
    }
}

#[test]
fn the_census_reaches_the_parameter_command() {
    let sent = dispatched();
    assert!(
        sent.contains("Set parameter"),
        "the property rows sent {sent:?}, which does not include the command \
         they are drawn from"
    );
}

/// An override is a scale, not a second way to write. A row that reads in its
/// own units still writes through the catalog's keyframe-aware path, which is the
/// whole reason the gain row stopped flattening fades.
#[test]
fn a_row_with_its_own_units_writes_through_the_catalog() {
    for (group, units) in param_units::OVERRIDES {
        let stage = STAGES
            .iter()
            .find(|stage| stage.group == *group)
            .unwrap_or_else(|| panic!("the {group} block has units but no dressed state"));
        for unit in *units {
            let mut state = (stage.dress)();
            let row = state
                .param_rows(group)
                .into_iter()
                .find(|row| row.param == unit.param)
                .unwrap_or_else(|| {
                    panic!("the {group} block declares units for a property it does not draw")
                });
            let watch = Watch::on(&state);
            (unit.write)(&mut state, unit.step)
                .unwrap_or_else(|err| panic!("riding {}: {err}", row.id));
            let sent = watch.take();
            assert_eq!(
                sent,
                vec!["Set parameter"],
                "riding `{}` sent {sent:?}: a row with its own units must still \
                 write one property, not a command carrying the whole block",
                row.id
            );
        }
    }
}

/// The reading half of the same rule: a scale is declared for a property the
/// catalog holds, so an override that outlives its property goes red.
#[test]
fn every_declared_scale_names_a_property_that_is_drawn() {
    let groups = groups_with_rows();
    for (group, units) in param_units::OVERRIDES {
        assert!(
            groups.contains(*group),
            "`{group}` has a declared scale and no property group"
        );
        assert!(!units.is_empty(), "`{group}` declares an empty scale");
    }
}

#[test]
fn every_row_the_inspector_draws_has_its_own_id() {
    let mut seen: BTreeSet<gpui::SharedString> = BTreeSet::new();
    for stage in STAGES {
        let state = (stage.dress)();
        for row in state.param_rows(stage.group) {
            assert!(
                seen.insert(row.id.clone()),
                "two rows answer to `{}`, so one steals the other's element id \
                 and its keyframe dot",
                row.id
            );
        }
    }
}

/// The class the hand-listed blocks broke: a block-shaped command carries every
/// property in the block, so a panel that rebuilt one from constants deleted the
/// curve on the property beside the one being dragged.
#[test]
fn a_nudge_keeps_the_curve_the_property_carries() {
    for stage in STAGES {
        let mut state = (stage.dress)();
        for row in state.param_rows(stage.group) {
            let clip = state
                .selected_ui_clip()
                .expect("the dressed state has a selection");
            let start = clip.timeline_start_ticks;
            let end = clip.timeline_end_ticks;
            let mid = start + (end - start) / 2;
            assert!(mid > start, "the fixture clip is too short to key twice");

            state.set_playhead(start);
            state
                .add_keyframe_at_playhead(row.param)
                .unwrap_or_else(|err| panic!("pinning {} at its head: {err}", row.id));
            state.set_playhead(mid);
            state
                .add_keyframe_at_playhead(row.param)
                .unwrap_or_else(|err| panic!("pinning {} at its middle: {err}", row.id));
            let keyed = state.keyframes_of(row.param).len();
            assert_eq!(
                keyed, 2,
                "`{}` did not take two keyframes, so this row cannot measure a \
                 flattened curve",
                row.id
            );

            nudge(&mut state, stage.group, &row)
                .unwrap_or_else(|err| panic!("nudging {}: {err}", row.id));
            assert_eq!(
                state.keyframes_of(row.param).len(),
                keyed,
                "nudging `{}` in the {} block changed how many keyframes it \
                 carries: the write flattened the curve instead of moving a key",
                row.id,
                stage.group
            );
        }
    }
}

#[test]
fn every_group_with_rows_is_drawn_by_a_block() {
    let drawn = drawn_groups();
    let missing: Vec<String> = groups_with_rows()
        .into_iter()
        .filter(|group| !drawn.contains(group.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "the model has property groups the inspector draws no block for: \
         {missing:?}. A block is `param_rows(theme, \"<group>\", state, cx)` in \
         one of the inspector's files. This test reads calls and the style \
         registry; it cannot see whether the block is reachable through a tab, \
         which `tab_host`'s own tests hold"
    );
}

/// The four layer-style blocks are a loop, so `drawn_groups` trusts the registry
/// for them. This is the line that makes that trust true.
#[test]
fn the_style_blocks_draw_their_group() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/panels/inspector/blocks/styles.rs"),
    )
    .expect("the styles block is readable");
    assert!(
        source.contains("param_rows(theme, style, state, cx)"),
        "the layer-style block no longer draws the rows of the style it is \
         looping over, so `drawn_groups` credits it with four groups it does \
         not draw"
    );
    let groups = groups_with_rows();
    for (id, _) in crate::app::clip_styles::STYLES {
        assert!(
            groups.contains(*id),
            "the style `{id}` names no property group, so its block draws \
             nothing"
        );
    }
}

// --- dressed states ------------------------------------------------------
//
// Each returns a project with one clip selected, holding whatever the group's
// rows belong to. Nothing here writes a property: a census that set up its rows
// by hand would measure the setter it is about to test.

fn a_clip() -> AppState {
    with_clip().0
}

/// A clip on an audio lane. The audio properties are media-only, and the lane
/// also drops every visual row, which is the state the audio block draws in.
fn a_clip_with_sound() -> AppState {
    crate::app::fixtures::with_audio().0
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

fn with_style(id: &str) -> AppState {
    let mut state = a_clip();
    state
        .toggle_selected_style(id)
        .unwrap_or_else(|err| panic!("turning on {id}: {err}"));
    state
}

fn a_clip_with_a_shadow() -> AppState {
    with_style("shadow")
}

fn a_clip_with_a_glow() -> AppState {
    with_style("glow")
}

fn a_clip_with_an_outline() -> AppState {
    with_style("outline")
}

fn a_clip_with_a_card() -> AppState {
    with_style("background")
}

fn a_title() -> AppState {
    generated(Generator::text("Title"))
}

/// A star with an outline, because two shape properties only exist on a shape
/// that has them: the inner ratio belongs to a star and the stroke width to a
/// shape that is stroked.
fn a_shape() -> AppState {
    generated(Generator::Shape {
        shape: Shape::Star {
            points: 5,
            inner_ratio: Param::Constant(0.5),
        },
        rgba: Param::Constant([255, 255, 255, 255]),
        width: Param::Constant(200.0),
        height: Param::Constant(120.0),
        corner_radius: Param::Constant(4.0),
        stroke: Some(ShapeStroke::new([0, 0, 0, 255], 2.0)),
    })
}

/// Place a generated clip and select it.
///
/// Found by content rather than by `landing_clip`, which expires: a census that
/// raced a settling animation would dress nothing under load.
fn generated(generator: Generator) -> AppState {
    let mut state = a_clip();
    state
        .add_generated(generator)
        .expect("the generated clip lands on the timeline");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the generated clip is on the timeline")
        .id;
    state.select_clip(ClipId::from_raw(placed));
    state
}
