//! Framing commands for the selected clip: the crop window, mirroring, and
//! motion blur.
//!
//! `SetClipCrop` carries the whole rect plus both mirror flags, so an edit to
//! one edge must resend the rest or it silently un-crops the other three and
//! drops the mirroring. Every setter clamps to the model's valid-rect set
//! (`w`/`h` at least [`MIN_CROP_FRACTION`], edges inside the frame) instead of
//! letting the engine reject the drag.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{CropRect, MIN_CROP_FRACTION, MotionBlur};

use super::AppState;

/// The four crop edges, as (id, label, reader, writer).
///
/// Data rather than four hand-written rows: the inspector generates one stepper
/// per entry, so the panel cannot drift from the rect.
pub const CROP_EDGES: &[(&str, &str, fn(&CropRect) -> f32, fn(&mut CropRect, f32))] = &[
    (
        "crop-left",
        "Left",
        |c| c.x,
        |c, v| {
            let right = c.x + c.w;
            c.x = v;
            c.w = right - v;
        },
    ),
    (
        "crop-top",
        "Top",
        |c| c.y,
        |c, v| {
            let bottom = c.y + c.h;
            c.y = v;
            c.h = bottom - v;
        },
    ),
    (
        "crop-right",
        "Right",
        |c| 1.0 - (c.x + c.w),
        |c, v| {
            c.w = 1.0 - v - c.x;
        },
    ),
    (
        "crop-bottom",
        "Bottom",
        |c| 1.0 - (c.y + c.h),
        |c, v| {
            c.h = 1.0 - v - c.y;
        },
    ),
];

/// Read one crop inset by id. Left and top are the kept region's origin; right
/// and bottom are measured from the far edge, which is how a crop control reads.
pub fn crop_inset(crop: &CropRect, id: &str) -> f32 {
    CROP_EDGES
        .iter()
        .find(|(edge, ..)| *edge == id)
        .map(|(_, _, get, _)| get(crop))
        .unwrap_or(0.0)
}

/// Keeps the origin a hair inside the far edge: the model rejects
/// `x + w > 1.0`, and clamping to exactly `1.0 - w` can land an ulp past it.
const EDGE_GUARD: f32 = 1e-4;

/// Clamp a rect into the model's valid set so a drag stops at the limit rather
/// than being refused.
///
/// Extent first, then origin: the extent has a hard floor
/// ([`MIN_CROP_FRACTION`]), so the origin is what has to give way at the edge.
fn clamp_crop(mut crop: CropRect) -> CropRect {
    if !crop.x.is_finite() || !crop.y.is_finite() || !crop.w.is_finite() || !crop.h.is_finite() {
        return CropRect::FULL;
    }
    crop.w = crop.w.clamp(MIN_CROP_FRACTION, 1.0);
    crop.h = crop.h.clamp(MIN_CROP_FRACTION, 1.0);
    crop.x = crop.x.clamp(0.0, (1.0 - crop.w - EDGE_GUARD).max(0.0));
    crop.y = crop.y.clamp(0.0, (1.0 - crop.h - EDGE_GUARD).max(0.0));
    crop
}

impl AppState {
    /// Whether the selection has a framing block at all (visual clips only).
    pub fn selected_has_framing(&self) -> bool {
        self.selected_ui_clip()
            .is_some_and(|clip| !matches!(clip.kind, crate::types::UiClipKind::Audio))
    }

    fn send_crop(&mut self, crop: CropRect, flip_h: bool, flip_v: bool) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipCrop {
            clip,
            crop: clamp_crop(crop),
            flip_h,
            flip_v,
            at: None,
        }))?;
        Ok(())
    }

    /// Set one crop inset, keeping the other three and both mirror flags.
    pub fn set_selected_crop_inset(&mut self, id: &str, value: f32) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some((_, _, _, set)) = CROP_EDGES.iter().find(|(edge, ..)| *edge == id) else {
            return Ok(());
        };
        let mut crop = ui.crop;
        set(&mut crop, value.clamp(0.0, 1.0 - MIN_CROP_FRACTION));
        self.send_crop(crop, ui.flip_h, ui.flip_v)
    }

    /// Move one crop inset by `step` from its current value.
    pub fn nudge_selected_crop_inset(&mut self, id: &str, step: f32) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        self.set_selected_crop_inset(id, crop_inset(&ui.crop, id) + step)
    }

    /// Mirror the content left-right, keeping the crop.
    pub fn toggle_selected_flip_h(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        self.send_crop(ui.crop, !ui.flip_h, ui.flip_v)
    }

    /// Mirror the content top-bottom, keeping the crop.
    pub fn toggle_selected_flip_v(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        self.send_crop(ui.crop, ui.flip_h, !ui.flip_v)
    }

    /// Restore the full frame and clear both mirrors.
    pub fn clear_selected_crop(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_crop(CropRect::FULL, false, false)?;
        self.status_message = Some("Reset framing".into());
        Ok(())
    }

    /// Set the whole crop window at once, keeping both mirror flags. The on-canvas
    /// drag builds a rect and commits it here, so a gesture is one edit rather than
    /// one per pointer move.
    pub fn set_selected_crop(&mut self, crop: CropRect) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        if clamp_crop(crop) == ui.crop {
            return Ok(());
        }
        self.send_crop(crop, ui.flip_h, ui.flip_v)
    }

    fn send_motion_blur(&mut self, motion_blur: MotionBlur) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMotionBlur {
            clip,
            motion_blur,
        }))?;
        Ok(())
    }

    pub fn toggle_selected_motion_blur(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let mut blur = ui.motion_blur;
        blur.enabled = !blur.enabled;
        self.send_motion_blur(blur)
    }

    /// Move the shutter angle, clamped to the model's `[0, 720]`.
    pub fn nudge_selected_shutter(&mut self, step: f32) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let mut blur = ui.motion_blur;
        blur.shutter_deg = (blur.shutter_deg + step).clamp(0.0, 720.0);
        self.send_motion_blur(blur)
    }

    /// Move the sub-frame sample count, clamped to the model's `[2, 32]`.
    pub fn nudge_selected_blur_samples(&mut self, step: i32) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let mut blur = ui.motion_blur;
        blur.samples = (blur.samples as i32 + step).clamp(2, 32) as u32;
        self.send_motion_blur(blur)
    }
}
