//! The incremental projection against the full one.
//!
//! The class these close: a lane kept from the previous store while something
//! it draws moved. Every edit in the census (`aria_commands::every_edit_command`)
//! runs on a fresh fixture, once aimed at a text clip on a lane with a
//! transition and once at a media clip linked across three lanes. The store
//! projected from the previous one must equal the store projected from nothing,
//! after the edit and again after its undo.
//!
//! Each case also records how much it re-projects in [`REACH`]: refused, some
//! lanes kept, or every lane rebuilt. A command added to the census fails here
//! until its row exists, and a row whose command left the census fails too.
//!
//! What they do not catch: an input that no census edit can move on this
//! fixture. A refused case proves only that a refusal keeps every lane.

use aria_commands::{Command, EditCommand, EditOutcome, every_edit_command};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AudioInsertTarget, ClipId, Generator, MediaId, MediaSource, Project, Rational, RationalTime,
    TimeRange, TrackId, TrackKind,
};

use super::{LaneBasis, Previous};
use crate::projection::{project_to_ui_from, project_to_ui_with};
use crate::types::UiEditorStore;

/// The census builds its samples at this rate; an edit on another timebase is
/// refused before its handler runs.
const RATE: Rational = Rational::FPS_30;

/// How much of the store an edit re-projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The edit was refused; every lane is kept.
    Refused,
    /// At least one lane kept its clips.
    Lanes,
    /// Every lane was rebuilt.
    Every,
}
use Reach::{Every, Lanes, Refused};

