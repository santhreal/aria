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
        let canvas = fit_frame(container, canvas_size);
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

    /// The selected clip's quad as the operator sees it right now: the resolved
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
        let committed = self.selected_ui_clip_ref()?.transform;
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
        Some((
            [
                center[0] + (shown.position_x - committed.position_x) * canvas[0] as f32,
                center[1] + (shown.position_y - committed.position_y) * canvas[1] as f32,
            ],
            [
                size[0] * ratio(shown.scale_x, committed.scale_x),
                size[1] * ratio(shown.scale_y, committed.scale_y),
            ],
        ))
    }

    pub fn overlay_dragging(&self) -> bool {
        self.overlay_drag.is_some()
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
            _ => self
                .selected_ui_clip()
                .and_then(|clip| clip.mask)
                .map(|mask| (mask.center, mask.size, mask.rotation)),
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
        let Some(mask) = self.selected_ui_clip().and_then(|clip| clip.mask) else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::Mask {
            handle,
            center: mask.center,
            size: mask.size,
            rotation: mask.rotation,
        });
    }

    pub fn begin_transform_drag(&mut self, handle: Option<TransformHandle>, at: [f32; 2]) {
        let Some(ui) = self.selected_ui_clip() else {
            return;
        };
        self.overlay_drag = Some(OverlayDrag::Transform {
            handle,
            start: ui.transform,
            grab: at,
            draft: ui.transform,
        });
        // The picture is drafted from the first move, not the first commit: see
        // `preview_transform_draft`.
        self.preview_transform_draft(ui.transform);
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
            _ => self.selected_ui_clip().map(|clip| clip.transform),
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
                        let [px, py] = drag_position(
                            [start.position_x, start.position_y],
                            grab,
                            point,
                            canvas,
                        );
                        draft.position_x = px;
                        draft.position_y = py;
                    }
                }
                self.overlay_drag = Some(OverlayDrag::Transform {
                    handle,
                    start,
                    grab,
                    draft,
                });
                self.preview_transform_draft(draft);
            }
            None => {}
        }
    }

    pub fn end_overlay_drag(&mut self) -> Result<(), EngineError> {
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
            None => Ok(()),
        }
    }

    pub fn cancel_overlay_drag(&mut self) {
        self.overlay_drag = None;
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
    }
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
