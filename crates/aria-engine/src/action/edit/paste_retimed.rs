use aria_models::{
    Clip, ClipId, GeneratedRefusal, MAX_SPEED, MIN_SPEED, ModelError, Rational, RationalTime,
    TrackId, check_same_rate, resample, retimed_duration,
};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Place a clipboard copy on `track` retimed to a target duration or speed.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    copy: Clip,
    track: TrackId,
    start: RationalTime,
    target_duration: Option<RationalTime>,
    target_speed: Option<Rational>,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    check_same_rate(start.rate, tl_rate)?;
    if start.value < 0 {
        return Err(EngineError::from(ModelError::InvalidRange));
    }

    // A generated clip plays no source, so it fills a span by taking its
    // length, and has no speed to set.
    let Some(source) = copy.source_range() else {
        let target_dur = match (target_duration, target_speed) {
            (Some(target_dur), _) => target_dur,
            (None, Some(_)) => {
                return Err(ModelError::Generated(GeneratedRefusal::Speed).into());
            }
            (None, None) => return Err(missing_target()),
        };
        check_same_rate(target_dur.rate, tl_rate)?;
        if target_dur.value <= 0 {
            return Err(EngineError::Refused(
                "target duration must be positive".to_string(),
            ));
        }
        let speed = copy.speed;
        let new_id = ctx
            .project
            .place_copy(copy, track, start, Some((speed, target_dur.value)))?;
        return Ok((new_id, Box::new(RemoveClipAction { clip: new_id })));
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
        let speed = Rational::from_ratio_fitted(src_dur_tl, fill);
        let speed_f32 = speed.num as f32 / speed.den as f32;
        if speed_f32 < MIN_SPEED || speed_f32 > MAX_SPEED {
            return Err(EngineError::Refused(format!(
                "filling that span takes {} speed, {}; mark a span closer to the clip's length",
                times(speed_f32),
                speed_bounds()
            )));
        }
        let dur = retimed_duration(
            src_dur_tl,
            speed,
            copy.speed_curve_average(),
            copy.has_speed_curve(),
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
                "{} speed is {}",
                times(speed_f32),
                speed_bounds()
            )));
        }
        let dur = retimed_duration(
            src_dur_tl,
            speed,
            copy.speed_curve_average(),
            copy.has_speed_curve(),
        );
        (speed, dur)
    } else {
        return Err(missing_target());
    };

    let new_id =
        ctx.project
            .place_copy(copy, track, start, Some((derived_speed, duration_ticks)))?;
    Ok((new_id, Box::new(RemoveClipAction { clip: new_id })))
}

fn missing_target() -> EngineError {
    EngineError::Refused("paste retimed requires a target duration or target speed".to_string())
}

/// `speed` as a multiple, to two decimals: `0.01x`, `2400x`.
fn times(speed: f32) -> String {
    format!("{}x", (speed * 100.0).round() / 100.0)
}

/// The speeds a clip plays at, as a clause.
fn speed_bounds() -> String {
    format!("outside {} to {}", times(MIN_SPEED), times(MAX_SPEED))
}
