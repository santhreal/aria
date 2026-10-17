use aria_models::{
    ClipId, MediaId, ModelError, RationalTime, SoundPlacement, SourceChannel, TimeRange, TrackId,
    resample,
};

use crate::action::edit::{extract_audio, fit_to_fill, set_audio};
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// A three- or four-point edit from a Source tab or a media drop, placed
/// through source patching.
///
/// The span `[at, at + fill)` is opened once on every patched lane, by a
/// ripple that also carries the other sync-locked lanes or by a cover on each
/// lane, before anything is placed. The picture then lands on `track`, retimed
/// to the span the way [`fit_to_fill`] retimes one. A sound sent to a lane is
/// detached onto it the way Extract Audio detaches it, so the two clips are
/// linked and the picture plays silent; a sound kept in the picture plays from
/// it; a sound left out silences the picture. A sound placed with its picture
/// patched off is a plain clip on the audio lane.
///
/// Atomic: one `CompoundAction` inverse, and a refused step reverts the ones
/// already applied before the error propagates.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
    fill: Option<RationalTime>,
    ripple: bool,
    track: Option<TrackId>,
    sound: SoundPlacement,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let has_sound = check(ctx, media, track, sound)?;
    let span = match fill {
        Some(duration) => fit_to_fill::span_ticks(ctx, duration)?,
        None => resample(source.duration, ctx.project.timeline().frame_rate)
            .value
            .max(1),
    };
    let lanes: Vec<TrackId> = track.into_iter().chain(sound.lane()).collect();

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let placed = fit_to_fill::open_span(ctx, &lanes, at, span, ripple, &mut inverses).and_then(
        |()| match (track, sound) {
            (Some(picture_lane), sound) => {
                let picture = fit_to_fill::place_retimed(
                    ctx,
                    picture_lane,
                    media,
                    source,
                    at,
                    span,
                    &mut inverses,
                )?;
                match sound {
                    SoundPlacement::Lane(sound_lane) => {
                        let (_, inverse) = extract_audio::execute(ctx, picture, Some(sound_lane))?;
                        inverses.push(inverse);
                    }
                    SoundPlacement::Off if has_sound => inverses.push(silence(ctx, picture)?),
                    SoundPlacement::Off | SoundPlacement::InPicture => {}
                }
                Ok(picture)
            }
            (None, SoundPlacement::Lane(sound_lane)) => {
                fit_to_fill::place_retimed(ctx, sound_lane, media, source, at, span, &mut inverses)
            }
            (None, SoundPlacement::InPicture | SoundPlacement::Off) => Err(no_lane()),
        },
    );
    match placed {
        Ok(id) => Ok((id, Box::new(CompoundAction { actions: inverses }))),
        Err(err) => {
            fit_to_fill::rollback(ctx, inverses);
            Err(err)
        }
    }
}

/// Refuse a patch the media cannot fill, before anything is applied. Returns
/// whether the media has sound.
fn check(
    ctx: &ApplyContext<'_>,
    media: MediaId,
    track: Option<TrackId>,
    sound: SoundPlacement,
) -> Result<bool, EngineError> {
    if track.is_none() && sound.lane().is_none() {
        return Err(no_lane());
    }
    let source = ctx
        .project
        .media(media)
        .ok_or(ModelError::UnknownMedia(media))?;
    let has_picture = source.has_channel(SourceChannel::Video);
    let has_sound = source.has_channel(SourceChannel::Audio);
    if track.is_some() && !has_picture {
        return Err(EngineError::Refused(format!(
            "media {media} has no picture; patch only its sound"
        )));
    }
    if sound.lane().is_some() && !has_sound {
        return Err(EngineError::Refused(format!(
            "media {media} has no sound; patch only its picture"
        )));
    }
    for (channel, lane) in [
        (SourceChannel::Video, track),
        (SourceChannel::Audio, sound.lane()),
    ] {
        let Some(lane) = lane else { continue };
        let kind = ctx
            .project
            .timeline()
            .track(lane)
            .ok_or(ModelError::UnknownTrack(lane))?
            .kind;
        if kind != channel.lane_kind() {
            return Err(ModelError::IncompatibleTrackKind { track: lane, kind }.into());
        }
    }
    Ok(has_sound)
}

/// Mute a placed picture whose sound was patched off.
fn silence(ctx: &mut ApplyContext<'_>, clip: ClipId) -> Result<Box<dyn EditAction>, EngineError> {
    let placed = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let rate = placed.start().rate;
    let (fade_in, fade_out) = (
        RationalTime::new(placed.fade_in, rate),
        RationalTime::new(placed.fade_out, rate),
    );
    set_audio::set_audio(ctx, clip, Some(0.0), fade_in, fade_out)
}

fn no_lane() -> EngineError {
    EngineError::Refused(
        "a source edit needs a patched lane; patch the picture or the sound to a lane".into(),
    )
}
