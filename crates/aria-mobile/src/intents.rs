//! Gesture-level editor intents, composed into engine command batches.
//!
//! The shells speak in gestures ("append these picks", "ripple-trim this main
//! clip", "drop a text at the playhead"); the engine speaks in atomic
//! [`EditCommand`]s. The translation — seconds→ticks, host-lane selection,
//! ripple choreography, undo grouping — is editor *logic* shared by iOS and
//! Android, so it lives here in Rust. Every multi-command intent runs inside
//! one history group: it undoes as a single step and rolls back atomically on
//! mid-flight failure.
//!
//! Wire shape: `{"intent": "add_text", ...}` (snake_case tags and fields,
//! seconds as `f64`). Responses are small JSON objects with the created ids;
//! shells refresh the full `ui_state` after every intent anyway.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use aria_commands::{Command, EditCommand, EditOutcome, ProjectCommand};
use aria_engine::{ApplyOutcome, Engine, EngineError};
use aria_models::{
    AudioRole, Clip, ClipId, ClipParam, ClipSource, ClipTransform, Easing, Generator, MediaId,
    ModelError, ParamValue, Project, Rational, RationalTime, Scale2, TimeRange, TrackId, TrackKind,
};

// The handlers, grouped by the gesture family they serve. Each child is
// declared with `#[path]` and opens with `use super::*`, so every helper below
// resolves exactly as it did when all of them lived in this one file.
#[path = "intents_content.rs"]
mod intents_content;
#[path = "intents_place.rs"]
mod intents_place;
#[path = "intents_props.rs"]
mod intents_props;
#[path = "intents_structure.rs"]
mod intents_structure;
#[path = "intents_wire.rs"]
mod intents_wire;

/// The wire types are this module's public surface; the split is internal.
pub use intents_wire::*;
// Globbed so `run` reaches every handler by the bare name it always used.
use intents_content::*;
use intents_place::*;
use intents_props::*;
use intents_structure::*;

/// Execute one intent. Multi-command intents run in a history group: one undo
/// step, atomic rollback on failure. Returns the intent's response payload.
pub fn run(engine: &mut Engine, intent: Intent) -> Result<serde_json::Value, EngineError> {
    match intent {
        Intent::AppendMain { paths } => grouped(engine, |e| append_main(e, &paths)),
        Intent::InsertMain { path, at_seconds } => {
            grouped(engine, |e| insert_main(e, &path, at_seconds))
        }
        Intent::Split { clip, seconds } => split(engine, clip, seconds),
        Intent::RippleTrimMain {
            clip,
            edge,
            delta_seconds,
        } => grouped(engine, |e| ripple_trim_main(e, clip, edge, delta_seconds)),
        Intent::TrimLane {
            clip,
            start_seconds,
            length_seconds,
        } => trim_lane(engine, clip, start_seconds, length_seconds),
        Intent::MoveLane {
            clip,
            track,
            start_seconds,
        } => grouped(engine, |e| move_lane(e, clip, track, start_seconds)),
        Intent::InsertIntoMain { clip, index } => {
            grouped(engine, |e| insert_into_main(e, clip, index))
        }
        Intent::AddText {
            text,
            at_seconds,
            duration_seconds,
        } => grouped(engine, |e| {
            add_generated(
                e,
                TrackKind::Text,
                Generator::text(text.clone()),
                at_seconds,
                duration_seconds,
            )
        }),
        Intent::AddSticker {
            at_seconds,
            duration_seconds,
            asset,
        } => grouped(engine, |e| {
            let asset = asset
                .clone()
                .or_else(|| {
                    aria_models::sticker_catalog()
                        .first()
                        .map(|spec| spec.id.to_owned())
                })
                .ok_or_else(|| invalid("no stickers are bundled"))?;
            add_generated(
                e,
                TrackKind::Sticker,
                Generator::sticker(asset),
                at_seconds,
                duration_seconds,
            )
        }),
        Intent::AddEffect {
            kind,
            at_seconds,
            duration_seconds,
        } => grouped(engine, |e| {
            add_generated(
                e,
                kind.track_kind(),
                kind.generator(),
                at_seconds,
                duration_seconds,
            )
        }),
        Intent::AddPip { path, at_seconds } => grouped(engine, |e| add_pip(e, &path, at_seconds)),
        Intent::AddAudio {
            path,
            at_seconds,
            role,
        } => grouped(engine, |e| add_audio(e, &path, at_seconds, role)),
        Intent::Duplicate { clip } => grouped(engine, |e| duplicate(e, clip)),
        Intent::ReplaceMedia { clip, path } => grouped(engine, |e| replace_media(e, clip, &path)),
        Intent::ExtractAudio { clip } => extract_audio(engine, clip),
        Intent::Freeze {
            clip,
            seconds,
            png_path,
            duration_seconds,
        } => grouped(engine, |e| {
            freeze(e, clip, seconds, &png_path, duration_seconds)
        }),
        Intent::SetSpeed {
            clip,
            speed,
            reversed,
        } => set_speed(engine, clip, speed, reversed),
        Intent::SetSpeedPreset { clip, preset } => set_speed_preset(engine, clip, preset),
        Intent::SetAudio {
            clip,
            volume,
            fade_in_seconds,
            fade_out_seconds,
        } => set_audio(engine, clip, volume, fade_in_seconds, fade_out_seconds),
        Intent::SetTransform {
            clip,
            pos_x,
            pos_y,
            scale,
            rotation_degrees,
            opacity,
            at_seconds,
        } => set_transform(
            engine,
            clip,
            [pos_x, pos_y],
            scale,
            rotation_degrees,
            opacity,
            at_seconds,
        ),
        Intent::ToggleTransformKeyframe { clip, seconds } => {
            grouped(engine, |e| toggle_transform_keyframe(e, clip, seconds))
        }
        Intent::SetTransition {
            clip,
            transition_id,
            duration_seconds,
        } => grouped(engine, |e| {
            set_transition(e, clip, transition_id.clone(), duration_seconds)
        }),
    }
}

