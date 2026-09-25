use aria_models::{
    ClipId, ClipSource, MAX_SPEED, MIN_SPEED, ModelError, Rational, RationalTime, TrackId,
    check_same_rate, resample, retimed_duration,
};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Deep-copy one clip to an explicit destination retimed to a target duration or speed.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
    target_duration: Option<RationalTime>,
    target_speed: Option<Rational>,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    check_same_rate(start.rate, tl_rate)?;
    if start.value < 0 {
        return Err(EngineError::from(ModelError::InvalidRange));
    }

    let source_clip = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;

    let ClipSource::Media { media: _, source } = &source_clip.content else {
        return Err(EngineError::from(ModelError::InvalidParam(
            "speed requires a media-backed clip".to_string(),
        )));
    };

    let src_dur_tl = resample(source.duration, tl_rate).value.max(1);

    let (derived_speed, duration_ticks) = if let Some(target_dur) = target_duration {
        check_same_rate(target_dur.rate, tl_rate)?;
        if target_dur.value <= 0 {
            return Err(EngineError::Refused(
                "target duration must be positive".to_string(),
            ));
        }
        let fill = target_dur.value;
        let speed = ratio(src_dur_tl, fill);
        let speed_f32 = speed.num as f32 / speed.den as f32;
        if speed_f32 < MIN_SPEED || speed_f32 > MAX_SPEED {
            return Err(EngineError::Refused(format!(
                "speed {speed_f32} outside documented min {MIN_SPEED} and max {MAX_SPEED}"
            )));
        }
        let dur = retimed_duration(
            src_dur_tl,
            speed,
            source_clip.speed_curve_average(),
            source_clip.has_speed_curve(),
        );
        (speed, dur)
    } else if let Some(speed) = target_speed {
        if speed.num <= 0 || speed.den <= 0 {
            return Err(EngineError::Refused(
                "target speed must be positive".to_string(),
            ));
        }
        let speed_f32 = speed.num as f32 / speed.den as f32;
        if speed_f32 < MIN_SPEED || speed_f32 > MAX_SPEED {
            return Err(EngineError::Refused(format!(
                "speed {speed_f32} outside documented min {MIN_SPEED} and max {MAX_SPEED}"
            )));
        }
        let dur = retimed_duration(
            src_dur_tl,
            speed,
            source_clip.speed_curve_average(),
            source_clip.has_speed_curve(),
        );
        (speed, dur)
    } else {
        return Err(EngineError::Refused(
            "paste retimed requires a target duration or target speed".to_string(),
        ));
    };

    let new_id =
        ctx.project
            .duplicate_clip_retimed(clip, to_track, start, derived_speed, duration_ticks)?;
    Ok((new_id, Box::new(RemoveClipAction { clip: new_id })))
}

fn ratio(num: i64, den: i64) -> Rational {
    let divisor = gcd(num, den).max(1);
    let (mut num, mut den) = (num / divisor, den / divisor);
    while num > i64::from(i32::MAX) || den > i64::from(i32::MAX) {
        num = (num / 2).max(1);
        den = (den / 2).max(1);
    }
    Rational::new(num as i32, den as i32)
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
