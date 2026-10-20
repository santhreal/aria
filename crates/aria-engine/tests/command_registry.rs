//! The command registry against the engine that runs it.
//!
//! A command is a row in two tables: `aria-commands` describes it (label,
//! reach, target shape, whether it can break an abutment, what its undo
//! restores) and `aria-engine` applies it. The compiler already refuses a
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
//!
//! ## What it does not catch
//!
//! A retargeted census sample is rejected when its payload does not fit the
//! fixture (a media id that was never imported, a time outside the clip). Those
//! cases only prove that a refusal changes nothing, which is worth asserting
//! but is not the row's behaviour. Undo *correctness* per command lives in
//! `inverse_edits.rs`; this file only checks that an undo step exists when the
//! row says it should.

use aria_commands::{
    Command, EditCommand, EditOutcome, Inverse, every_edit_command, every_project_command,
};
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{
    AudioInsertTarget, ClipId, Generator, Project, Rational, TimeRange, TrackId, TrackKind,
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

/// A session holding a transition on a real abutment, plus the clip and the
/// lane every census sample is retargeted onto.
///
/// Text lanes take generated clips and support transitions, so the fixture
/// needs no media asset and no case can skip itself for want of one.
fn fixture() -> (Engine, ClipId, TrackId) {
    let mut engine = engine_at_census_rate();
    let track = match engine.apply(Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Text,
        name: "T1".to_string(),
        index: None,
        pinned: true,
    })) {
        Ok(ApplyOutcome::Edited(EditOutcome::CreatedTrack(id))) => id,
        other => panic!("expected a new lane, got {other:?}"),
    };
    let left = generated(&mut engine, track, 0);
    let _right = generated(&mut engine, track, 24);
    engine
        .apply(Command::Edit(EditCommand::AddTransition {
            clip: left,
            transition_id: "dip_to_black".to_string(),
        }))
        .expect("a transition on an abutting pair");
    assert!(
        engine.project().has_transitions(),
        "the fixture has to hold a junction for a structural edit to break"
    );
    (engine, left, track)
}

fn generated(engine: &mut Engine, track: TrackId, start: i64) -> ClipId {
    match engine.apply(Command::Edit(EditCommand::AddGenerated {
        track,
        generator: Generator::text("t"),
        timeline: TimeRange::at_rate(start, 24, RATE),
    })) {
        Ok(ApplyOutcome::Edited(EditOutcome::Created(id))) => id,
        other => panic!("expected a generated clip, got {other:?}"),
    }
}

/// Point a census sample at the fixture's real clip and lane.
///
/// The census fabricates ids so it can name every command without a session.
/// Dispatched as-is, every id-bearing command takes the unknown-entity path and
/// no row's behaviour is ever reached. The clip and lane fields are named the
/// same way on the wire by every command that has them, and an insert chain's
/// `target` holds its clip or lane under one key, so the rewrite is one rule
/// rather than a per-variant table: a new command is retargeted by the same
/// rule, with no row to add here.
fn retarget(command: &EditCommand, clip: ClipId, track: TrackId) -> EditCommand {
    let mut wire = serde_json::to_value(command).expect("a command serializes");
    let fields = wire
        .as_object_mut()
        .expect("a command is an internally tagged wire object");
    for (name, value) in fields.iter_mut() {
        *value = match name.as_str() {
            "clip" => serde_json::to_value(clip),
            "clips" => serde_json::to_value([clip]),
            "track" | "to_track" => serde_json::to_value(track),
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
        let (mut engine, clip, track) = fixture();
        let command = retarget(&sample, clip, track);
        // `spec` is `&'static`, so it outlives the command that dispatch eats.
        let spec = command.spec();
        let before = engine.history_depth();

        match engine.apply(Command::Edit(command)) {
            Ok(outcome) => {
                assert_eq!(
                    engine.history_depth(),
                    before + 1,
                    "{} applied but recorded no undo step, though its row says undo {}",
                    spec.id,
                    spec.inverse.reason(),
                );
                assert_eq!(
                    newest_label(&engine),
                    spec.label,
                    "{} recorded a history step under another row's name",
                    spec.id,
                );
                let outcome = edited(&outcome, spec.id);
                assert!(
                    names_the_entity_it_claims(&engine, outcome),
                    "{} reported {outcome:?}, which is not what the session holds: \
                     is it wired to another row's handler?",
                    spec.id,
                );
                assert!(
                    !holds_a_dead_transition(engine.project()),
                    "{} left a transition on a broken abutment; its row says \
                     junctions: {}",
                    spec.id,
                    if spec.structural { "breaks" } else { "keeps" },
                );
            }
            Err(_) => {
                assert_eq!(
                    engine.history_depth(),
                    before,
                    "{} was refused but left a step on the undo stack",
                    spec.id,
                );
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
/// Returns the engine, the clip and lane every sample is retargeted onto (both
/// inside the open sequence), and the root timeline as it stood before any
/// sample ran.
///
/// The root content is a media clip on a video lane, because a nest is a
/// sequence clip and only a lane that takes media accepts one.
fn fixture_inside_a_sequence() -> (Engine, ClipId, TrackId, serde_json::Value) {
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
    let clip = generated(&mut engine, lane, 0);
    let root = serde_json::to_value(engine.project().root_timeline()).expect("root serializes");
    (engine, clip, lane, root)
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
    let (engine, clip, lane, _root) = fixture_inside_a_sequence();
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
/// the one the operator has open silently writes into a timeline nobody is
/// looking at — and, when its sibling reads the open one, refuses the next edit
/// outright. Asserting the root is byte-identical catches both directions
/// without naming a single command.
#[test]
fn no_edit_touches_the_root_timeline_while_a_sequence_is_open() {
    for sample in every_edit_command() {
        let (mut engine, clip, lane, root_before) = fixture_inside_a_sequence();
        let command = retarget(&sample, clip, lane);
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
