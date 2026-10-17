//! Clip content intents: duplicating a clip with its whole look, relinking its
//! media, detaching its audio, and freezing a frame into a still.
//!
//! Declared from `intents.rs` with `#[path]`; `use super::*` reaches that
//! module's private items and its own imports, so the move changed no name
//! resolution.

use super::*;

pub(super) fn duplicate(
    engine: &mut Engine,
    clip: ClipId,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let main = crate::ui_state::main_track(engine.project());
    let (track_id, snapshot) = clip_snapshot(engine.project(), clip)?;
    let track_kind = engine
        .project()
        .timeline()
        .track(track_id)
        .ok_or(ModelError::UnknownTrack(track_id))?
        .kind;

    let s = snapshot.timeline.start.value;
    let d = snapshot.timeline.duration.value;
    let copy_start = s + d;

    let new = match &snapshot.content {
        ClipSource::Generated(generator) => {
            let range = TimeRange::at_rate(copy_start, d, rate);
            let lane = if !engine
                .project()
                .timeline()
                .track(track_id)
                .ok_or(ModelError::UnknownTrack(track_id))?
                .has_overlap(range, None)?
            {
                track_id
            } else {
                ensure_host_lane(engine, track_kind, range, None)?
            };
            apply_created(
                engine,
                EditCommand::AddGenerated {
                    track: lane,
                    generator: generator.clone(),
                    timeline: range,
                },
            )?
        }
        ClipSource::Media { media, source } => {
            // The copy is placed at source-derived length first, then retimed
            // back to `d`; reserve the larger of the two extents.
            let placed_len = aria_models::resample(source.duration, rate).value.max(1);
            let reserve = placed_len.max(d);

            let new = if Some(track_id) == main {
                engine.apply(Command::Edit(EditCommand::ShiftClips {
                    track: track_id,
                    from: RationalTime::new(copy_start, rate),
                    delta: RationalTime::new(reserve, rate),
                }))?;
                apply_created(
                    engine,
                    EditCommand::AddClip {
                        track: track_id,
                        media: *media,
                        source: *source,
                        start: RationalTime::new(copy_start, rate),
                    },
                )?
            } else {
                let range = TimeRange::at_rate(copy_start, reserve, rate);
                let lane = if !engine
                    .project()
                    .timeline()
                    .track(track_id)
                    .ok_or(ModelError::UnknownTrack(track_id))?
                    .has_overlap(range, None)?
                {
                    track_id
                } else {
                    ensure_host_lane(engine, track_kind, range, None)?
                };
                apply_created(
                    engine,
                    EditCommand::AddClip {
                        track: lane,
                        media: *media,
                        source: *source,
                        start: RationalTime::new(copy_start, rate),
                    },
                )?
            };

            // Re-derive the copy's duration (speed / reverse), then trim any
            // over-reservation back out of the main track.
            if snapshot.speed != Rational::new(1, 1) || snapshot.reversed {
                engine.apply(Command::Edit(EditCommand::SetClipSpeed {
                    clip: new,
                    speed: snapshot.speed,
                    reversed: snapshot.reversed,
                }))?;
            }
            if Some(track_id) == main && reserve > d {
                engine.apply(Command::Edit(EditCommand::ShiftClips {
                    track: track_id,
                    from: RationalTime::new(copy_start + reserve, rate),
                    delta: RationalTime::new(d - reserve, rate),
                }))?;
            }

            // Carry the mix + framing over (audio applies to media clips only).
            engine.apply(Command::Edit(EditCommand::SetClipAudio {
                clip: new,
                volume: Some(snapshot.volume.sample(0)),
                fade_in: RationalTime::new(snapshot.fade_in, rate),
                fade_out: RationalTime::new(snapshot.fade_out, rate),
            }))?;
            let crop = snapshot.crop.sample(0);
            if !crop.is_full() || snapshot.flip_h || snapshot.flip_v {
                engine.apply(Command::Edit(EditCommand::SetClipCrop {
                    clip: new,
                    crop,
                    flip_h: snapshot.flip_h,
                    flip_v: snapshot.flip_v,
                    at: None,
                }))?;
            }
            new
        }
        // A multicam clip carries a cut list and a nested sequence carries a
        // whole timeline; the phone surface offers neither, so duplicating one
        // would silently drop what makes it that clip. Refuse by name instead.
        ClipSource::Multicam { .. } => {
            return Err(EngineError::Unsupported(
                "duplicating a multicam clip needs the desktop editor".to_string(),
            ));
        }
        ClipSource::Sequence { .. } => {
            return Err(EngineError::Unsupported(
                "duplicating a nested sequence needs the desktop editor".to_string(),
            ));
        }
    };

    // Visual placement + effect chain (animation flattens to its start value).
    if track_kind.is_visual() {
        let pose = snapshot.transform.sample(0);
        if !pose.is_identity() {
            engine.apply(Command::Edit(EditCommand::SetClipTransform {
                clip: new,
                transform: pose,
                at: None,
            }))?;
        }
    }
    for effect in &snapshot.effects {
        engine.apply(Command::Edit(EditCommand::AddEffect {
            clip: new,
            effect_id: effect.effect_id.clone(),
        }))?;
    }
    copy_look(engine, &snapshot, new)?;

    Ok(serde_json::json!({ "clip": new.raw() }))
}

