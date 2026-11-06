//! The command registry against the engine that runs it.
//!
//! A command is a row in two tables: `aria-commands` describes it (label,
//! reach, target shape, which abutments it can break, what its undo restores)
//! and `aria-engine` applies it. The compiler already refuses a
//! variant with no row in either — both dispatchers are generated from the
//! tables and are the only exhaustive matches over the axis. What the compiler
//! cannot check is whether a row describes what its handler *does*, which is
//! the drift this file is for.
//!
//! Every case walks the census (`aria_commands::every_edit_command`), so a new
//! command is covered here the moment it exists. Nothing is counted and no
//! command is named: an expectation written as a list goes stale in silence,
//! which is the same as having no test.
//!
//! ## What each assertion would catch
//!
//! - **History depth.** A handler that mutates the session without returning an
//!   inverse, or a refused command that recorded one anyway.
//! - **The newest label.** The row's `label` is what the history panel shows.
//! - **The outcome's id.** The label comes from the command, so it cannot tell
//!   a misrouted handler apart. The outcome is the handler's own word for what
//!   it did: a row wired to the wrong one reports an entity of the wrong kind,
//!   or an id the session no longer holds.
//! - **No dead transition.** The engine snapshots and prunes junctions around
//!   an edit whose row says `junctions: breaks`. Downgrade one of those rows to
//!   `junctions: keeps` and a transition survives on an abutment whose clips no
//!   longer meet — which is what this checks, for every edit, not for the
//!   twelve someone remembered.
//! - **Every junction back on undo.** Each fixture holds a second, sync-locked
//!   lane with its own junction, which a ripple on the addressed lane shifts.
//!   A row that ripples but says `junctions: breaks` prunes that junction
//!   without capturing it first, and undo leaves the lane short of it.
//!
//! ## What it does not catch
//!
//! A retargeted census sample is rejected when its payload does not fit the
//! fixture (a media sample against the text fixture, which imports none; a
//! time outside the clip). Those cases only prove that a refusal changes
//! nothing, which is worth asserting but is not the row's behaviour. Undo
//! *correctness* per command lives in `inverse_edits.rs`; this file only
//! checks that an undo step exists when the row says it should.

use aria_commands::{
    Command, EditCommand, EditOutcome, Inverse, every_edit_command, every_project_command,
};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AudioInsertTarget, ClipId, Generator, MediaId, Project, Rational, TimeRange, TrackId,
    TrackKind, Transition,
};

/// The census builds its samples at this rate, so the fixture has to use it:
/// an edit whose time is on another timebase is refused before it reaches its
/// handler, and would prove nothing about the row.
const RATE: Rational = Rational::FPS_30;

fn engine_at_census_rate() -> Engine {
    Engine::with_project(
        EngineConfig { undo_limit: 64 },
        Project::new("registry", RATE),
    )
    .expect("engine")
}

/// The fixture's own ids a census sample is pointed at.
#[derive(Clone, Copy)]
struct Target {
    clip: ClipId,
    track: TrackId,
    /// The imported source, when the fixture holds one. Media ids come from a
    /// process-wide counter, so a census media id names nothing in a fixture
    /// and every placement sample would take the unknown-media refusal.
    media: Option<MediaId>,
}

/// A session holding a transition on a real abutment, plus the clip and the
/// lane every census sample is retargeted onto, and a second lane with its own
/// junction that rides a ripple on the first.
///
/// Text lanes take generated clips and support transitions, so the fixture
/// needs no media asset and no case can skip itself for want of one.
fn fixture() -> (Engine, Target) {
    let mut engine = engine_at_census_rate();
    let (track, clip) = lane_with_junction(&mut engine, "T1");
    // Sync lock is on for a new lane, so T2 rides every ripple on T1.
    let _rider = lane_with_junction(&mut engine, "T2");
    let target = Target {
        clip,
        track,
        media: None,
    };
    (engine, target)
}