/// Per census command: the reach aimed at the text clip, then at the linked
/// media clip.
const REACH: &[(&str, Reach, Reach)] = &[
    ("AddTrack", Lanes, Lanes),
    ("AddClip", Refused, Refused),
    ("ExtractAudio", Refused, Refused),
    ("DuplicateClip", Lanes, Lanes),
    ("PasteRetimed", Refused, Lanes),
    ("PasteAttributes", Refused, Refused),
    ("RemoveAttributes", Lanes, Lanes),
    ("FreezeFrame", Refused, Refused),
    ("BakeClip", Lanes, Refused),
    ("RestoreBakedClip", Refused, Refused),
    ("AddGenerated", Refused, Refused),
    ("SetGenerator", Lanes, Refused),
    ("SetClipMedia", Refused, Lanes),
    ("SetReplaceable", Refused, Refused),
    ("SetTextEditable", Lanes, Refused),
    ("SetClipTransform", Lanes, Refused),
    ("SetClipParent", Refused, Refused),
    ("SetParamKeyframe", Refused, Refused),
    ("RemoveParamKeyframe", Refused, Refused),
    ("ApplyEasingPreset", Refused, Refused),
    ("SetParamConstant", Lanes, Refused),
    ("SetParamDriver", Lanes, Refused),
    ("SetClipSpeed", Refused, Lanes),
    ("SetSpeedCurve", Refused, Lanes),
    ("SetClipPitch", Refused, Lanes),
    ("SetClipCrop", Lanes, Refused),
    ("SetClipAudio", Refused, Lanes),
    ("SetClipDenoise", Refused, Lanes),
    ("SetClipLabel", Lanes, Lanes),
    ("SetClipName", Lanes, Lanes),
    ("SetClipType", Lanes, Lanes),
    ("SetClipLineParam", Lanes, Refused),
    ("AddClipMask", Lanes, Refused),
    ("SetClipMask", Refused, Refused),
    ("RemoveClipMask", Refused, Refused),
    ("MoveClipMask", Refused, Refused),
    ("SetClipBlendMode", Lanes, Refused),
    ("SetClipMotionBlur", Lanes, Refused),
    ("SetClipLayerStyles", Lanes, Refused),
    ("SetClipChroma", Refused, Refused),
    ("SetClipMatte", Lanes, Refused),
    ("SetClipStabilize", Refused, Refused),
    ("SetClipFilter", Lanes, Refused),
    ("SetClipLut", Lanes, Refused),
    ("SetClipAdjustments", Lanes, Refused),
    ("SetClipAnimation", Lanes, Refused),
    ("SetAudioRole", Refused, Lanes),
    ("AddEffect", Lanes, Refused),
    ("RemoveEffect", Refused, Refused),
    ("MoveEffect", Refused, Refused),
    ("SetEffectParam", Refused, Refused),
    ("SetEffectInstance", Refused, Refused),
    ("SetEffectBypass", Refused, Refused),
    ("ApplyPreset", Lanes, Refused),
    ("AddTransition", Lanes, Refused),
    ("RemoveTransition", Lanes, Refused),
    ("SetTransition", Lanes, Refused),
    ("SetTransitionAlignment", Lanes, Refused),
    ("SetTransitionState", Lanes, Refused),
    ("SplitClip", Refused, Lanes),
    ("TrimClip", Refused, Lanes),
    ("TrimClipUnlinked", Refused, Lanes),
    ("SlipClip", Refused, Lanes),
    ("CreateBin", Refused, Refused),
    ("RenameBin", Refused, Refused),
    ("RemoveBin", Refused, Refused),
    ("MoveBin", Refused, Refused),
    ("MoveMediaToBin", Refused, Refused),
    ("SetPoolItemPosition", Refused, Refused),
    ("CreateSubclip", Refused, Refused),
    ("CreateMulticamGroup", Refused, Refused),
    ("AddMulticamClip", Refused, Refused),
    ("SwitchMulticamAngle", Refused, Refused),
    ("SetMulticamSync", Refused, Refused),
    ("MoveClip", Lanes, Lanes),
    ("MoveClipUnlinked", Lanes, Lanes),
    ("RippleMoveClip", Every, Every),
    ("RemoveClip", Lanes, Lanes),
    ("RemoveTrack", Lanes, Lanes),
    ("MoveTrack", Lanes, Lanes),
    ("SetTrackName", Lanes, Lanes),
    ("SetTrackEnabled", Lanes, Lanes),
    ("SetTrackMuted", Lanes, Lanes),
    ("SetTrackSolo", Lanes, Lanes),
    ("SetTrackLocked", Lanes, Lanes),
    ("SetTrackDuckSource", Lanes, Lanes),
    ("SetTrackTargeted", Lanes, Lanes),
    ("SetTrackSyncLock", Lanes, Lanes),
    ("SetTrackDisplay", Lanes, Lanes),
    ("SetTrackGain", Refused, Lanes),
    ("SetTrackPan", Refused, Lanes),
    ("SetTrackAutomation", Refused, Lanes),
    ("SetTrackOutput", Refused, Refused),
    ("AddSend", Refused, Refused),
    ("SetSend", Refused, Refused),
    ("RemoveSend", Refused, Refused),
    ("AddAudioEffect", Refused, Refused),
    ("RemoveAudioEffect", Refused, Refused),
    ("MoveAudioEffect", Refused, Refused),
    ("SetAudioEffectParam", Refused, Refused),
    ("SetAudioEffectBypass", Refused, Refused),
    ("SetAudioEffectSidechain", Refused, Refused),
    ("RippleDelete", Every, Every),
    ("ShiftClips", Lanes, Lanes),
    ("RippleInsert", Refused, Lanes),
    ("OverwriteClip", Refused, Lanes),
    ("FitToFill", Refused, Lanes),
    ("LinkClips", Refused, Refused),
    ("UnlinkClips", Refused, Lanes),
    ("SetClipsEnabled", Lanes, Lanes),
    ("GroupClips", Refused, Refused),
    ("UngroupClips", Refused, Refused),
    ("SetClipBeats", Refused, Lanes),
    ("ClearBeats", Refused, Lanes),
    ("AddMarker", Lanes, Lanes),
    ("RemoveMarker", Refused, Refused),
    ("SetMarker", Refused, Refused),
    ("SetCanvas", Lanes, Lanes),
    ("SetSequenceRate", Lanes, Lanes),
    ("SetAudioLayout", Lanes, Lanes),
    ("SetTimeDisplay", Lanes, Lanes),
    ("SetMasterMix", Lanes, Lanes),
    ("SetProjectName", Lanes, Lanes),
    ("CreateNestedSequence", Refused, Every),
    ("PlaceNestedSequence", Refused, Refused),
    ("OpenSequence", Refused, Refused),
    ("CreateSequence", Every, Every),
    ("SetKeyframeEase", Refused, Refused),
    ("SetKeyframeRoving", Refused, Refused),
    ("SetSourcePatch", Refused, Refused),
    ("SourceEdit", Refused, Refused),
];

