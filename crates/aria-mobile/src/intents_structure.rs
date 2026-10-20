//! Structural intents: splitting, trimming (ripple on main, plain on a lane),
//! moving a clip between lanes, and lifting one into the main track.
//!
//! Declared from `intents.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

pub(super) fn split(
    engine: &mut Engine,
    clip: ClipId,
    seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let new = apply_created(
        engine,
        EditCommand::SplitClip {
            clip,
            at: at(seconds, rate),
        },
    )?;
    Ok(serde_json::json!({ "clip": new.raw() }))
}

pub(super) fn ripple_trim_main(
    engine: &mut Engine,
    clip: ClipId,
    edge: TrimEdge,
    delta_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let (track, snapshot) = require_main_clip(engine.project(), clip)?;
    let s = snapshot.timeline.start.value;
    let d = snapshot.timeline.duration.value;

    // Clamp shrink so at least one frame survives; growth is validated by
    // TrimClip against the source window.
    let dt = frame_tick(delta_seconds.abs(), rate) * delta_seconds.signum() as i64;
    let dt = match edge {
        TrimEdge::Leading => dt.min(d - 1),
        TrimEdge::Trailing => dt.max(-(d - 1)),
    };
    if dt == 0 {
        return Ok(serde_json::json!({ "clip": clip.raw() }));
    }

    let shift = |from: i64, delta: i64| EditCommand::ShiftClips {
        track,
        from: RationalTime::new(from, rate),
        delta: RationalTime::new(delta, rate),
    };
    let trim = |start: i64, len: i64| EditCommand::TrimClip {
        clip,
        timeline: TimeRange::at_rate(start, len, rate),
    };

    // Ordering rule: free space before consuming it. Shrinks trim first and
    // close the gap after; extensions open space first and trim into it.
    let commands: [EditCommand; 2] = match (edge, dt > 0) {
        (TrimEdge::Leading, true) => [trim(s + dt, d - dt), shift(s + dt, -dt)],
        (TrimEdge::Leading, false) => [shift(s, -dt), trim(s, d - dt)],
        (TrimEdge::Trailing, true) => [shift(s + d, dt), trim(s, d + dt)],
        (TrimEdge::Trailing, false) => [trim(s, d + dt), shift(s + d, dt)],
    };
    for command in commands {
        engine.apply(Command::Edit(command))?;
    }
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

pub(super) fn trim_lane(
    engine: &mut Engine,
    clip: ClipId,
    start_seconds: f64,
    length_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let start = frame_tick(start_seconds, rate);
    let len = frame_tick(length_seconds, rate).max(1);
    engine.apply(Command::Edit(EditCommand::TrimClip {
        clip,
        timeline: TimeRange::at_rate(start, len, rate),
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw() }))
}

pub(super) fn move_lane(
    engine: &mut Engine,
    clip: ClipId,
    track: Option<TrackId>,
    start_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let (current_track, snapshot) = clip_snapshot(engine.project(), clip)?;
    let start = frame_tick(start_seconds, rate);
    let range = TimeRange::at_rate(start, snapshot.timeline.duration.value.max(1), rate);

    let target = match track {
        Some(target) => target,
        None => {
            let kind = engine
                .project()
                .timeline()
                .require_track(current_track)?
                .kind;
            ensure_host_lane(engine, kind, range, Some(clip))?
        }
    };
    // Lifting a clip off the magnetic main track closes the hole it leaves
    // (later clips slide left), like every ripple edit there.
    let from_main = Some(current_track) == crate::ui_state::main_track(engine.project())
        && current_track != target;
    engine.apply(Command::Edit(EditCommand::MoveClip {
        clip,
        to_track: target,
        start: RationalTime::new(start, rate),
    }))?;
    if from_main {
        let s = snapshot.timeline.start.value;
        let d = snapshot.timeline.duration.value;
        engine.apply(Command::Edit(EditCommand::ShiftClips {
            track: current_track,
            from: RationalTime::new(s + d, rate),
            delta: RationalTime::new(-d, rate),
        }))?;
    }
    Ok(serde_json::json!({ "clip": clip.raw(), "track": target.raw() }))
}

pub(super) fn insert_into_main(
    engine: &mut Engine,
    clip: ClipId,
    index: usize,
) -> Result<serde_json::Value, EngineError> {
    let main = ensure_main(engine)?;
    let rate = engine.project().timeline().frame_rate;
    let (current_track, snapshot) = clip_snapshot(engine.project(), clip)?;
    let len = snapshot.timeline.duration.value;

    let track = engine.project().timeline().require_track(main)?;
    let on_main = current_track == main;

    // Boundaries of the (post-removal, for reorders) slot layout.
    let others: Vec<i64> = track
        .clips_ordered()
        .iter()
        .filter(|c| c.id != clip)
        .map(|c| c.timeline.duration.value)
        .collect();
    let index = index.min(others.len());
    let target: i64 = others.iter().take(index).sum();

    let move_to = |start: i64| EditCommand::MoveClip {
        clip,
        to_track: main,
        start: RationalTime::new(start, rate),
    };
    let shift = |from: i64, delta: i64| EditCommand::ShiftClips {
        track: main,
        from: RationalTime::new(from, rate),
        delta: RationalTime::new(delta, rate),
    };

    if on_main {
        let s = snapshot.timeline.start.value;
        if s == target {
            return Ok(serde_json::json!({ "clip": clip.raw() }));
        }
        // Park at the end, close the old gap, open the new slot, land.
        let park = track.content_end();
        engine.apply(Command::Edit(move_to(park)))?;
        engine.apply(Command::Edit(shift(s, -len)))?;
        engine.apply(Command::Edit(shift(target, len)))?;
        engine.apply(Command::Edit(move_to(target)))?;
    } else {
        engine.apply(Command::Edit(shift(target, len)))?;
        engine.apply(Command::Edit(move_to(target)))?;
    }
    Ok(serde_json::json!({ "clip": clip.raw() }))
}
