//! `AppState` overlay: 14 methods moved out of `canvas_overlay.rs`.
//!
//! A pure move to stay under the file ceiling. Inherent methods keep their
//! paths wherever they are written, because a method belongs to the type and
//! not to the file, so no caller changes.

use super::*;

impl AppState {
    /// Record the picture area the preview painted, deriving the canvas rectangle
    /// and the selected clip's quad from it. Called from the paint pass, which is
    /// the only place the drawn geometry is known.
    ///
    /// Returns whether anything moved, so the caller repaints once on a resize
    /// instead of every frame.
    pub fn record_overlay_frame(&mut self, container: Rect) -> bool {
        let canvas_size = self.canvas_size();
        let canvas = view_frame(container, self.preview_view, canvas_size);
        let layer = self
            .overlay_quad()
            .map(|(center, size)| layer_rect(canvas, canvas_size, center, size));
        let next = FrameBox {
            container: Some(container),
            canvas: Some(canvas),
            layer,
        };
        let moved = next != self.overlay_frame;
        self.overlay_frame = next;
        moved
    }

    /// The selected clip's quad as the preview draws it right now: the resolved
    /// quad, moved and scaled by whatever a transform drag has drafted and not
    /// yet committed.
    ///
    /// The resolver answers for the project, and a drag deliberately leaves the
    /// project alone until it ends, so without this the handles would sit at the
    /// old size and position until the mouse came up. The delta is taken against
    /// the clip's committed transform, so with no drag in flight this is the
    /// resolved quad unchanged rather than a second code path.
    ///
    /// Position is a fraction of the canvas (the unit [`drag_position`] works
    /// in), so it converts to the quad's canvas pixels by the canvas size.
    fn overlay_quad(&self) -> Option<([f32; 2], [f32; 2])> {
        let (center, size) = self.selected_layer_quad()?;
        let committed = self.selected_pose()?;
        let shown = self.overlay_transform()?;
        let canvas = self.canvas_size();
        // A committed scale of zero draws nothing to compare against, so the
        // drafted quad keeps the resolved size instead of dividing by it.
        let ratio = |drafted: f32, base: f32| {
            if base.abs() <= f32::EPSILON {
                1.0
            } else {
                drafted / base
            }
        };
        // A child's position is held in its parent's frame, so the drafted
        // move is placed through that frame before it is read on the canvas.
        let [dx, dy] = on_canvas(
            self.selected_parent_frame(),
            [committed.position_x, committed.position_y],
            [shown.position_x, shown.position_y],
        );
        Some((
            [
                center[0] + dx * canvas[0] as f32,
                center[1] + dy * canvas[1] as f32,
            ],
            [
                size[0] * ratio(shown.scale_x, committed.scale_x),
                size[1] * ratio(shown.scale_y, committed.scale_y),
            ],
        ))
    }

    pub fn overlay_dragging(&self) -> bool {
        self.overlay_drag.is_some() || self.mask_path_holding()
    }

    pub fn overlay_crop(&self) -> Option<CropRect> {
        match self.overlay_drag {
            Some(OverlayDrag::Crop { draft, .. }) => Some(draft),
            _ => self.selected_ui_clip().map(|clip| clip.crop),
        }
    }

    pub fn overlay_mask(&self) -> Option<([f32; 2], [f32; 2], f32)> {
        match self.overlay_drag {
            Some(OverlayDrag::Mask {
                center,
                size,
                rotation,
                ..
            }) => Some((center, size, rotation)),
            _ => self.active_mask_pose(),
        }
    }

