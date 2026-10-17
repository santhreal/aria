//! Driving the preview's on-canvas handles: the move, the four scale corners,
//! the eight crop handles, the three mask handles, a clip type's handles, a
//! motion path's keyframes, tangents and segments, and a mask path's vertices,
//! tangents and segments.
//!
//! # Why
//!
//! `Transform` claimed the preview's handles and nothing pressed them. Which
//! handle set is live is the overlay mode's answer, and a crop corner and a
//! scale corner land on the same pixel when the crop is full, so a corner wired
//! to the wrong verb is invisible by inspection: it draws in the right place and
//! edits the wrong thing. The registry is
//! [`overlay_gesture`](crate::surfaces::overlay_gesture) and this drags every
//! handle it names.
//!
//! A drag is three steps and the census makes all three: the press, one move,
//! and the release. Only the release edits, so a census that pressed and
//! released without moving would measure nothing at all, which is its own test
//! below.
//!
//! The canvas geometry is stated rather than rendered: `framed` puts the picture
//! at a known place, because a gate has no window to paint one in.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use aria_models::{ClipTypeSpec, MarkerColor, MaskKind, Registered, register_clip_type};

use crate::app::AppState;
use crate::app::canvas_overlay::motion_path::{PATH_HIT, PathKey, PathParam, TangentSide};
use crate::app::canvas_overlay::{CropHandle, MaskHandle, TransformHandle};
use crate::app::fixtures::keyed_path;
use crate::app::mask_path::tests::{mask_path_press, masked_path};
use crate::app::sent_census::Watch;
use crate::app::transform_handle_tests::{framed, selected};
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::types::EditorTool;

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "overlay_gesture.rs";

/// How far the pointer travels, in window pixels. Both axes move, so a scale
/// that reads one of them still changes, and none of the three sets lands back
/// on its own value: an idempotent write is a write nothing can see.
const TRAVEL: [f32; 2] = [60.0, 30.0];

/// Where a drag on one handle starts, in window pixels.
///
/// A handle is grabbed where it is drawn. That is not decoration: scaling
/// measures the pointer's distance from the layer's centre, so a corner grabbed
/// at the centre has no distance to grow from and the whole gesture is a no-op.
/// A crop or mask press carries no position at all, and the centre is as good a
/// place as any to start one from. A motion path is pressed where it is drawn,
/// because a press on a path finds what it edits by where it lands.
fn grab(state: &AppState, gesture: Gesture) -> [f32; 2] {
    let layer = state
        .overlay_frame
        .layer
        .expect("`framed` declared the picture's rectangle");
    match gesture {
        Gesture::Scale(handle) => layer.at(handle.anchor()),
        Gesture::TypeHandle(index) => layer.at(*state
            .overlay_type_handles()
            .get(index)
            .expect("the census clip draws every handle its type declares")),
        Gesture::Move | Gesture::Crop(_) | Gesture::Mask(_) => layer.at([0.5, 0.5]),
        Gesture::PathKey(key) | Gesture::PathCorner(key) => drawn_key(state, key).at,
        Gesture::PathTangent(key, side) => drawn_key(state, key)
            .handle(side)
            .expect("the census path's smooth keyframe draws both handles"),
        Gesture::PathAdd => off_the_keys(state),
        Gesture::MaskVertex(_)
        | Gesture::MaskTangent(..)
        | Gesture::MaskVertexDelete(_)
        | Gesture::MaskVertexConvert(_)
        | Gesture::MaskPathPen => mask_path_press(state, gesture),
    }
}

/// The census path's keyframe `key` as the preview draws it.
fn drawn_key(state: &AppState, key: PathKey) -> crate::app::canvas_overlay::motion_path::DrawnKey {
    let paths = state.motion_paths();
    let path = paths
        .iter()
        .find(|path| path.param == key.param)
        .expect("`keyed_path` keyed the census clip's Position");
    *path
        .keys
        .get(key.index)
        .expect("the census path draws every keyframe it holds")
}