/// The clip, lane, and media every sample is pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Targets {
    clip: ClipId,
    track: TrackId,
    media: MediaId,
}

/// Which clip a census sample is pointed at.
#[derive(Debug, Clone, Copy)]
enum Aim {
    /// The left of two abutting text clips with a transition between them.
    Text,
    /// A media clip on an audio lane, linked to a picture on a video lane and a
    /// second sound on another audio lane, each at its own offset.
    Linked,
}

fn apply(engine: &mut Engine, command: EditCommand) -> ApplyOutcome {
    engine
        .apply(Command::Edit(command))
        .unwrap_or_else(|err| panic!("fixture edit refused: {err}"))
}

fn created(outcome: ApplyOutcome) -> ClipId {
    match outcome {
        ApplyOutcome::Edited(EditOutcome::Created(id)) => id,
        other => panic!("expected a new clip, got {other:?}"),
    }
}

fn lane(engine: &mut Engine, kind: TrackKind, name: &str) -> TrackId {
    match apply(
        engine,
        EditCommand::AddTrack {
            kind,
            name: name.to_string(),
            index: None,
            pinned: true,
        },
    ) {
        ApplyOutcome::Edited(EditOutcome::CreatedTrack(id)) => id,
        other => panic!("expected a new lane, got {other:?}"),
    }
}

fn text(engine: &mut Engine, track: TrackId, start: i64) -> ClipId {
    created(apply(
        engine,
        EditCommand::AddGenerated {
            track,
            generator: Generator::text("t"),
            timeline: TimeRange::at_rate(start, 24, RATE),
        },
    ))
}

/// Reset `engine` to the census fixture aimed at `aim`.
///
/// # Why
///
/// `Engine::with_project` calls `Renderer::new_headless`, which brings up a
/// Vulkan device (`66` OS threads on Lavapipe) and JIT-compiles 25 WGSL
/// pipelines (`~240 ms` wall per call). Reusing one `Engine` across the
/// 230 `(EditCommand, Aim)` cases and resetting its project and history on
/// each case keeps every projection and undo assertion identical without
/// constructing 230 headless GPU devices.
fn reset_fixture(engine: &mut Engine, aim: Aim) -> Targets {
    let mut project = Project::new("lanes", RATE);
    let media = project.add_media(MediaSource::new("take.mp4", 1920, 1080, RATE, 300, true));
    let mut place = |kind: TrackKind, name: &str, at: i64| {
        let track = project.add_track(kind, name);
        let clip = project
            .add_clip(
                track,
                media,
                TimeRange::at_rate(0, 60, RATE),
                RationalTime::new(at, RATE),
            )
            .expect("a clip on an empty lane");
        (track, clip)
    };
    let (_, picture) = place(TrackKind::Video, "V1", 0);
    let (sound_lane, sound) = place(TrackKind::Audio, "A1", 2);
    let (_, echo) = place(TrackKind::Audio, "A2", 5);
    engine.reset_project(project);
    apply(
        engine,
        EditCommand::LinkClips {
            clips: vec![picture, sound, echo],
        },
    );
    let titles = lane(engine, TrackKind::Text, "T1");
    let left = text(engine, titles, 0);
    text(engine, titles, 24);
    apply(
        engine,
        EditCommand::AddTransition {
            clip: left,
            transition_id: "dip_to_black".to_string(),
        },
    );
    let bystander = lane(engine, TrackKind::Text, "T2");
    text(engine, bystander, 0);
    let (clip, track) = match aim {
        Aim::Text => (left, titles),
        Aim::Linked => (sound, sound_lane),
    };
    Targets { clip, track, media }
}

