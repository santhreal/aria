//! Mask, chroma-key, and track-matte commands for the selected clip.
//!
//! `SetClipMask`, `SetClipChroma`, and `SetClipMatte` each carry the whole
//! value, so every setter here rebuilds the untouched fields from the
//! projection. All three are `Option`, so clearing is the same command with
//! `None`.
//!
//! The numeric knobs are *not* here. Rebuilding a block to change one knob
//! flattens every keyframe in it, which is exactly what this module's knob
//! tables used to do; a scalar edit goes through `clip_params.rs`, which
//! addresses the one property. What is left is the structural part — which
//! shape, which key colour, which stencil — where carrying the whole value is
//! the command's own shape.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ChromaKey, Mask, MaskKind, MatteSource, Param, TrackMatte};

use super::AppState;
use crate::types::{UiChroma, UiMask, UiMatte};

/// Rebuild the full model mask from the projection, so one knob edit keeps the
/// rest.
fn mask_from_ui(ui: &UiMask) -> Mask {
    Mask {
        kind: ui.kind,
        feather: Param::Constant(ui.feather),
        center: Param::Constant(ui.center),
        size: Param::Constant(ui.size),
        rotation: Param::Constant(ui.rotation),
        roundness: Param::Constant(ui.roundness),
        invert: ui.invert,
    }
}

fn chroma_from_ui(ui: &UiChroma) -> ChromaKey {
    ChromaKey {
        rgb: ui.rgb,
        strength: Param::Constant(ui.strength),
        shadow: Param::Constant(ui.shadow),
    }
}

fn matte_from_ui(ui: &UiMatte) -> TrackMatte {
    TrackMatte {
        source: ui.source,
        invert: ui.invert,
        softness: Param::Constant(ui.softness),
    }
}

impl AppState {
    fn send_mask(&mut self, mask: Option<Mask>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMask { clip, mask }))?;
        Ok(())
    }

    /// Apply a mask shape. Keeps the current knobs when only the shape changes.
    pub fn set_selected_mask_kind(&mut self, kind: MaskKind) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let mask = match &ui.mask {
            Some(existing) => {
                let mut mask = mask_from_ui(existing);
                mask.kind = kind;
                mask
            }
            None => Mask::new(kind),
        };
        self.send_mask(Some(mask))
    }

    pub fn clear_selected_mask(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_mask(None)?;
        self.status_message = Some("Cleared mask".into());
        Ok(())
    }

    /// Keep the outside of the mask instead of the inside.
    pub fn toggle_selected_mask_invert(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(existing) = ui.mask.as_ref() else {
            return Ok(());
        };
        let mut mask = mask_from_ui(existing);
        mask.invert = !existing.invert;
        self.send_mask(Some(mask))
    }

    /// Set the mask center, size, and rotation at once, keeping the shape and the
    /// remaining knobs. The on-canvas handles build all three, so a drag is one
    /// edit rather than one per pointer move.
    pub fn set_selected_mask_geometry(
        &mut self,
        center: [f32; 2],
        size: [f32; 2],
        rotation: f32,
    ) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(existing) = ui.mask.as_ref() else {
            return Ok(());
        };
        if !center.iter().chain(size.iter()).all(|v| v.is_finite()) || !rotation.is_finite() {
            return Ok(());
        }
        if existing.center == center && existing.size == size && existing.rotation == rotation {
            return Ok(());
        }
        let mut mask = mask_from_ui(existing);
        mask.center = Param::Constant(center);
        mask.size = Param::Constant([size[0].clamp(0.01, 10.0), size[1].clamp(0.01, 10.0)]);
        mask.rotation = Param::Constant(rotation.clamp(-3600.0, 3600.0));
        self.send_mask(Some(mask))
    }

    fn send_chroma(&mut self, chroma: Option<ChromaKey>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipChroma { clip, chroma }))?;
        Ok(())
    }

    /// Key on a colour, seeding a usable tolerance so the first click does
    /// something visible.
    pub fn set_selected_chroma_color(&mut self, rgb: [u8; 3]) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let chroma = match &ui.chroma {
            Some(existing) => {
                let mut chroma = chroma_from_ui(existing);
                chroma.rgb = rgb;
                chroma
            }
            None => ChromaKey {
                rgb,
                strength: Param::Constant(0.5),
                shadow: Param::Constant(0.0),
            },
        };
        self.send_chroma(Some(chroma))
    }

    pub fn clear_selected_chroma(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_chroma(None)?;
        self.status_message = Some("Cleared chroma key".into());
        Ok(())
    }

    fn send_matte(&mut self, matte: Option<TrackMatte>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipMatte { clip, matte }))?;
        Ok(())
    }

    /// Whether a clip on a higher lane overlaps the selected clip.
    ///
    /// The stencil is not chosen in the inspector: the renderer reads whatever
    /// layer sits directly above, and a matted clip with nothing above it draws
    /// unmatted. So this is what makes the difference between a matte that cuts
    /// and one that is inert, and the setters say which the operator just got.
    pub fn matte_stencil_ready(&self) -> bool {
        let Some(ui) = self.selected_ui_clip() else {
            return false;
        };
        let tracks = &self.store.sequence.tracks;
        let Some(lane) = tracks.iter().position(|t| t.id == ui.track_id) else {
            return false;
        };
        // Tracks are held top-first, so every lane above this one has a
        // smaller index.
        tracks[..lane].iter().flat_map(|t| t.clips.iter()).any(|c| {
            c.timeline_start_ticks < ui.timeline_end_ticks
                && c.timeline_end_ticks > ui.timeline_start_ticks
        })
    }

    /// Cut the selected clip with the lane above, reading `source`.
    ///
    /// Seeds a soft edge of zero: a hard cut is what a matte means before the
    /// operator asks for a falloff.
    pub fn set_selected_matte_source(&mut self, source: MatteSource) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let matte = match ui.matte.as_ref() {
            Some(existing) => TrackMatte {
                source,
                ..matte_from_ui(existing)
            },
            None => TrackMatte {
                source,
                invert: false,
                softness: Param::Constant(0.0),
            },
        };
        let ready = self.matte_stencil_ready();
        self.send_matte(Some(matte))?;
        self.status_message = Some(if ready {
            format!("Track matte reads {} above", source.label()).into()
        } else {
            format!(
                "Track matte set to {}; nothing above this clip to cut with",
                source.label()
            )
            .into()
        });
        Ok(())
    }

    pub fn clear_selected_matte(&mut self) -> Result<(), EngineError> {
        if self.selected_clip().is_none() {
            return Ok(());
        }
        self.send_matte(None)?;
        self.status_message = Some("Removed track matte".into());
        Ok(())
    }

    /// Flip which side of the stencil keeps the content.
    pub fn toggle_selected_matte_invert(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(existing) = ui.matte.as_ref() else {
            return Ok(());
        };
        let mut matte = matte_from_ui(existing);
        matte.invert = !matte.invert;
        self.send_matte(Some(matte))
    }
}