/// A drawn dot on the census path that no keyframe sits on or near, which is
/// where a Ctrl+click adds one.
fn off_the_keys(state: &AppState) -> [f32; 2] {
    let paths = state.motion_paths();
    let path = paths.first().expect("`keyed_path` draws a path");
    let clear = |dot: &[f32; 2]| {
        path.keys
            .iter()
            .all(|key| (dot[0] - key.at[0]).hypot(dot[1] - key.at[1]) > 2.0 * PATH_HIT)
    };
    *path
        .dots
        .iter()
        .find(|dot| clear(dot))
        .expect("the census path has a segment between its keyframes")
}

/// The census path's first keyframe, a corner, and its second, a smooth one.
const CORNER: PathKey = PathKey {
    param: PathParam::Position,
    index: 0,
};
const SMOOTH: PathKey = PathKey {
    param: PathParam::Position,
    index: 1,
};

/// Whether a gesture presses a motion path, which the census clip draws only
/// once its Position is keyed.
fn on_path(gesture: Gesture) -> bool {
    matches!(
        gesture,
        Gesture::PathKey(_) | Gesture::PathTangent(..) | Gesture::PathCorner(_) | Gesture::PathAdd
    )
}

/// Whether a gesture presses a mask path, which the census clip draws only
/// once its mask is a path.
fn on_mask_path(gesture: Gesture) -> bool {
    matches!(
        gesture,
        Gesture::MaskVertex(_)
            | Gesture::MaskTangent(..)
            | Gesture::MaskVertexDelete(_)
            | Gesture::MaskVertexConvert(_)
            | Gesture::MaskPathPen
    )
}

/// Whether only the Pen makes a gesture: deleting or converting a vertex, and
/// inserting one on a segment.
fn pen_only(gesture: Gesture) -> bool {
    matches!(
        gesture,
        Gesture::MaskVertexDelete(_) | Gesture::MaskVertexConvert(_) | Gesture::MaskPathPen
    )
}

/// Where that drag ends.
fn release(from: [f32; 2]) -> [f32; 2] {
    [from[0] + TRAVEL[0], from[1] + TRAVEL[1]]
}

/// The clip type the census clip holds, and the vec2 parameters it draws as
/// handles. A lib test registers no effect, so the type names the shipped
/// `color_overlay`: a handle needs a vec2 slot in the chain, not a generator.
const TYPED: &str = "test.overlay-census.offset";
const TYPED_HANDLES: [&str; 1] = ["offset"];

/// List [`TYPED`] for the life of the test binary.
fn typed() {
    static HELD: OnceLock<Registered> = OnceLock::new();
    HELD.get_or_init(|| {
        let mut spec = ClipTypeSpec::new(
            TYPED,
            "Overlay census type",
            "color_overlay",
            MarkerColor::Orange,
        );
        spec.handles = TYPED_HANDLES.map(String::from).to_vec();
        register_clip_type(spec)
            .expect("the census clip type registers")
            .0
    });
}

/// A selected, framed clip carrying a mask and a clip type, so every handle
/// set is drawn.
fn framed_clip() -> AppState {
    typed();
    let mut state = selected();
    framed(&mut state);
    state
        .add_selected_mask(MaskKind::Circle)
        .expect("a mask to drag");
    state
        .add_effect_to_selected("color_overlay")
        .expect("the type's effect");
    let clip = state.selected_clip().expect("a selected clip");
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetClipType {
                clip,
                clip_type: Some(TYPED.into()),
            },
        ))
        .expect("the clip type");
    state
}

/// [`framed_clip`], with its Position keyed for a gesture on a motion path.
///
/// Only then: a keyed Position turns a move of the picture into a keyframe
/// edit, which is the move's own business and not this census's.
fn dressed(gesture: Gesture) -> AppState {
    let mut state = framed_clip();
    if on_path(gesture) {
        keyed_path(&mut state);
    }
    if on_mask_path(gesture) {
        masked_path(&mut state);
    }
    if pen_only(gesture) {
        state.tool = EditorTool::Pen;
    }
    state
}