fn fixture_engine(gpu: Option<&aria_compositor::GpuContext>) -> Engine {
    let config = EngineConfig { undo_limit: 64 };
    let project = Project::new("lanes", RATE);
    match gpu {
        Some(gpu) => Engine::with_renderer(
            config,
            project,
            aria_render::Renderer::with_gpu(gpu.share()),
        ),
        None => Engine::with_project(config, project).expect("an engine over the fixture"),
    }
}

fn fixture(aim: Aim) -> (Engine, Targets) {
    let ctx = aria_plugin::PluginContext::new();
    let gpu = crate::test_support::mounted_gpu(&ctx);
    let mut engine = fixture_engine(gpu.as_ref());
    let targets = reset_fixture(&mut engine, aim);
    (engine, targets)
}

/// Point a census sample at the fixture's clip, lane and media, by the names
/// every command gives those fields on the wire.
fn retarget(command: &EditCommand, fixture: &Targets) -> EditCommand {
    let (clip, track) = (fixture.clip, fixture.track);
    let mut wire = serde_json::to_value(command).expect("a command serializes");
    let fields = wire
        .as_object_mut()
        .expect("a command is an internally tagged wire object");
    for (name, value) in fields.iter_mut() {
        *value = match name.as_str() {
            "clip" => serde_json::to_value(clip),
            "clips" => serde_json::to_value([clip]),
            "track" | "to_track" => serde_json::to_value(track),
            "media" => serde_json::to_value(fixture.media),
            "target" => match value.as_object().and_then(|chain| chain.keys().next()) {
                Some(kind) if kind == "Clip" => serde_json::to_value(AudioInsertTarget::Clip(clip)),
                Some(kind) if kind == "Track" => {
                    serde_json::to_value(AudioInsertTarget::Track(track))
                }
                _ => continue,
            },
            _ => continue,
        }
        .expect("an id serializes");
    }
    serde_json::from_value(wire).expect("a retargeted command deserializes")
}

fn at() -> RationalTime {
    RationalTime::new(0, RATE)
}

/// The first lane where `kept` and `full` disagree, with the first clip that
/// differs, or the whole stores when every lane agrees and something else
/// does not.
fn difference(kept: &UiEditorStore, full: &UiEditorStore) -> Option<String> {
    if kept == full {
        return None;
    }
    for (a, b) in kept.sequence.tracks.iter().zip(&full.sequence.tracks) {
        if a == b {
            continue;
        }
        let clip = a
            .clips
            .iter()
            .zip(b.clips.iter())
            .find(|(x, y)| x != y)
            .map_or_else(
                || format!("{} clips kept, {} built", a.clips.len(), b.clips.len()),
                |(x, y)| format!("kept {x:?}\nbuilt {y:?}"),
            );
        return Some(format!("lane {}: {clip}", b.name));
    }
    Some(format!(
        "kept {:?}\nbuilt {:?}",
        kept.sequence.tracks.len(),
        full.sequence.tracks.len()
    ))
}

fn project(
    engine: &Engine,
    selection: &[ClipId],
    previous: Option<Previous<'_>>,
) -> (UiEditorStore, LaneBasis) {
    project_to_ui_from(
        engine.project(),
        at(),
        selection,
        false,
        false,
        false,
        previous,
    )
}

fn catalogs() -> [u64; 4] {
    [
        aria_models::effect_catalog().revision(),
        aria_models::transition_catalog().revision(),
        aria_models::clip_type_catalog().revision(),
        aria_models::audio_effect_catalog().revision(),
    ]
}

