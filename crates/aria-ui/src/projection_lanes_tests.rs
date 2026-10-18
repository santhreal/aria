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

/// The fixture and the clip and lane every sample is pointed at.
struct Fixture {
    engine: Engine,
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

fn fixture(aim: Aim) -> Fixture {
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
    let mut engine = Engine::with_project(EngineConfig { undo_limit: 64 }, project)
        .expect("an engine over the fixture");
    apply(
        &mut engine,
        EditCommand::LinkClips {
            clips: vec![picture, sound, echo],
        },
    );
    let titles = lane(&mut engine, TrackKind::Text, "T1");
    let left = text(&mut engine, titles, 0);
    text(&mut engine, titles, 24);
    apply(
        &mut engine,
        EditCommand::AddTransition {
            clip: left,
            transition_id: "dip_to_black".to_string(),
        },
    );
    let bystander = lane(&mut engine, TrackKind::Text, "T2");
    text(&mut engine, bystander, 0);
    let (clip, track) = match aim {
        Aim::Text => (left, titles),
        Aim::Linked => (sound, sound_lane),
    };
    Fixture {
        engine,
        clip,
        track,
        media,
    }
}

/// Point a census sample at the fixture's clip, lane and media, by the names
/// every command gives those fields on the wire.
fn retarget(command: &EditCommand, fixture: &Fixture) -> EditCommand {
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
fn case(sample: &EditCommand, aim: Aim) -> Result<Option<Reach>, String> {
    let catalogs_before = catalogs();
    let mut fixture = fixture(aim);
    let selection = [fixture.clip];
    let (before, basis) = project(&fixture.engine, &selection, None);
    let command = retarget(sample, &fixture);
    let id = command.spec().id;
    let refused = fixture.engine.apply(Command::Edit(command)).is_err();
    let previous = Previous {
        store: &before,
        basis: &basis,
    };
    let (after, basis_after) = project(&fixture.engine, &selection, Some(previous));
    let full = project_to_ui_with(
        fixture.engine.project(),
        at(),
        &selection,
        false,
        false,
        false,
    );
    if let Some(wrong) = difference(&after, &full) {
        return Err(format!(
            "{id} aimed at {aim:?}: the kept store is stale\n{wrong}"
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
    if !refused && fixture.engine.undo() {
        let previous = Previous {
            store: &after,
            basis: &basis_after,
        };
        let (undone, _) = project(&fixture.engine, &selection, Some(previous));
        let full = project_to_ui_with(
            fixture.engine.project(),
            at(),
            &selection,
            false,
            false,
            false,
        );
        if let Some(wrong) = difference(&undone, &full) {
            return Err(format!(
                "{id} aimed at {aim:?}: the kept store is stale after its undo\n{wrong}"
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
            let reach = (0..4).find_map(|_| case(sample, aim).transpose());
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

/// A selection change rebuilds the lanes of the clips it toggled and keeps the
/// rest, and the kept store equals a full projection.
#[test]
fn a_selection_change_rebuilds_the_lanes_it_toggles() {
    let fixture = fixture(Aim::Text);
    let project = fixture.engine.project();
    let (before, basis) = project_to_ui_from(project, at(), &[], false, false, false, None);
    let selection = [fixture.clip];
    let previous = Previous {
        store: &before,
        basis: &basis,
    };
    let (after, _) = project_to_ui_from(
        project,
        at(),
        &selection,
        false,
        false,
        false,
        Some(previous),
    );
    let full = project_to_ui_with(project, at(), &selection, false, false, false);
    if let Some(wrong) = difference(&after, &full) {
        panic!("a selection change left a lane stale\n{wrong}");
    }
    let lane = fixture.track.raw();
    for (old, new) in before.sequence.tracks.iter().zip(&after.sequence.tracks) {
        assert_eq!(
            old.clips.shares(&new.clips),
            new.id != lane,
            "lane {}: only the lane holding the toggled clip is rebuilt",
            new.name
        );
    }
}