/// Every handle the preview draws, in the order the panel draws them.
///
/// The three catalogs are the ones the panel loops over, so a fourth crop
/// handle or a second mask control is dragged here without an edit.
fn gestures() -> Vec<Gesture> {
    let mut out = vec![Gesture::Move];
    out.extend(TransformHandle::ALL.into_iter().map(Gesture::Scale));
    out.extend(CropHandle::ALL.into_iter().map(Gesture::Crop));
    out.extend(MaskHandle::ALL.into_iter().map(Gesture::Mask));
    out.extend((0..TYPED_HANDLES.len()).map(Gesture::TypeHandle));
    out.push(Gesture::PathKey(CORNER));
    out.extend(TangentSide::ALL.map(|side| Gesture::PathTangent(SMOOTH, side)));
    out.extend([CORNER, SMOOTH].map(Gesture::PathCorner));
    out.push(Gesture::PathAdd);
    out.push(Gesture::MaskVertex(0));
    out.extend(TangentSide::ALL.map(|side| Gesture::MaskTangent(1, side)));
    out.extend([
        Gesture::MaskVertexDelete(0),
        Gesture::MaskVertexConvert(0),
        Gesture::MaskPathPen,
    ]);
    out
}

/// The commands the handle sets write, one each.
const SETS: [&str; 5] = [
    "Transform",
    "Crop",
    "Set mask",
    "Set keyframe",
    "Set parameter",
];

/// The command one handle set writes.
fn writes(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::Move | Gesture::Scale(_) => "Transform",
        Gesture::Crop(_) => "Crop",
        Gesture::Mask(_)
        | Gesture::MaskVertex(_)
        | Gesture::MaskTangent(..)
        | Gesture::MaskVertexDelete(_)
        | Gesture::MaskVertexConvert(_)
        | Gesture::MaskPathPen => "Set mask",
        // A generator parameter with no curve takes the value as a constant.
        Gesture::TypeHandle(_) => "Set parameter",
        Gesture::PathKey(_)
        | Gesture::PathTangent(..)
        | Gesture::PathCorner(_)
        | Gesture::PathAdd => "Set keyframe",
    }
}

/// Drag one handle the way the pointer does: press, move, release.
fn drag(state: &mut AppState, gesture: Gesture) {
    state.set_overlay_mode(gesture.mode());
    let from = grab(state, gesture);
    step(state, Step::Grab(gesture, from));
    step(state, Step::Carry(release(from)));
    step(state, Step::Release);
}

/// One step, refusing to pass a refusal off as a drag.
fn step(state: &mut AppState, step: Step) {
    overlay_gesture::apply(state, step)
        .unwrap_or_else(|err| panic!("a drag on the picture was refused: {err}"));
}

/// Every command a handle drag asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for gesture in gestures() {
        let mut state = dressed(gesture);
        let watch = Watch::on(&state);
        drag(&mut state, gesture);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_handle_the_preview_draws_is_dragged() {
    // Fail by default on a new arm: `what` is a total match, so the compiler
    // forces prose for it and this forces a drag.
    let dragged: BTreeSet<String> = gestures()
        .into_iter()
        .map(|gesture| gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !dragged.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these canvas handles are drawn and nothing drags them: {missing:?}"
    );
}

#[test]
fn a_handle_sends_the_command_its_own_set_owns() {
    // The defect the missing surface hid: a corner in the wrong set. Every
    // handle is asserted to write its own set's command and nothing from
    // another set, because the sets share their pixels.
    for gesture in gestures() {
        let label = writes(gesture);
        let mut state = dressed(gesture);
        let watch = Watch::on(&state);
        drag(&mut state, gesture);
        let sent = watch.take();
        assert!(
            sent.contains(&label),
            "{} sent {sent:?} rather than `{label}`",
            gesture.what()
        );
        for other in SETS {
            if other == label {
                continue;
            }
            assert!(
                !sent.contains(&other),
                "{} reached into the {other} set as well: {sent:?}",
                gesture.what()
            );
        }
    }
}

#[test]
fn a_press_that_never_moved_is_a_click_rather_than_an_edit() {
    // Selecting the picture is a press on the same surface a move starts from,
    // and a clip type handle and a motion path sit on that surface. An edit here
    // would put a no-op on the undo stack for every click.
    for gesture in [
        Gesture::Move,
        Gesture::TypeHandle(0),
        Gesture::PathKey(CORNER),
        Gesture::PathTangent(SMOOTH, TangentSide::Out),
        Gesture::MaskVertex(0),
        Gesture::MaskTangent(1, TangentSide::Out),
    ] {
        let mut state = dressed(gesture);
        let watch = Watch::on(&state);
        state.set_overlay_mode(gesture.mode());
        let from = grab(&state, gesture);
        step(&mut state, Step::Grab(gesture, from));
        step(&mut state, Step::Release);
        assert!(
            watch.take().is_empty(),
            "a press on {} with no movement wrote an edit",
            gesture.what()
        );
    }
}

