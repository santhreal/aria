//! Inspector audio edits: volume, fades, reverse. Pan is a catalog property and
//! is written by the generated row through `set_param_scalar`.
//!
//! Volume is stored as the linear multiplier a mixer applies and read by an
//! operator in decibels, so both conversions come from
//! [`aria_analysis::gain`] rather than being spelled out here: a clip's gain, a
//! normalisation, and an export's headroom have to agree on what −6 dB means.

use aria_analysis::{db_from_linear, linear_from_db};
use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipParam, Rational, RationalTime};

use super::AppState;

/// The quietest fader position that still reads as a level. A step below this
/// is silence, which has no decibel value, and a step up from silence lands
/// here rather than seventy steps under unity.
pub const VOLUME_FLOOR_DB: f32 = -60.0;

impl AppState {
    /// Set the selected clip's gain.
    ///
    /// Written through the parameter path, not through `SetClipAudio`: gain is an
    /// animatable property, and a whole-block write would replace a volume
    /// envelope with the constant the fader happened to be at. It also leaves the
    /// fades alone, which a block write could not.
    pub fn set_selected_volume(&mut self, volume: f32) -> Result<(), EngineError> {
        self.set_param_scalar(ClipParam::Volume, None, volume)
    }

    /// Move the selected clip's gain by `decibels`.
    ///
    /// A fader step is constant in decibels, not in the multiplier: 1 dB down
    /// from unity and 1 dB down from a whisper are the same gesture, while a
    /// fixed linear step is inaudible at the top and a cliff at the bottom.
    /// The bottom of the range is silence, so stepping under
    /// `VOLUME_FLOOR_DB` mutes rather than approaching zero forever.
    pub fn nudge_selected_volume_db(&mut self, decibels: f32) -> Result<(), EngineError> {
        let Some(clip) = self.selected_ui_clip_ref() else {
            return Ok(());
        };
        let next = match db_from_linear(clip.volume) {
            None if decibels > 0.0 => linear_from_db(VOLUME_FLOOR_DB),
            None => 0.0,
            Some(current) => {
                let level = current + decibels;
                if level < VOLUME_FLOOR_DB {
                    0.0
                } else {
                    linear_from_db(level)
                }
            }
        };
        self.set_selected_volume(next)
    }

    pub fn set_selected_fade_in(&mut self, ticks: i64) -> Result<(), EngineError> {
        self.set_selected_fades(Some(ticks), None)
    }

    pub fn set_selected_fade_out(&mut self, ticks: i64) -> Result<(), EngineError> {
        self.set_selected_fades(None, Some(ticks))
    }

    pub fn toggle_selected_reverse(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let den = ui.speed_den.max(1);
        self.apply_command(Command::Edit(EditCommand::SetClipSpeed {
            clip,
            speed: Rational::new(ui.speed_num, den),
            reversed: !ui.reversed,
        }))?;
        Ok(())
    }

    fn set_selected_fades(
        &mut self,
        fade_in: Option<i64>,
        fade_out: Option<i64>,
    ) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let duration = (ui.timeline_end_ticks - ui.timeline_start_ticks).max(0);
        let fade_in = fade_in.unwrap_or(ui.fade_in_ticks).clamp(0, duration);
        let fade_out = fade_out.unwrap_or(ui.fade_out_ticks).clamp(0, duration);
        self.apply_command(Command::Edit(EditCommand::SetClipAudio {
            clip,
            volume: None,
            fade_in: RationalTime::new(fade_in, self.playhead.rate),
            fade_out: RationalTime::new(fade_out, self.playhead.rate),
        }))?;
        Ok(())
    }

    /// Set one clip's fade in, by id rather than by selection: a fade handle is
    /// dragged on a card, which may not be the selected clip.
    pub fn set_clip_fade_in(&mut self, clip: u64, ticks: i64) -> Result<(), EngineError> {
        self.set_clip_fades(clip, Some(ticks), None)
    }

    /// Set one clip's fade out, by id.
    pub fn set_clip_fade_out(&mut self, clip: u64, ticks: i64) -> Result<(), EngineError> {
        self.set_clip_fades(clip, None, Some(ticks))
    }

    /// Clamped so a drag past either end of the card stalls at the whole clip
    /// rather than being refused: the gesture has no way to know the limit.
    fn set_clip_fades(
        &mut self,
        clip: u64,
        fade_in: Option<i64>,
        fade_out: Option<i64>,
    ) -> Result<(), EngineError> {
        let Some(ui) = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|c| c.id == clip)
            .cloned()
        else {
            return Ok(());
        };
        let duration = (ui.timeline_end_ticks - ui.timeline_start_ticks).max(0);
        let fade_in = fade_in.unwrap_or(ui.fade_in_ticks).clamp(0, duration);
        let fade_out = fade_out.unwrap_or(ui.fade_out_ticks).clamp(0, duration);
        if fade_in == ui.fade_in_ticks && fade_out == ui.fade_out_ticks {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetClipAudio {
            clip: aria_models::ClipId::from_raw(clip),
            volume: None,
            fade_in: RationalTime::new(fade_in, self.playhead.rate),
            fade_out: RationalTime::new(fade_out, self.playhead.rate),
        }))?;
        Ok(())
    }
}