// --- tick math --------------------------------------------------------------

/// Nearest frame tick at `rate` for a wall-clock position (never negative).
pub fn frame_tick(seconds: f64, rate: Rational) -> i64 {
    (seconds.max(0.0) * rate.as_f64()).round() as i64
}

fn at(seconds: f64, rate: Rational) -> RationalTime {
    RationalTime::new(frame_tick(seconds, rate), rate)
}

/// A UI speed multiplier as the exact rational the model stores (milli
/// precision, clamped to the model's speed range).
pub fn speed_to_rational(speed: f64) -> Rational {
    let clamped = speed.clamp(0.05, 100.0);
    Rational::new((clamped * 1000.0).round() as i32, 1000)
}

fn invalid(msg: impl Into<String>) -> EngineError {
    ModelError::InvalidParam(msg.into()).into()
}

// --- engine plumbing ---------------------------------------------------------

/// Run `f` inside one history group; commit on success, roll back on failure.
fn grouped<T>(
    engine: &mut Engine,
    f: impl FnOnce(&mut Engine) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    engine.begin_group();
    match f(engine) {
        Ok(value) => {
            engine.commit_group();
            Ok(value)
        }
        Err(err) => {
            engine.rollback_group();
            Err(err)
        }
    }
}

fn apply_created(engine: &mut Engine, command: EditCommand) -> Result<ClipId, EngineError> {
    match engine.apply(Command::Edit(command))? {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => Ok(id),
        other => Err(invalid(format!("expected a created clip, got {other:?}"))),
    }
}

fn apply_created_track(engine: &mut Engine, command: EditCommand) -> Result<TrackId, EngineError> {
    match engine.apply(Command::Edit(command))? {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => Ok(id),
        other => Err(invalid(format!("expected a created track, got {other:?}"))),
    }
}

fn apply_imported(engine: &mut Engine, path: &std::path::Path) -> Result<MediaId, EngineError> {
    match engine.apply(Command::Project(ProjectCommand::Import {
        path: path.to_path_buf(),
    }))? {
        ApplyOutcome::Imported { media } => Ok(media),
        other => Err(invalid(format!(
            "expected an import outcome, got {other:?}"
        ))),
    }
}