#[test]
fn a_type_handle_drafts_the_preview_and_commits_one_undoable_value() {
    // The handle and the picture both follow the pointer before the release,
    // the release writes the value the handle drew, and the whole drag is one
    // undo step.
    let mut state = framed_clip();
    let gesture = Gesture::TypeHandle(0);
    state.set_overlay_mode(gesture.mode());
    let before = state.overlay_type_handles();
    let from = grab(&state, gesture);
    step(&mut state, Step::Grab(gesture, from));
    step(&mut state, Step::Carry(release(from)));
    let mid = state.overlay_type_handles();
    assert_ne!(
        mid, before,
        "mid-drag the handle still reads the committed value"
    );
    assert!(
        state.engine.lock().has_live_overrides(),
        "mid-drag the preview shows the committed value"
    );
    step(&mut state, Step::Release);
    assert!(
        !state.engine.lock().has_live_overrides(),
        "the release left the draft in the preview"
    );
    assert_eq!(
        state.overlay_type_handles(),
        mid,
        "the release committed a value other than the one the drag drew"
    );
    assert!(state.undo(), "the drag left nothing to undo");
    assert_eq!(
        state.overlay_type_handles(),
        before,
        "one undo did not restore the handle"
    );
}

#[test]
fn a_cancelled_type_handle_drag_drops_its_draft() {
    let mut state = framed_clip();
    let watch = Watch::on(&state);
    let gesture = Gesture::TypeHandle(0);
    state.set_overlay_mode(gesture.mode());
    let before = state.overlay_type_handles();
    let from = grab(&state, gesture);
    step(&mut state, Step::Grab(gesture, from));
    step(&mut state, Step::Carry(release(from)));
    state.cancel_overlay_drag();
    assert!(watch.take().is_empty(), "cancelling the drag committed it");
    assert!(
        !state.engine.lock().has_live_overrides(),
        "the cancelled drag left its draft in the preview"
    );
    assert_eq!(state.overlay_type_handles(), before);
}

#[test]
fn the_picture_follows_the_drag_before_it_is_committed() {
    // The handles read a draft while the drag is in flight, and the preview is
    // drafted with it: an outline sliding over a stale frame is the class this
    // closes. Asserted as the draft the handles read, because a gate has no
    // window to read pixels from.
    let mut state = framed_clip();
    let before = state.overlay_transform().expect("a selected transform");
    state.set_overlay_mode(Gesture::Move.mode());
    let from = grab(&state, Gesture::Move);
    step(&mut state, Step::Grab(Gesture::Move, from));
    step(&mut state, Step::Carry(release(from)));
    let mid = state.overlay_transform().expect("a drafted transform");
    assert_ne!(
        mid.position_x, before.position_x,
        "mid-drag the handles still read the committed position"
    );
    step(&mut state, Step::Release);
    assert!(
        !state.overlay_dragging(),
        "the drag outlived its own release"
    );
}

#[test]
fn a_cancelled_drag_leaves_the_project_alone() {
    // Every way out of a drag has to drop the draft, or the preview stays frozen
    // on a frame the project never received.
    let mut state = framed_clip();
    let watch = Watch::on(&state);
    state.set_overlay_mode(Gesture::Move.mode());
    let from = grab(&state, Gesture::Move);
    step(&mut state, Step::Grab(Gesture::Move, from));
    step(&mut state, Step::Carry(release(from)));
    state.cancel_overlay_drag();
    assert!(
        watch.take().is_empty(),
        "cancelling a drag committed it anyway"
    );
    assert!(
        !state.overlay_dragging(),
        "the cancelled drag is still live"
    );
}

#[test]
fn the_census_reaches_every_command_these_handles_claim() {
    let sent = dispatched();
    for label in SETS {
        assert!(
            sent.contains(label),
            "the overlay census dragged every handle the preview draws and none \
             of them sent `{label}`"
        );
    }
}

#[test]
fn the_preview_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "preview");
}