/// Run `sample` aimed by `aim`: the incremental store after the edit and after
/// its undo, each against a full projection. Returns the reach, or `None` when
/// another test moved a catalog mid-case and the reach cannot be read.
fn case(engine: &mut Engine, sample: &EditCommand, aim: Aim) -> Result<Option<Reach>, String> {
    let catalogs_before = catalogs();
    let targets = reset_fixture(engine, aim);
    let selection = [targets.clip];
    let (before, basis) = project(engine, &selection, None);
    let command = retarget(sample, &targets);
    let id = command.spec().id;
    let refused = engine.apply(Command::Edit(command)).is_err();
    let previous = Previous {
        store: &before,
        basis: &basis,
    };
    let (after, basis_after) = project(engine, &selection, Some(previous));
    let full = project_to_ui_with(engine.project(), at(), &selection, false, false, false);
    if let Some(wrong) = difference(&after, &full) {
        return Err(format!(
            "{id} aimed at {aim:?}: the kept store is stale\n{wrong}"
        ));
    }
    if crate::app::project_facts::ProjectFacts::of_with_basis(engine, Some(&basis_after)).mixer
        != crate::app::project_facts::ProjectFacts::of_with_basis(engine, None).mixer
    {
        return Err(format!(
            "{id} aimed at {aim:?}: the kept mixer facts are stale"
        ));
    }
    let kept = after
        .sequence
        .tracks
        .iter()
        .filter(|lane| {
            before
                .sequence
                .tracks
                .iter()
                .any(|old| old.id == lane.id && old.clips.shares(&lane.clips))
        })
        .count();
    if !refused && engine.undo() {
        let previous = Previous {
            store: &after,
            basis: &basis_after,
        };
        let (undone, basis_undone) = project(engine, &selection, Some(previous));
        let full = project_to_ui_with(engine.project(), at(), &selection, false, false, false);
        if let Some(wrong) = difference(&undone, &full) {
            return Err(format!(
                "{id} aimed at {aim:?}: the kept store is stale after its undo\n{wrong}"
            ));
        }
        if crate::app::project_facts::ProjectFacts::of_with_basis(engine, Some(&basis_undone)).mixer
            != crate::app::project_facts::ProjectFacts::of_with_basis(engine, None).mixer
        {
            return Err(format!(
                "{id} aimed at {aim:?}: the kept mixer facts are stale after its undo"
            ));
        }
    }
    if catalogs() != catalogs_before {
        return Ok(None);
    }
    Ok(Some(if refused {
        if kept != after.sequence.tracks.len() {
            return Err(format!(
                "{id} aimed at {aim:?} was refused and rebuilt a lane"
            ));
        }
        Refused
    } else if kept == 0 {
        Every
    } else {
        Lanes
    }))
}