/// Carry the Phase I look fields onto a duplicated clip. Each setter runs only
/// when the source had the property, so validation never trips on defaults.
fn copy_look(engine: &mut Engine, snapshot: &Clip, new: ClipId) -> Result<(), EngineError> {
    for mask in &snapshot.masks {
        engine.apply(Command::Edit(EditCommand::AddClipMask {
            clip: new,
            mask: mask.clone(),
        }))?;
    }
    if snapshot.chroma_key.is_some() {
        engine.apply(Command::Edit(EditCommand::SetClipChroma {
            clip: new,
            chroma: snapshot.chroma_key.clone(),
        }))?;
    }
    if snapshot.matte.is_some() {
        engine.apply(Command::Edit(EditCommand::SetClipMatte {
            clip: new,
            matte: snapshot.matte.clone(),
        }))?;
    }
    if snapshot.stabilize.is_some() {
        engine.apply(Command::Edit(EditCommand::SetClipStabilize {
            clip: new,
            stabilize: snapshot.stabilize,
        }))?;
    }
    if snapshot.filter.is_some() {
        engine.apply(Command::Edit(EditCommand::SetClipFilter {
            clip: new,
            filter: snapshot.filter.clone(),
        }))?;
    }
    if !snapshot.adjust.is_neutral() {
        engine.apply(Command::Edit(EditCommand::SetClipAdjustments {
            clip: new,
            adjust: snapshot.adjust.clone(),
        }))?;
    }
    for (slot, animation) in [
        (aria_models::AnimationSlot::In, &snapshot.animation_in),
        (aria_models::AnimationSlot::Out, &snapshot.animation_out),
        (aria_models::AnimationSlot::Combo, &snapshot.animation_combo),
    ] {
        if animation.is_some() {
            engine.apply(Command::Edit(EditCommand::SetClipAnimation {
                clip: new,
                slot,
                animation: animation.clone(),
            }))?;
        }
    }
    if snapshot.audio_role.is_some() {
        engine.apply(Command::Edit(EditCommand::SetAudioRole {
            clip: new,
            role: snapshot.audio_role,
        }))?;
    }
    Ok(())
}

pub(super) fn replace_media(
    engine: &mut Engine,
    clip: ClipId,
    path: &std::path::Path,
) -> Result<serde_json::Value, EngineError> {
    let (_, snapshot) = clip_snapshot(engine.project(), clip)?;
    let ClipSource::Media { .. } = snapshot.content else {
        return Err(invalid("replace targets a media clip"));
    };

    let media_id = apply_imported(engine, path)?;
    let media = engine
        .project()
        .media(media_id)
        .ok_or(ModelError::UnknownMedia(media_id))?;

    // Source ticks the clip consumes at its current speed, at the new media's
    // native rate.
    let needed = snapshot
        .scale_by_speed(aria_models::resample(snapshot.timeline.duration, media.frame_rate).value)
        .max(1);
    if !media.is_image && needed > media.duration.value {
        return Err(EngineError::Import(format!(
            "replacement media is shorter than the clip needs ({} < {} ticks)",
            media.duration.value, needed
        )));
    }
    engine.apply(Command::Edit(EditCommand::SetClipMedia {
        clip,
        media: media_id,
        source: TimeRange::at_rate(0, needed, media.frame_rate),
    }))?;
    Ok(serde_json::json!({ "clip": clip.raw(), "media": media_id.raw() }))
}

pub(super) fn extract_audio(
    engine: &mut Engine,
    clip: ClipId,
) -> Result<serde_json::Value, EngineError> {
    let audio_clip = match engine.apply(Command::Edit(EditCommand::ExtractAudio {
        clip,
        to_track: None,
    }))? {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => {
            return Err(invalid(format!(
                "expected an extracted audio clip, got {other:?}"
            )));
        }
    };
    let lane = engine
        .project()
        .timeline()
        .track_of(audio_clip)
        .ok_or(ModelError::UnknownClip(audio_clip))?;
    Ok(serde_json::json!({ "clip": audio_clip.raw(), "track": lane.raw() }))
}