    pub fn begin_crop_drag(&mut self, handle: CropHandle) {
        let Some(ui) = self.selected_ui_clip() else {
            return;
        };
        let Some(clip) = self.selected_clip() else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::Crop {
            handle,
            draft: ui.crop,
        });
        let (flip_h, flip_v) = (ui.flip_h, ui.flip_v);
        self.engine
            .lock()
            .set_crop_override(Some((clip, CropRect::FULL, flip_h, flip_v)));
    }

    pub fn begin_mask_drag(&mut self, handle: MaskHandle) {
        // The mask at the playhead, not the projected row: the row holds the
        // clip's first frame, and a tracked mask drawn elsewhere would jump
        // there on the first move.
        let Some((center, size, rotation)) = self.active_mask_pose() else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::Mask {
            handle,
            center,
            size,
            rotation,
        });
    }

    pub fn begin_transform_drag(&mut self, handle: Option<TransformHandle>, at: [f32; 2]) {
        // The pose at the playhead, not the projected row: the row holds the
        // clip's first frame, and a keyframed clip drawn elsewhere would jump
        // there on the first move.
        let Some(pose) = self.selected_pose() else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::Transform {
            handle,
            start: pose,
            grab: at,
            draft: pose,
        });
        // The picture is drafted from the first move, not the first commit: see
        // `preview_transform_draft`.
        self.preview_transform_draft(pose);
    }

    /// Show `draft` in the preview without editing the project.
    ///
    /// The overlay rectangle follows the pointer because it is drawn from the
    /// draft, but the picture under it used to come from the committed project,
    /// so a scale drag read as an outline sliding over a stale frame until the
    /// mouse came up. `Engine::set_transform_override` exists for exactly this
    /// and had no caller; the drag is now its caller, so the framed picture
    /// follows the gesture and the project still sees one edit at the end.
    fn preview_transform_draft(&mut self, draft: crate::types::UiClipTransform) {
        let Some(clip) = self.selected_clip() else {
            return;
        };
        self.engine
            .lock()
            .set_transform_override(Some((clip, draft.into())));
    }

    pub fn overlay_transform(&self) -> Option<crate::types::UiClipTransform> {
        match self.overlay_drag {
            Some(OverlayDrag::Transform { draft, .. }) => Some(draft),
            // A motion path drag moves the picture through the curve it drafts.
            Some(OverlayDrag::PathKey { .. } | OverlayDrag::PathTangent { .. }) => self
                .path_drafted_transform()
                .or_else(|| self.selected_pose()),
            _ => self.selected_pose(),
        }
    }

    pub fn overlay_mode(&self) -> crate::types::OverlayMode {
        self.overlay_mode
    }

    pub fn set_overlay_mode(&mut self, mode: crate::types::OverlayMode) {
        self.overlay_mode = mode;
    }

    pub fn cycle_overlay_mode(&mut self) {
        self.overlay_mode = self.overlay_mode.next();
        self.status_message = Some(format!("{} handles", self.overlay_mode.label()));
    }

    pub fn drag_overlay_to(&mut self, point: [f32; 2]) {
        // A mask path holds its own draft, in path space.
        if self.carry_mask_path(point) {
            return;
        }
        // A motion path is drawn on the canvas as well as on the layer, so its
        // drag needs no quad.
        if self.drag_path_to(point) {
            return;
        }
        let Some(layer) = self.overlay_frame.layer else {
            return;
        };
        let to = layer.fraction_of(point);
        match self.overlay_drag {
            Some(OverlayDrag::Crop { handle, draft }) => {
                self.overlay_drag = Some(OverlayDrag::Crop {
                    handle,
                    draft: drag_crop(draft, handle, to),
                });
            }
            Some(OverlayDrag::Mask {
                handle,
                center,
                size,
                rotation,
            }) => {
                let (center, size, rotation) = match handle {
                    MaskHandle::Center => (drag_mask_center(to), size, rotation),
                    MaskHandle::Size => (center, drag_mask_size(center, to), rotation),
                    MaskHandle::Rotate => (center, size, drag_mask_rotation(center, to, layer)),
                };
                self.overlay_drag = Some(OverlayDrag::Mask {
                    handle,
                    center,
                    size,
                    rotation,
                });
            }
            Some(OverlayDrag::Transform {
                handle,
                start,
                grab,
                ..
            }) => {
                let Some(canvas) = self.overlay_frame.canvas else {
                    return;
                };
                let mut draft = start;
                match handle {
                    Some(_) => {
                        let [sx, sy] =
                            drag_scale([start.scale_x, start.scale_y], layer, grab, point);
                        draft.scale_x = sx;
                        draft.scale_y = sy;
                    }
                    None => {
                        // The pointer moves the clip on the canvas; a child's
                        // position is written back into its parent's frame.
                        let frame = self.selected_parent_frame();
                        let own = [start.position_x, start.position_y];
                        let moved = drag_position(placed(frame, own), grab, point, canvas);
                        [draft.position_x, draft.position_y] = unplaced(frame, moved, own);
                    }
                }
                self.overlay_drag = Some(OverlayDrag::Transform {
                    handle,
                    start,
                    grab,
                    draft,
                });
                if handle.is_none() {
                    self.snap_overlay_move(canvas);
                }
                if let Some(OverlayDrag::Transform { draft, .. }) = self.overlay_drag {
                    self.preview_transform_draft(draft);
                }
            }
            Some(OverlayDrag::TypeHandle {
                index,
                handle,
                start,
                ..
            }) => self.draft_type_handle(index, handle, start, handle.value_at(to)),
            Some(OverlayDrag::PathKey { .. } | OverlayDrag::PathTangent { .. }) | None => {}
        }
    }

    /// Move the drafted position so the drafted quad's edges or center land on
    /// a guide within [`crate::app::viewer_guides::SNAP_PX`]. The quad is derived
    /// from the draft, not read back from the last paint, so the snap holds on
    /// the frame the pointer moves rather than one frame later.
    fn snap_overlay_move(&mut self, canvas: Rect) {
        if canvas.w <= 0.0 || canvas.h <= 0.0 {
            return;
        }
        let size = self.canvas_size();
        let Some(layer) = self
            .overlay_quad()
            .map(|(center, quad)| layer_rect(canvas, size, center, quad))
        else {
            return;
        };
        let [dx, dy] = self.guide_snap_shift(layer);
        let frame = self.selected_parent_frame();
        if let Some(OverlayDrag::Transform {
            handle: None,
            draft,
            ..
        }) = self.overlay_drag.as_mut()
        {
            [draft.position_x, draft.position_y] = moved_on_canvas(
                frame,
                [draft.position_x, draft.position_y],
                [dx / canvas.w, dy / canvas.h],
            );
        }
    }

    pub fn end_overlay_drag(&mut self) -> Result<(), EngineError> {
        if self
            .tool_session
            .mask_path
            .as_ref()
            .is_some_and(|edit| edit.hold.is_some())
        {
            return self.release_mask_path();
        }
        let drag = self.overlay_drag.take();
        self.clear_overlay_previews();
        match drag {
            Some(OverlayDrag::Crop { draft, .. }) => self.set_selected_crop(draft),
            Some(OverlayDrag::Mask {
                center,
                size,
                rotation,
                ..
            }) => self.set_selected_mask_geometry(center, size, rotation),
            Some(OverlayDrag::Transform { start, draft, .. }) => {
                // A press with no movement is a selection click, not an edit: a
                // transform command here would put a no-op on the undo stack.
                if draft == start {
                    return Ok(());
                }
                self.set_selected_transform(draft)
            }
            Some(OverlayDrag::TypeHandle {
                handle,
                start,
                draft,
                ..
            }) => {
                if draft == start {
                    return Ok(());
                }
                if let Some([_, _, z]) = self.selected_effect_point3(handle.effect, handle.param) {
                    self.set_selected_effect_point3(
                        handle.effect,
                        handle.param,
                        [draft[0], draft[1], z],
                    )
                } else {
                    self.set_selected_effect_vec2(handle.effect, handle.param, draft)
                }
            }
            Some(drag @ (OverlayDrag::PathKey { .. } | OverlayDrag::PathTangent { .. })) => {
                self.commit_path_drag(drag)
            }
            None => Ok(()),
        }
    }

    pub fn cancel_overlay_drag(&mut self) {
        self.overlay_drag = None;
        self.cancel_mask_path();
        self.clear_overlay_previews();
    }

    /// Drop every live preview a drag installed.
    ///
    /// One owner and one lock, because `end_overlay_drag` returns early on a
    /// press that never moved: an override left behind there would freeze the
    /// preview on a draft the project never received.
    fn clear_overlay_previews(&mut self) {
        let mut engine = self.engine.lock();
        engine.set_crop_override(None);
        engine.set_transform_override(None);
        engine.set_effect_vec2_override(None);
    }
}

