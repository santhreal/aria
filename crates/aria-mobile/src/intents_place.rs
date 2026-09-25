//! Import and placement intents: appending picks to main, ripple-inserting one,
//! and dropping generated / picture-in-picture / audio content onto a host lane.
//!
//! Declared from `intents.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

pub(super) fn append_main(
    engine: &mut Engine,
    paths: &[PathBuf],
) -> Result<serde_json::Value, EngineError> {
    let main = ensure_main(engine)?;
    let rate = engine.project().timeline().frame_rate;
    let mut clips = Vec::new();
    let mut media_ids = Vec::new();

    for path in paths {
        let media_id = apply_imported(engine, path)?;
        media_ids.push(media_id.raw());
        let media = engine
            .project()
            .media(media_id)
            .ok_or(ModelError::UnknownMedia(media_id))?;
        let source = media.full_range();

        let clip = if media.is_audio_only() {
            // Music/voiceover picks can't sit on the main picture track.
            let main_end = main_track_end(engine.project(), main);
            let len = aria_models::resample(source.duration, rate).value.max(1);
            let range = TimeRange::at_rate(main_end, len, rate);
            let lane = ensure_host_lane(engine, TrackKind::Audio, range, None)?;
            apply_created(
                engine,
                EditCommand::AddClip {
                    track: lane,
                    media: media_id,
                    source,
                    start: RationalTime::new(main_end, rate),
                },
            )?
        } else {
            let main_end = main_track_end(engine.project(), main);
            apply_created(
                engine,
                EditCommand::AddClip {
                    track: main,
                    media: media_id,
                    source,
                    start: RationalTime::new(main_end, rate),
                },
            )?
        };
        clips.push(clip.raw());
    }

    Ok(serde_json::json!({ "clips": clips, "media": media_ids }))
}

pub(super) fn insert_main(
    engine: &mut Engine,
    path: &std::path::Path,
    at_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let main = ensure_main(engine)?;
    let rate = engine.project().timeline().frame_rate;
    let media_id = apply_imported(engine, path)?;
    let media = engine
        .project()
        .media(media_id)
        .ok_or(ModelError::UnknownMedia(media_id))?;
    let source = media.full_range();
    let tick = nearest_main_boundary(engine.project(), main, frame_tick(at_seconds, rate));
    let clip = apply_created(
        engine,
        EditCommand::RippleInsert {
            track: main,
            media: media_id,
            source,
            at: RationalTime::new(tick, rate),
        },
    )?;
    Ok(serde_json::json!({ "clip": clip.raw(), "media": media_id.raw() }))
}

pub(super) fn add_generated(
    engine: &mut Engine,
    kind: TrackKind,
    generator: Generator,
    at_seconds: f64,
    duration_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let start = frame_tick(at_seconds, rate);
    let len = frame_tick(duration_seconds.max(0.1), rate).max(1);
    let range = TimeRange::at_rate(start, len, rate);
    let lane = ensure_host_lane(engine, kind, range, None)?;
    let clip = apply_created(
        engine,
        EditCommand::AddGenerated {
            track: lane,
            generator,
            timeline: range,
        },
    )?;
    Ok(serde_json::json!({ "clip": clip.raw(), "track": lane.raw() }))
}

pub(super) fn add_pip(
    engine: &mut Engine,
    path: &std::path::Path,
    at_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let media_id = apply_imported(engine, path)?;
    let media = engine
        .project()
        .media(media_id)
        .ok_or(ModelError::UnknownMedia(media_id))?;
    if media.is_audio_only() {
        return Err(invalid("picture-in-picture needs visual media"));
    }
    let source = media.full_range();
    let start = frame_tick(at_seconds, rate);
    let len = aria_models::resample(source.duration, rate).value.max(1);
    let range = TimeRange::at_rate(start, len, rate);
    let lane = ensure_host_lane(engine, TrackKind::Video, range, None)?;
    let clip = apply_created(
        engine,
        EditCommand::AddClip {
            track: lane,
            media: media_id,
            source,
            start: RationalTime::new(start, rate),
        },
    )?;
    // The CapCut drop pose: half scale, sitting in the upper half.
    engine.apply(Command::Edit(EditCommand::SetClipTransform {
        clip,
        transform: ClipTransform {
            position: [0.0, -0.18],
            anchor_point: [0.5, 0.5],
            scale: 0.5.into(),
            rotation: 0.0,
            opacity: 1.0,
        },
        at: None,
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw(), "track": lane.raw(), "media": media_id.raw() }))
}

pub(super) fn add_audio(
    engine: &mut Engine,
    path: &std::path::Path,
    at_seconds: f64,
    role: Option<AudioRole>,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let media_id = apply_imported(engine, path)?;
    let media = engine
        .project()
        .media(media_id)
        .ok_or(ModelError::UnknownMedia(media_id))?;
    if !media.has_audio {
        return Err(invalid("file has no audio stream"));
    }
    let source = media.full_range();
    let start = frame_tick(at_seconds, rate);
    let len = aria_models::resample(source.duration, rate).value.max(1);
    let range = TimeRange::at_rate(start, len, rate);
    let lane = ensure_host_lane(engine, TrackKind::Audio, range, None)?;
    let clip = apply_created(
        engine,
        EditCommand::AddClip {
            track: lane,
            media: media_id,
            source,
            start: RationalTime::new(start, rate),
        },
    )?;
    if role.is_some() {
        engine.apply(Command::Edit(EditCommand::SetAudioRole { clip, role }))?;
    }
    Ok(serde_json::json!({ "clip": clip.raw(), "track": lane.raw(), "media": media_id.raw() }))
}