pub(super) fn freeze(
    engine: &mut Engine,
    clip: ClipId,
    seconds: f64,
    png_path: &std::path::Path,
    duration_seconds: f64,
) -> Result<serde_json::Value, EngineError> {
    let rate = engine.project().timeline().frame_rate;
    let (track, snapshot) = require_main_clip(engine.project(), clip)?;
    let ClipSource::Media { media, .. } = snapshot.content else {
        return Err(invalid("freeze frame targets a media clip"));
    };
    let entry = engine
        .project()
        .media(media)
        .ok_or(ModelError::UnknownMedia(media))?
        .clone();
    if entry.is_image {
        return Err(invalid("clip is already a still"));
    }

    // Which source frame sits under the playhead (speed/reverse honoured)?
    // Edge hits clamp to the nearest interior frame so a freeze at a cut
    // boundary still has content to freeze.
    let s = snapshot.timeline.start.value;
    let d = snapshot.timeline.duration.value;
    let local = (frame_tick(seconds, rate) - s).clamp(0, d);
    let probe_tick = s + local.min(d - 1);
    let source_time = snapshot
        .source_time_at(RationalTime::new(probe_tick, rate))?
        .ok_or_else(|| invalid("playhead is outside the clip"))?;

    // Extract the frame at native size before touching the timeline, so a
    // decode failure leaves no half-applied edit behind.
    let frame = render_media_frame(&entry, source_time)?;
    write_freeze_png_async(png_path.to_path_buf(), frame)?;
    let still = apply_imported(engine, png_path)?;
    let still_window = engine
        .project()
        .media(still)
        .ok_or(ModelError::UnknownMedia(still))?
        .duration
        .value;

    // Mid-clip freezes split first; edge hits insert flush at the boundary.
    let mut split = None;
    let insert_at = if local == 0 {
        s
    } else if local >= d {
        s + d
    } else {
        split = Some(apply_created(
            engine,
            EditCommand::SplitClip {
                clip,
                at: RationalTime::new(s + local, rate),
            },
        )?);
        s + local
    };

    let len =
        frame_tick(duration_seconds.max(0.1), aria_models::STILL_TICK_RATE).clamp(1, still_window);
    let freeze_clip = apply_created(
        engine,
        EditCommand::RippleInsert {
            track,
            media: still,
            source: TimeRange::at_rate(0, len, aria_models::STILL_TICK_RATE),
            at: RationalTime::new(insert_at, rate),
        },
    )?;

    // The still inherits the source clip's look at the frozen instant:
    // framing flattens to a constant, the effect chain carries by id.
    let pose = snapshot.transform.sample(local);
    if !pose.is_identity() {
        engine.apply(Command::Edit(EditCommand::SetClipTransform {
            clip: freeze_clip,
            transform: pose,
            at: None,
        }))?;
    }
    for effect in &snapshot.effects {
        engine.apply(Command::Edit(EditCommand::AddEffect {
            clip: freeze_clip,
            effect_id: effect.effect_id.clone(),
        }))?;
    }

    Ok(serde_json::json!({
        "clip": freeze_clip.raw(),
        "media": still.raw(),
        "split": split.map(|c| c.raw()),
    }))
}

/// Render one frame of `media` at native size through the normal
/// resolve/render path (a scratch single-clip project, the thumbnailer
/// trick), so orientation and aspect handling can never drift from preview.
fn render_media_frame(
    media: &aria_models::MediaSource,
    source_time: RationalTime,
) -> Result<aria_core::RgbaImage, EngineError> {
    let rate = media.frame_rate;
    let frames = media.duration.value.max(1);
    let mut scratch = Project::new("freeze", rate);
    let id = scratch.add_media(media.clone());
    let track = scratch.add_track(TrackKind::Video, "Media");
    scratch.add_clip(
        track,
        id,
        TimeRange::at_rate(0, frames, rate),
        RationalTime::new(0, rate),
    )?;
    let mut renderer = aria_render::Renderer::new_headless()?;
    let tick = source_time.value.clamp(0, frames - 1);
    Ok(renderer.render_frame(&scratch, RationalTime::new(tick, rate))?)
}

/// Write a tightly-packed RGBA8 frame as an 8-bit PNG asynchronously off the session thread.
fn write_freeze_png_async(
    path: std::path::PathBuf,
    image: aria_core::RgbaImage,
) -> Result<(), EngineError> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let res = write_freeze_png(&path, &image);
        let _ = tx.send(res);
    });
    rx.recv()
        .map_err(|_| EngineError::Import("freeze encode thread disconnected".into()))?
}

/// Write a tightly-packed RGBA8 frame as an 8-bit PNG.
fn write_freeze_png(
    path: &std::path::Path,
    image: &aria_core::RgbaImage,
) -> Result<(), EngineError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| EngineError::Import(format!("freeze png: {e}")))?;
    writer
        .write_image_data(&image.pixels)
        .map_err(|e| EngineError::Import(format!("freeze png: {e}")))?;
    Ok(())
}