/// Where a clip at its own position `own` sits on the canvas: through its
/// parents' frame when one covers the playhead, else `own` itself. Both in
/// canvas fractions from the canvas center.
fn placed(frame: Option<aria_models::ParentFrame>, own: [f32; 2]) -> [f32; 2] {
    frame.map_or(own, |frame| frame.place(own))
}

/// The own position that places a clip at canvas position `world`. A flattened
/// parent places every child on one point, so no position moves it and the
/// clip keeps `fallback`.
fn unplaced(
    frame: Option<aria_models::ParentFrame>,
    world: [f32; 2],
    fallback: [f32; 2],
) -> [f32; 2] {
    match frame {
        Some(frame) => frame.unplace(world).unwrap_or(fallback),
        None => world,
    }
}

/// The own position that moves a clip at own position `own` by `by` on the
/// canvas, through its parents' frame when one covers the playhead. Canvas
/// fractions from the canvas center. A flattened parent moves nothing, so the
/// clip keeps `own`.
pub(crate) fn moved_on_canvas(
    frame: Option<aria_models::ParentFrame>,
    own: [f32; 2],
    by: [f32; 2],
) -> [f32; 2] {
    let [x, y] = placed(frame, own);
    unplaced(frame, [x + by[0], y + by[1]], own)
}