/// Every edit, aimed at each clip, leaves an incremental store equal to a full
/// one, and re-projects as much as its [`REACH`] row records.
#[test]
fn every_edit_reprojects_what_a_full_projection_would() {
    let ctx = aria_plugin::PluginContext::new();
    let gpu = crate::test_support::mounted_gpu(&ctx);
    let mut fresh_engine = fixture_engine(gpu.as_ref());
    let fresh_targets = reset_fixture(&mut fresh_engine, Aim::Text);
    let mut engine = fixture_engine(gpu.as_ref());
    let _ = reset_fixture(&mut engine, Aim::Linked);
    let _ = engine.apply(Command::Edit(EditCommand::SetProjectName {
        name: "mutated".to_string(),
    }));
    let reset_targets = reset_fixture(&mut engine, Aim::Text);
    let shape = |eng: &Engine, targets: Targets| {
        let store = project_to_ui_with(eng.project(), at(), &[targets.clip], false, false, false);
        let tracks: Vec<_> = store
            .sequence
            .tracks
            .iter()
            .map(|track| {
                let clips: Vec<_> = track
                    .clips
                    .iter()
                    .map(|clip| {
                        (
                            clip.timeline_start_ticks,
                            clip.timeline_end_ticks,
                            clip.selected,
                            clip.transition.is_some(),
                        )
                    })
                    .collect();
                (track.name.clone(), track.kind, clips)
            })
            .collect();
        let history: Vec<_> = eng
            .history_entries()
            .into_iter()
            .map(|entry| (entry.label.into_owned(), entry.applied))
            .collect();
        (
            store.project_name.to_string(),
            tracks,
            history,
            eng.is_dirty(),
            eng.in_group(),
            eng.project_path().cloned(),
            eng.active_sequence(),
        )
    };
    assert_eq!(
        shape(&engine, reset_targets),
        shape(&fresh_engine, fresh_targets),
    );
    drop(fresh_engine);

    let mut failures = Vec::new();
    let mut missing = Vec::new();
    let census: Vec<EditCommand> = every_edit_command();
    for sample in &census {
        let id = sample.spec().id;
        let mut measured = [Refused; 2];
        for (slot, aim) in [Aim::Text, Aim::Linked].into_iter().enumerate() {
            // A catalog moved by a test running beside this one changes what
            // every lane reads; the case runs again rather than read that as
            // this edit's reach.
            let reach = (0..4).find_map(|_| case(&mut engine, sample, aim).transpose());
            match reach {
                Some(Ok(reach)) => measured[slot] = reach,
                Some(Err(why)) => failures.push(why),
                None => failures.push(format!("{id} aimed at {aim:?}: catalogs kept moving")),
            }
        }
        match REACH.iter().find(|(row, _, _)| *row == id) {
            Some(&(_, text, linked)) => {
                if [text, linked] != measured {
                    failures.push(format!(
                        "{id}: REACH records ({text:?}, {linked:?}), measured ({:?}, {:?})",
                        measured[0], measured[1]
                    ));
                }
            }
            None => missing.push(format!(
                "    (\"{id}\", {:?}, {:?}),",
                measured[0], measured[1]
            )),
        }
    }
    for (row, _, _) in REACH {
        if !census.iter().any(|sample| sample.spec().id == *row) {
            failures.push(format!("REACH holds {row}, which the census no longer has"));
        }
    }
    if !missing.is_empty() {
        failures.push(format!(
            "commands with no REACH row; record their reach:\n{}",
            missing.join("\n")
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Run `case` until no catalog moved while it ran: a catalog moved by a test
/// running beside this one changes what every lane reads, so every lane is
/// rebuilt. `None` when the catalogs kept moving.
fn steady<T>(mut case: impl FnMut() -> T) -> Option<T> {
    (0..4).find_map(|_| {
        let before = catalogs();
        let outcome = case();
        (catalogs() == before).then_some(outcome)
    })
}

/// A selection change rebuilds the lanes of the clips it toggled and keeps the
/// rest, takes every row of the lanes it rebuilds, and the kept store equals a
/// full projection.
#[test]
fn a_selection_change_rebuilds_the_lanes_it_toggles() {
    let (engine, targets) = fixture(Aim::Text);
    let project = engine.project();
    let selection = [targets.clip];
    let (before, after, built) = steady(|| {
        let (before, basis) = project_to_ui_from(project, at(), &[], false, false, false, None);
        let previous = Previous {
            store: &before,
            basis: &basis,
        };
        super::reset_rows_built();
        let (after, _) = project_to_ui_from(
            project,
            at(),
            &selection,
            false,
            false,
            false,
            Some(previous),
        );
        let built = super::rows_built();
        (before, after, built)
    })
    .expect("the catalogs kept moving");
    let full = project_to_ui_with(project, at(), &selection, false, false, false);
    if let Some(wrong) = difference(&after, &full) {
        panic!("a selection change left a lane stale\n{wrong}");
    }
    assert_eq!(built, 0, "a selection change takes every row");
    let lane = targets.track.raw();
    for (old, new) in before.sequence.tracks.iter().zip(&after.sequence.tracks) {
        assert_eq!(
            old.clips.shares(&new.clips),
            new.id != lane,
            "lane {}: only the lane holding the toggled clip is rebuilt",
            new.name
        );
    }
}

/// Three title lanes of twenty abutting clips each, and the clips of the
/// middle lane in timeline order.
fn rows_fixture() -> (Project, Vec<TrackId>, Vec<ClipId>) {
    let mut project = Project::new("rows", RATE);
    let mut lanes = Vec::new();
    let mut middle = Vec::new();
    for n in 0..3 {
        let lane = project.add_track(TrackKind::Text, format!("T{n}"));
        for at in 0..20 {
            let clip = project
                .add_generated(
                    lane,
                    Generator::text("t"),
                    TimeRange::at_rate(at * 10, 10, RATE),
                )
                .expect("a title on an open stretch of its lane");
            if n == 1 {
                middle.push(clip);
            }
        }
        lanes.push(lane);
    }
    (project, lanes, middle)
}

/// One write between two projections: to the project, aimed at the middle
/// lane and its clips, and to the store the first projection built.
type RowWrite = fn(&mut Project, &mut UiEditorStore, TrackId, &[ClipId]);

/// A rebuilt lane builds the rows whose clip, right neighbor, or lane moved,
/// and takes every other row from the previous store. The store still equals
/// a full projection, and the lanes nothing wrote to are kept whole.
///
/// WHY: an Inspector scrub writes one clip per pointer move, and building
/// every row of its lane again is a third of the projection on a long lane.
/// The class closed: a row taken although something it reads moved (each
/// input of `RowBasis` and `LaneHead` has a case), or taken from clips a
/// caller wrote to after the projection. Linked clips, which read their
/// partners, are covered by the census above.
#[test]
fn a_rebuilt_lane_builds_only_the_rows_that_moved() {
    fn rename(project: &mut Project, clip: ClipId) {
        project.clip_mut(clip).expect("a clip of the lane").name = Some("renamed".into());
    }
    let cases: [(&str, RowWrite, usize); 6] = [
        (
            "a rename builds the renamed row",
            |project, _, _, clips| rename(project, clips[5]),
            1,
        ),
        (
            "a later start builds the row and the neighbor it no longer abuts",
            |project, _, _, clips| {
                project
                    .clip_timeline_mut(clips[5])
                    .expect("a clip of the lane")
                    .timeline = TimeRange::at_rate(52, 8, RATE);
            },
            2,
        ),
        (
            "a removal builds the neighbor it leaves",
            |project, _, _, clips| {
                project.remove_clip(clips[5]).expect("a clip of the lane");
            },
            1,
        ),
        (
            "a lock builds every row of the lane",
            |project, _, lane, _| {
                project
                    .timeline_mut()
                    .track_mut(lane)
                    .expect("the middle lane")
                    .locked = true;
            },
            20,
        ),
        (
            "a transition builds every row of the lane",
            |project, _, _, clips| {
                project
                    .add_transition(clips[5], "dip_to_black")
                    .expect("an abutting pair");
            },
            20,
        ),
        (
            "clips the store wrote to since are not taken",
            |project, store, lane, clips| {
                let held = store
                    .sequence
                    .tracks
                    .iter_mut()
                    .find(|held| held.id == lane.raw())
                    .expect("the middle lane in the store");
                held.clips.to_mut()[3].name = "stale".into();
                rename(project, clips[5]);
            },
            20,
        ),
    ];
    for (what, write, expected) in cases {
        let (start, lanes, clips) = rows_fixture();
        let (before, project, after, built) = steady(|| {
            let (mut before, basis) =
                project_to_ui_from(&start, at(), &[], false, false, false, None);
            let mut project = start.clone();
            write(&mut project, &mut before, lanes[1], &clips);
            let previous = Previous {
                store: &before,
                basis: &basis,
            };
            super::reset_rows_built();
            let (after, _) =
                project_to_ui_from(&project, at(), &[], false, false, false, Some(previous));
            let built = super::rows_built();
            (before, project, after, built)
        })
        .expect("the catalogs kept moving");
        assert_eq!(built, expected, "{what}");
        let full = project_to_ui_with(&project, at(), &[], false, false, false);
        if let Some(wrong) = difference(&after, &full) {
            panic!("{what}: the store is stale\n{wrong}");
        }
        for (old, new) in before.sequence.tracks.iter().zip(&after.sequence.tracks) {
            assert_eq!(
                old.clips.shares(&new.clips),
                new.id != lanes[1].raw(),
                "{what}: lane {} is kept whole unless written to",
                new.name
            );
        }
    }
}