fn clip_snapshot(project: &Project, clip: ClipId) -> Result<(TrackId, Clip), EngineError> {
    let track = project
        .timeline()
        .track_of(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let snapshot = project
        .timeline()
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    Ok((track, snapshot.clone()))
}

// --- lane selection (the shared "host lane" policy) ---------------------------

/// Where a clip of `kind` spanning `range` should land.
#[derive(Debug, PartialEq, Eq)]
pub enum HostLanePlan {
    /// Reuse this lane, whose span at the range is free.
    Existing(TrackId),
    /// Create a lane at this stack position (`None` = top of the stack).
    Create { index: Option<usize> },
}

/// First non-main lane of `kind` (scanning UI rows top to bottom) whose span
/// at `range` is free; otherwise a plan to create one at the kind's default
/// position — video just above main, generated kinds at the bottom of the
/// overlay block, audio at the bottom of the stack.
pub fn host_lane_plan(
    project: &Project,
    kind: TrackKind,
    range: TimeRange,
    exclude: Option<ClipId>,
) -> Result<HostLanePlan, EngineError> {
    let main = crate::ui_state::main_track(project);
    let timeline = project.timeline();
    for id in crate::ui_state::lane_rows(project) {
        if Some(id) == main {
            continue;
        }
        let Some(track) = timeline.track(id) else {
            continue;
        };
        if track.kind == kind && !track.has_overlap(range, exclude)? {
            return Ok(HostLanePlan::Existing(id));
        }
    }
    Ok(HostLanePlan::Create {
        index: new_lane_index(project, kind),
    })
}

/// Stack position (0 = bottom) for a brand-new lane of `kind`, mirroring the
/// shells' row conventions: video overlays go directly above main; text /
/// sticker / effect kinds slot in *below* the existing overlay block (bottom
/// row of that group in the UI); audio appends and the timeline's audio-floor
/// invariant sinks it.
fn new_lane_index(project: &Project, kind: TrackKind) -> Option<usize> {
    let order = project.timeline().order();
    let position = |id: TrackId| order.iter().position(|t| *t == id);
    if kind.is_audio() {
        return None;
    }
    if kind.can_be_main() {
        let main = crate::ui_state::main_track(project)?;
        return position(main).map(|p| p + 1);
    }
    project
        .timeline()
        .tracks_ordered()
        .filter(|t| crate::ui_state::in_overlay_block(t.kind))
        .filter_map(|t| position(t.id))
        .min()
}

/// Resolve a [`host_lane_plan`], creating the lane when needed.
fn ensure_host_lane(
    engine: &mut Engine,
    kind: TrackKind,
    range: TimeRange,
    exclude: Option<ClipId>,
) -> Result<TrackId, EngineError> {
    match host_lane_plan(engine.project(), kind, range, exclude)? {
        HostLanePlan::Existing(id) => Ok(id),
        HostLanePlan::Create { index } => apply_created_track(
            engine,
            EditCommand::AddTrack {
                kind,
                name: kind.label().into(),
                index,
                pinned: false,
            },
        ),
    }
}

/// The main track, created (at the bottom of the stack) on first use.
fn ensure_main(engine: &mut Engine) -> Result<TrackId, EngineError> {
    if let Some(main) = crate::ui_state::main_track(engine.project()) {
        return Ok(main);
    }
    apply_created_track(
        engine,
        EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "Main".into(),
            index: Some(0),
            pinned: false,
        },
    )
}

// --- main-track geometry (shared by the intents that touch main) -------------

fn main_track_end(project: &Project, main: TrackId) -> i64 {
    project
        .timeline()
        .track(main)
        .map(|track| track.content_end())
        .unwrap_or(0)
}

/// The main-track clip boundary tick nearest `t` (a ripple insert must land
/// between clips, never inside one).
pub fn nearest_main_boundary(project: &Project, main: TrackId, tick: i64) -> i64 {
    let Some(track) = project.timeline().track(main) else {
        return 0;
    };
    // Every junction an insert may land on: the head of the track, plus the
    // end of each clip (a gapless main track makes those the only cuts).
    std::iter::once(0)
        .chain(track.clips_ordered().iter().map(|c| c.timeline.end_tick()))
        .min_by_key(|boundary| (boundary - tick).abs())
        .unwrap_or(0)
}

fn require_main_clip(project: &Project, clip: ClipId) -> Result<(TrackId, Clip), EngineError> {
    let (track, snapshot) = clip_snapshot(project, clip)?;
    if Some(track) != crate::ui_state::main_track(project) {
        return Err(invalid("clip is not on the main track"));
    }
    Ok((track, snapshot))
}

#[cfg(test)]
#[path = "intents_tests.rs"]
mod tests;

// The gesture cases: one file per family, split at the file ceiling. The
// fixtures live in `gesture_tests` and the property half reaches them through
// `use super::gesture_tests::*`.
#[cfg(test)]
#[path = "intents_edit_tests.rs"]
mod edit_tests;
#[cfg(test)]
#[path = "intents_gesture_tests.rs"]
mod gesture_tests;
#[cfg(test)]
#[path = "intents_prop_tests.rs"]
mod prop_tests;