/// How far the canvas position moves when a clip's own position goes from
/// `from` to `to`.
fn on_canvas(frame: Option<aria_models::ParentFrame>, from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let (a, b) = (placed(frame, from), placed(frame, to));
    [b[0] - a[0], b[1] - a[1]]
}

// The tests live in this module rather than in a `_tests.rs` child because the
// ledger row that asked for the live picture names
// `canvas_overlay_handles::picture_follows_the_drag`, and a check must select
// the test it names.
#[cfg(test)]
use crate::app::transform_handle_tests::{framed, selected};

/// The centre, in canvas pixels, of the layer `clip` draws in `scene`.
#[cfg(test)]
fn layer_center(scene: &aria_render::Scene, clip: aria_models::ClipId) -> [f32; 2] {
    scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(clip))
        .expect("the dragged clip draws a layer")
        .center
}

/// Mid-drag, the frame the engine would render carries the draft.
///
/// The class this closes: the overlay rectangle was drawn from the draft while
/// the picture came from the committed project, so a drag read as an outline
/// sliding over a stale frame. Asserted against the resolved scene rather than
/// a rendered image, because a Linux gate has no window to read pixels back
/// from; the scene is what the compositor is handed.
#[cfg(test)]
#[test]
fn picture_follows_the_drag() {
    let mut state = selected();
    framed(&mut state);
    let clip = state.selected_clip().expect("a selected clip");
    let at = state.playhead;
    let engine = std::sync::Arc::clone(state.engine());
    let revision = engine.lock().revision();
    let committed = layer_center(&engine.lock().preview_scene(at).expect("scene"), clip);

    state.begin_transform_drag(None, [200.0, 100.0]);
    state.drag_overlay_to([300.0, 100.0]);
    let draft = state.overlay_transform().expect("a draft mid-drag");

    let drawn = layer_center(&engine.lock().preview_scene(at).expect("scene"), clip);
    assert_ne!(
        drawn, committed,
        "the previewed picture is still where the committed project puts it"
    );
    let wanted = aria_render::resolve_with(
        engine.lock().project(),
        at,
        aria_render::ResolveOverrides {
            transform: Some((clip, draft.into())),
            ..aria_render::ResolveOverrides::default()
        },
    )
    .expect("scene");
    assert_eq!(
        drawn,
        layer_center(&wanted, clip),
        "the previewed picture is not the draft the outline is drawn from"
    );
    assert_eq!(
        engine.lock().revision(),
        revision,
        "a drag in flight edited the project"
    );
}

