//! Property intents: speed, audio levels and fades, the transform block, the
//! transform-keyframe toggle, and clip transitions.
//!
//! Declared from `intents.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

pub(super) fn set_speed(
    engine: &mut Engine,
    clip: ClipId,
    speed: f64,
    reversed: bool,
) -> Result<serde_json::Value, EngineError> {
    engine.apply(Command::Edit(EditCommand::SetClipSpeed {
        clip,
        speed: speed_to_rational(speed),
        reversed,
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

pub(super) fn set_speed_preset(
    engine: &mut Engine,
    clip: ClipId,
    preset: Option<String>,
) -> Result<serde_json::Value, EngineError> {
    let curve = match preset.as_deref() {
        Some(id) => Some(
            aria_models::speed_preset(id)
                .ok_or_else(|| invalid(format!("unknown speed preset '{id}'")))?,
        ),
        None => None,
    };
    engine.apply(Command::Edit(EditCommand::SetSpeedCurve { clip, curve }))?;
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

pub(super) fn set_audio(
    engine: &mut Engine,
    clip: ClipId,
    volume: Option<f32>,
    fade_in_seconds: f64,
    fade_out_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    engine.apply(Command::Edit(EditCommand::SetClipAudio {
        clip,
        volume,
        fade_in: at(fade_in_seconds, rate),
        fade_out: at(fade_out_seconds, rate),
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

pub(super) fn set_transform(
    engine: &mut Engine,
    clip: ClipId,
    pos: [f32; 2],
    scale: f32,
    rotation_degrees: f32,
    opacity: f32,
    at_seconds: Option<f64>,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    engine.apply(Command::Edit(EditCommand::SetClipTransform {
        clip,
        transform: ClipTransform {
            // UI position is 0..1 with 0.5 at the canvas center; the model
            // stores an offset from the center.
            position: [pos[0] - 0.5, pos[1] - 0.5],
            anchor_point: [0.5, 0.5],
            scale: Scale2::uniform(scale),
            rotation: rotation_degrees,
            opacity,
        },
        at: at_seconds.map(|s| at(s, rate)),
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

/// The transform properties the diamond toggle stamps.
const KEYFRAME_PARAMS: [ClipParam; 4] = [
    ClipParam::Position,
    ClipParam::Scale,
    ClipParam::Rotation,
    ClipParam::Opacity,
];

pub(super) fn toggle_transform_keyframe(
    engine: &mut Engine,
    clip: ClipId,
    seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let (_, snapshot) = clip_snapshot(engine.project(), clip)?;
    let tick = frame_tick(seconds, rate);
    if !snapshot.timeline.contains(RationalTime::new(tick, rate))? {
        return Err(invalid("keyframe position is outside the clip"));
    }
    let rel = tick - snapshot.timeline.start.value;

    // Which of the four already carry a key here, and what to stamp if none do,
    // both come off each property's registry row. The three per-shape closures
    // this replaces had to be written again for every value kind, and the match
    // that chose between them ended in `_ => false`, so a fifth property added
    // to KEYFRAME_PARAMS would have reported no keyframe and stamped nothing.
    let row = |param: ClipParam| aria_models::param_spec(param);
    let existing: Vec<ClipParam> = KEYFRAME_PARAMS
        .into_iter()
        .filter(|&param| {
            row(param).is_some_and(|spec| {
                spec.keyframes(&snapshot)
                    .iter()
                    .any(|(tick, _)| *tick == rel)
            })
        })
        .collect();

    let position = RationalTime::new(tick, rate);
    if existing.is_empty() {
        // Stamp the pose the clip is showing at `rel`, read through the same
        // rows: a property whose row reports no value is not stamped.
        let stamps: Vec<(ClipParam, ParamValue)> = KEYFRAME_PARAMS
            .into_iter()
            .filter_map(|param| {
                row(param)
                    .and_then(|spec| spec.sample(&snapshot, rel))
                    .map(|value| (param, value))
            })
            .collect();
        for (param, value) in stamps {
            engine.apply(Command::Edit(EditCommand::SetParamKeyframe {
                clip,
                param,
                at: position,
                value,
                easing: Easing::Linear,
                tangents: None,
            }))?;
        }
        Ok(serde_json::json!({ "clip": clip.raw(), "keyframed": true }))
    } else {
        for param in existing {
            engine.apply(Command::Edit(EditCommand::RemoveParamKeyframe {
                clip,
                param,
                at: position,
            }))?;
        }
        Ok(serde_json::json!({ "clip": clip.raw(), "keyframed": false }))
    }
}

pub(super) fn set_transition(
    engine: &mut Engine,
    clip: ClipId,
    transition_id: Option<String>,
    duration_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let (track_id, _) = clip_snapshot(engine.project(), clip)?;
    let existing = engine
        .project()
        .timeline()
        .track(track_id)
        .and_then(|t| t.transition_at(clip).map(|t| t.transition_id.clone()));

    match (existing, transition_id) {
        (Some(_), None) => {
            engine.apply(Command::Edit(EditCommand::RemoveTransition { clip }))?;
        }
        (None, None) => {}
        (existing, Some(id)) => {
            if existing.as_deref() != Some(id.as_str()) {
                if existing.is_some() {
                    engine.apply(Command::Edit(EditCommand::RemoveTransition { clip }))?;
                }
                engine.apply(Command::Edit(EditCommand::AddTransition {
                    clip,
                    transition_id: id,
                }))?;
            }
            if duration_seconds > 0.0 {
                engine.apply(Command::Edit(EditCommand::SetTransition {
                    clip,
                    duration: frame_tick(duration_seconds, rate).max(1),
                }))?;
            }
        }
    }
    Ok(serde_json::json!({ "clip": clip.raw() }))
}
