use aria_models::{ClipId, ModelError, Param, RationalTime};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Restore only the audio mix properties (volume, fade_in, fade_out) of a clip.
pub struct RestoreAudioAction {
    pub clip: ClipId,
    pub volume: Param<f32>,
    pub fade_in: i64,
    pub fade_out: i64,
}

impl EditAction for RestoreAudioAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let clip_id = self.clip;
        let clip = ctx
            .project
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let current_volume = clip.volume.clone();
        let current_fade_in = clip.fade_in;
        let current_fade_out = clip.fade_out;

        clip.volume = self.volume;
        clip.fade_in = self.fade_in;
        clip.fade_out = self.fade_out;

        Ok(Box::new(RestoreAudioAction {
            clip: clip_id,
            volume: current_volume,
            fade_in: current_fade_in,
            fade_out: current_fade_out,
        }))
    }
}

/// Set a media clip's audio mix (CapCut volume + fades). `volume` is `Some`
/// for the basic flat-level slider (overwrites the gain, flattening an M8
/// envelope) or `None` to keep the gain and change only the fades. The model
/// validates the gain range, media backing, and fade durations.
pub fn set_audio(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    volume: Option<f32>,
    fade_in: RationalTime,
    fade_out: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let clip_obj = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let prev_volume = clip_obj.volume.clone();
    let prev_fade_in = clip_obj.fade_in;
    let prev_fade_out = clip_obj.fade_out;

    ctx.project
        .set_clip_audio(clip, volume, fade_in, fade_out)?;
    Ok(Box::new(RestoreAudioAction {
        clip,
        volume: prev_volume,
        fade_in: prev_fade_in,
        fade_out: prev_fade_out,
    }))
}