/// Every way a drag ends drops the preview: release, no-op release, cancel.
///
/// The no-op case is the one that bites — `end_overlay_drag` returns early on a
/// press that never moved, so a clear placed after that return would leave the
/// preview frozen on a draft the project never received.
#[cfg(test)]
#[test]
fn no_ending_leaves_a_preview_behind() {
    for (label, moved, cancel) in [
        ("a committed drag", true, false),
        ("a press that never moved", false, false),
        ("a cancelled drag", true, true),
    ] {
        let mut state = selected();
        framed(&mut state);
        state.begin_transform_drag(None, [200.0, 100.0]);
        assert!(
            state.engine().lock().has_live_overrides(),
            "{label}: the drag never installed a preview"
        );
        if moved {
            state.drag_overlay_to([300.0, 100.0]);
        }
        if cancel {
            state.cancel_overlay_drag();
        } else {
            state.end_overlay_drag().expect("end the drag");
        }
        assert!(
            !state.engine().lock().has_live_overrides(),
            "{label}: left a preview override behind"
        );
    }
}

/// A moved layer lands on a guide within reach, only with snapping on, and
/// the committed position is the snapped one.
///
/// The class this closes: guides that are drawn and snap in a unit test of
/// [`crate::app::viewer_guides::snap_shift`] but never reach the drag, so a layer
/// slides past every guide. Driven through `drag_overlay_to` and
/// `end_overlay_drag`, the path the pointer takes. Not covered: snapping a
/// corner (scale) drag, which is not snapped.
#[cfg(test)]
#[test]
fn a_moved_layer_snaps_to_a_guide_in_reach() {
    use crate::app::viewer_guides::{Guide, GuideAxis, SNAP_PX};
    let quad = |state: &AppState| {
        let canvas = state.overlay_frame.canvas.expect("a drawn canvas");
        let size = state.canvas_size();
        state
            .overlay_quad()
            .map(|(center, quad)| layer_rect(canvas, size, center, quad))
            .expect("the selected clip draws a quad")
    };
    let drag_by = |snap: bool, dx: f32| {
        let mut state = selected();
        framed(&mut state);
        let canvas = state.overlay_frame.canvas.expect("a drawn canvas");
        let size = state.canvas_size();
        let start = quad(&state);
        assert!(
            // The center and right edge must stay out of reach of the guide
            // in every case below, so only the left edge can snap.
            start.w > 10.0 * SNAP_PX,
            "the layer is too narrow to tell its edges apart"
        );
        // A vertical guide 4 screen px right of where a 30 px move leaves the
        // layer's left edge.
        let line = start.x + 34.0;
        state.viewer_guides.guides.push(Guide {
            axis: GuideAxis::Vertical,
            at: (line - canvas.x) * size[0] as f32 / canvas.w,
        });
        state.viewer_guides.snap = snap;
        state.begin_transform_drag(None, [200.0, 100.0]);
        state.drag_overlay_to([200.0 + dx, 100.0]);
        let drafted = quad(&state);
        state.end_overlay_drag().expect("commit the move");
        (start, line, drafted, quad(&state))
    };
    let close = |a: f32, b: f32| (a - b).abs() < 1e-2;

    let (start, line, drafted, committed) = drag_by(true, 30.0);
    assert!(
        close(drafted.x, line),
        "mid-drag left edge {} is not on the guide at {line}",
        drafted.x
    );
    assert!(
        close(committed.x, line),
        "committed left edge {} is not on the guide at {line}",
        committed.x
    );
    assert!(
        close(committed.y, start.y),
        "a vertical guide moved the layer vertically"
    );

    let (start, _, drafted, committed) = drag_by(false, 30.0);
    assert!(
        close(drafted.x, start.x + 30.0),
        "snap off still moved the layer to {}",
        drafted.x
    );
    assert!(close(committed.x, start.x + 30.0));

    // 14 px short of the guide: out of reach, so the layer follows the pointer.
    let (start, _, drafted, _) = drag_by(true, 20.0);
    assert!(
        close(drafted.x, start.x + 20.0),
        "a guide out of reach pulled the layer to {}",
        drafted.x
    );
}