/// A text lane holding `0..60` abutting `60..90`, with a transition at 60.
/// Returns the lane and its left clip.
fn lane_with_junction(engine: &mut Engine, name: &str) -> (TrackId, ClipId) {
    let track = match engine.apply(Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Text,
        name: name.to_string(),
        index: None,
        pinned: true,
    })) {
        Ok(ApplyOutcome::Edited(EditOutcome::CreatedTrack(id))) => id,
        other => panic!("expected a new lane, got {other:?}"),
    };
    let left = generated(engine, track, 0, 60);
    let _right = generated(engine, track, 60, 30);
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: left,
            transition_id: "dip_to_black".to_string(),
        }))
        .expect("a transition on an abutting pair");
    (track, left)
}

fn generated(engine: &mut Engine, track: TrackId, start: i64, duration: i64) -> ClipId {
    match engine.apply(Command::Edit(EditCommand::AddGenerated {
        track,
        generator: Generator::text("t"),
        timeline: TimeRange::at_rate(start, duration, RATE),
    })) {
        Ok(ApplyOutcome::Edited(EditOutcome::Created(id))) => id,
        other => panic!("expected a generated clip, got {other:?}"),
    }
}

/// A media-backed session holding a speed-ramped clip `0..60` abutting a right
/// neighbor `60..90` with a transition at tick 60, so media-only edits
/// (`SetClipSpeed`, `SetSpeedCurve`, `RemoveAttributes`, `SlipClip`) run
/// against a real junction instead of taking the generated-clip refusal path,
/// and placement edits put the pool's own source at the junction.
/// V2 holds its own junction at 60 and rides a ripple on V1.
fn media_fixture() -> (Engine, Target) {
    let mut project = Project::new("registry_media", RATE);
    let _m1 = project.add_media(aria_models::MediaSource::new(
        "/media/a.mp4",
        1920,
        1080,
        RATE,
        300,
        true,
    ));
    let m2 = project.add_media(aria_models::MediaSource::new(
        "/media/b.mp4",
        1920,
        1080,
        RATE,
        300,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let track2 = project.add_track(TrackKind::Video, "V2");
    let left = project
        .add_clip(
            track,
            m2,
            TimeRange::at_rate(15, 30, RATE),
            aria_models::RationalTime::new(0, RATE),
        )
        .expect("left clip");
    project
        .set_clip_speed_curve(left, Some(aria_models::Param::Constant(0.5)))
        .expect("half-speed curve stretches 30 source ticks to 0..60");
    let _right = project
        .add_clip(
            track,
            m2,
            TimeRange::at_rate(0, 30, RATE),
            aria_models::RationalTime::new(60, RATE),
        )
        .expect("right clip");
    project
        .add_transition(left, "dip_to_black")
        .expect("transition on media abutment");
    let rider = project
        .add_clip(
            track2,
            m2,
            TimeRange::at_rate(0, 60, RATE),
            aria_models::RationalTime::new(0, RATE),
        )
        .expect("rider left clip");
    project
        .add_clip(
            track2,
            m2,
            TimeRange::at_rate(60, 30, RATE),
            aria_models::RationalTime::new(60, RATE),
        )
        .expect("rider right clip");
    project
        .add_transition(rider, "dip_to_black")
        .expect("transition on the rider abutment");
    let engine = Engine::with_project(EngineConfig { undo_limit: 64 }, project).expect("engine");
    let target = Target {
        clip: left,
        track,
        media: Some(m2),
    };
    (engine, target)
}

/// Point a census sample at the fixture's real clip, lane and media.
///
/// The census fabricates ids so it can name every command without a session.
/// Dispatched as-is, every id-bearing command takes the unknown-entity path and
/// no row's behaviour is ever reached. The clip, lane and media fields are
/// named the same way on the wire by every command that has them, and an
/// insert chain's `target` holds its clip or lane under one key, so the rewrite
/// is one rule rather than a per-variant table: a new command is retargeted by
/// the same rule, with no row to add here.
fn retarget(command: &EditCommand, target: Target) -> EditCommand {
    let Target { clip, track, media } = target;
    let mut wire = serde_json::to_value(command).expect("a command serializes");
    let fields = wire
        .as_object_mut()
        .expect("a command is an internally tagged wire object");
    for (name, value) in fields.iter_mut() {
        *value = match name.as_str() {
            "clip" => serde_json::to_value(clip),
            "clips" => serde_json::to_value([clip]),
            "track" | "to_track" => serde_json::to_value(track),
            "media" => match media {
                Some(media) => serde_json::to_value(media),
                None => continue,
            },
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

/// Whether any transition sits on an abutment that no longer exists.
///
/// The check is the prune itself, run against a throwaway copy: if pruning a
/// copy would drop something, the session the engine left behind is holding a
/// junction whose clips no longer meet.
fn holds_a_dead_transition(project: &Project) -> bool {
    let mut probe = project.clone();
    probe.prune_dead_transitions()
}

/// Every lane's transitions, by lane id: what an undo has to put back.
fn junctions(project: &Project) -> Vec<(TrackId, Vec<Transition>)> {
    let mut lanes: Vec<_> = project
        .timeline()
        .tracks_ordered()
        .map(|track| (track.id, track.transitions().to_vec()))
        .collect();
    lanes.sort_unstable_by_key(|(id, _)| *id);
    lanes
}

fn edited(outcome: &ApplyOutcome, id: &str) -> EditOutcome {
    match outcome {
        ApplyOutcome::Edited(edited) => *edited,
        other => panic!("{id} is an edit but reported {other:?}"),
    }
}

/// The label of the newest applied step: what the history panel's top row says.
fn newest_label(engine: &Engine) -> String {
    engine
        .history_entries()
        .into_iter()
        .rfind(|entry| entry.applied)
        .map(|entry| entry.label.into_owned())
        .expect("an applied history step")
}

/// Whether the outcome's id resolves the way the outcome claims: a created or
/// updated entity is in the session, a removed one is gone.
fn names_the_entity_it_claims(engine: &Engine, outcome: EditOutcome) -> bool {
    let project = engine.project();
    match outcome {
        EditOutcome::Created(id) | EditOutcome::Updated(id) => project.clip(id).is_some(),
        EditOutcome::Removed(id) => project.clip(id).is_none(),
        EditOutcome::CreatedTrack(id)
        | EditOutcome::UpdatedTrack(id)
        | EditOutcome::ShiftedTrack(id) => project.timeline().track(id).is_some(),
        EditOutcome::RemovedTrack(id) => project.timeline().track(id).is_none(),
        EditOutcome::CreatedMarker(id) | EditOutcome::UpdatedMarker(id) => {
            project.timeline().marker(id).is_some()
        }
        EditOutcome::RemovedMarker(id) => project.timeline().marker(id).is_none(),
        EditOutcome::CreatedBin(id) => project.bin(id).is_some(),
        // Nothing to resolve: the change is a project-wide property, and the
        // pool's structure has no single id to point at.
        EditOutcome::UpdatedCanvas
        | EditOutcome::UpdatedProject
        | EditOutcome::UpdatedPool
        | EditOutcome::UpdatedSequenceRate
        | EditOutcome::UpdatedAudioLayout
        | EditOutcome::UpdatedTimeDisplay
        | EditOutcome::UpdatedMaster => true,
        // A patch names a channel, not an entity; the claim is that the lane
        // it names, if any, is still on the open sequence.
        EditOutcome::UpdatedSourcePatch => {
            let patching = project.timeline().source_patching();
            aria_commands::SourceChannel::ALL
                .iter()
                .all(|channel| match patching.get(*channel) {
                    aria_commands::Patch::Track(id) => project.timeline().track(id).is_some(),
                    aria_commands::Patch::Auto | aria_commands::Patch::Off => true,
                })
        }
        // Navigation: whichever timeline the command opened is the one the
        // session now edits, and the root is a legitimate destination, so the
        // claim is that the active sequence (if any) exists.
        EditOutcome::OpenedSequence => project
            .active_sequence()
            .is_none_or(|id| project.sequence(id).is_some()),
    }
}

/// Every edit, applied to a fixture that holds a transition, either does what
/// its row says or changes nothing at all.
///
/// One fresh fixture per command: a shared session would let an earlier edit
/// decide whether a later one is even reachable, and the order of the census is
/// not a contract.
#[test]
fn every_edit_does_what_its_row_says_or_nothing() {
    for sample in every_edit_command() {
        for (kind, (mut engine, target)) in [("text", fixture()), ("media", media_fixture())] {
            let command = retarget(&sample, target);
            // `spec` is `&'static`, so it outlives the command that dispatch eats.
            let spec = command.spec();
            let before = engine.history_depth();
            let junctions_before = junctions(engine.project());

            match engine.apply(Command::Edit(command)) {
                Ok(outcome) => {
                    assert_eq!(
                        engine.history_depth(),
                        before + 1,
                        "{} ({kind}) applied but recorded no undo step, though its row says undo {}",
                        spec.id,
                        spec.inverse.reason(),
                    );
                    assert_eq!(
                        newest_label(&engine),
                        spec.label,
                        "{} ({kind}) recorded a history step under another row's name",
                        spec.id,
                    );
                    let outcome = edited(&outcome, spec.id);
                    assert!(
                        names_the_entity_it_claims(&engine, outcome),
                        "{} ({kind}) reported {outcome:?}, which is not what the session holds: \
                         is it wired to another row's handler?",
                        spec.id,
                    );
                    assert!(
                        !holds_a_dead_transition(engine.project()),
                        "{} ({kind}) left a transition on a broken abutment; its row says \
                         junctions: {:?}",
                        spec.id,
                        spec.junctions,
                    );
                    assert!(
                        engine.undo(),
                        "{} ({kind}) recorded a history step that could not be undone",
                        spec.id,
                    );
                    assert!(
                        !holds_a_dead_transition(engine.project()),
                        "{} ({kind}) left a dead transition after undo",
                        spec.id,
                    );
                    assert_eq!(
                        junctions(engine.project()),
                        junctions_before,
                        "{} ({kind}) undo did not put back every lane's junctions; its row says \
                         junctions: {:?}",
                        spec.id,
                        spec.junctions,
                    );
                    assert!(
                        engine.redo(),
                        "{} ({kind}) undone step could not be redone",
                        spec.id,
                    );
                    assert!(
                        !holds_a_dead_transition(engine.project()),
                        "{} ({kind}) left a dead transition after redo",
                        spec.id,
                    );
                }
                Err(_) => {
                    assert_eq!(
                        engine.history_depth(),
                        before,
                        "{} ({kind}) was refused but left a step on the undo stack",
                        spec.id,
                    );
                }
            }
        }
    }
}

/// Every project action records an undo step exactly when its row says it does.
///
/// The census points at paths that do not exist, so the file actions are
/// refused; what is asserted of a refusal is that it changed nothing, which is
/// the same contract from the other side.
#[test]
fn every_project_action_records_the_undo_its_row_declares() {
    for sample in every_project_command() {
        let mut engine = engine_at_census_rate();
        let spec = sample.spec();
        let before = engine.history_depth();

        match engine.apply(Command::Project(sample)) {
            Ok(_) => {
                let recorded = engine.history_depth() > before;
                match spec.inverse {
                    Inverse::Restores(_) => assert!(
                        recorded,
                        "{} recorded no undo step, though its row says undo restores {}",
                        spec.id,
                        spec.inverse.reason(),
                    ),
                    Inverse::None(_) => assert!(
                        !recorded,
                        "{} recorded an undo step, though its row says {}",
                        spec.id,
                        spec.inverse.reason(),
                    ),
                    // The row states the case; which way it went here is not a
                    // contract, and `dispatch_project` debug-asserts the rest.
                    Inverse::Conditional(_) => {}
                }
            }
            Err(_) => assert_eq!(
                engine.history_depth(),
                before,
                "{} was refused but left a step on the undo stack",
                spec.id,
            ),
        }
    }
}

/// A session editing *inside* a sequence, with content left on the root.
///
/// Returns the engine, the clip, lane and media every sample is retargeted onto
/// (the clip and lane inside the open sequence), and the root timeline as it
/// stood before any sample ran.
///
/// The root content is a media clip on a video lane, because a nest is a
/// sequence clip and only a lane that takes media accepts one.
fn fixture_inside_a_sequence() -> (Engine, Target, serde_json::Value) {
    let mut project = Project::new("nested", RATE);
    let media = project.add_media(aria_models::MediaSource::new(
        "/media/nested.mp4",
        1920,
        1080,
        RATE,
        300,
        false,
    ));
    let video = project.add_track(TrackKind::Video, "V1");
    let root_clip = project
        .add_clip(
            video,
            media,
            TimeRange::at_rate(0, 24, RATE),
            aria_models::RationalTime::new(0, RATE),
        )
        .expect("place the clip to nest");
    let mut engine =
        Engine::with_project(EngineConfig { undo_limit: 64 }, project).expect("engine");
    engine
        .apply(Command::Edit(EditCommand::CreateNestedSequence {
            name: "Scene".to_string(),
            clips: vec![root_clip],
        }))
        .expect("nest a clip so the project holds a sequence");
    let sequence = engine
        .project()
        .sequences()
        .next()
        .expect("the nest created a sequence")
        .id;
    engine
        .apply(Command::Edit(EditCommand::OpenSequence {
            sequence: Some(sequence),
        }))
        .expect("open it for editing");

    let lane = match engine.apply(Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Text,
        name: "Nested T1".to_string(),
        index: None,
        pinned: true,
    })) {
        Ok(ApplyOutcome::Edited(EditOutcome::CreatedTrack(id))) => id,
        other => panic!("expected a new lane, got {other:?}"),
    };
    let clip = generated(&mut engine, lane, 0, 24);
    let root = serde_json::to_value(engine.project().root_timeline()).expect("root serializes");
    let target = Target {
        clip,
        track: lane,
        media: Some(media),
    };
    (engine, target, root)
}

/// A lane added while a sequence is open belongs to that sequence, and holds
/// the clips placed on it.
///
/// The defect this closes: lane creation addressed the root timeline while clip
/// placement addressed the open one, so adding a lane inside a sequence put it
/// on the root and every clip dropped on it was refused as an unknown track.
/// Half of nested-sequence editing was unreachable.
#[test]
fn a_lane_added_inside_a_sequence_holds_the_clips_placed_on_it() {
    let (
        engine,
        Target {
            clip, track: lane, ..
        },
        _root,
    ) = fixture_inside_a_sequence();
    let sequence = engine
        .active_sequence()
        .expect("the fixture leaves the sequence open");
    let timeline = &engine
        .project()
        .sequence(sequence)
        .expect("the open sequence")
        .timeline;

    assert!(
        timeline.track(lane).is_some(),
        "the lane landed somewhere other than the sequence being edited"
    );
    assert!(
        timeline.clip(clip).is_some(),
        "the clip is not on the lane it was placed on"
    );
    assert!(
        engine.project().root_timeline().track(lane).is_none(),
        "the lane was added to the root timeline as well"
    );
}

/// Every edit applied while a sequence is open leaves the root timeline alone.
///
/// Walks the census, so a command added to the surface is covered by existing.
/// The class: an edit that reaches for the project's root timeline instead of
/// the open one silently writes into a timeline that is not on screen and,
/// when its sibling reads the open one, fails the next edit outright.
/// Asserting the root is byte-identical catches both directions without naming
/// a single command.
#[test]
fn no_edit_touches_the_root_timeline_while_a_sequence_is_open() {
    for sample in every_edit_command() {
        let (mut engine, target, root_before) = fixture_inside_a_sequence();
        let command = retarget(&sample, target);
        let label = format!("{command:?}");
        let _ = engine.apply(Command::Edit(command));
        let root_after =
            serde_json::to_value(engine.project().root_timeline()).expect("root serializes");
        assert_eq!(
            root_before, root_after,
            "{label} changed the root timeline while a sequence was open"
        );
    }
}

#[path = "command_registry_fuzz.rs"]
mod fuzz;
