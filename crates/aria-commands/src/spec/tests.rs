//! Contracts for the command registry.
//!
//! The class closed here is "a command row and the command itself drift apart".
//! The exhaustive variant-to-row match already refuses to compile when a variant
//! has no row, so what is left is whether the row it got says the right things:
//! that its id is the tag the wire actually uses, that its label is usable, that
//! it states what its undo restores, and that the target shape it was filed
//! under really is the shape of its payload.
//!
//! Both sides of every comparison are derived at run time — the table on one
//! side, serde's view of the census on the other — so no count is written down
//! anywhere and a new command cannot pass by being forgotten.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::*;
use crate::samples::{every_edit_command, every_project_command};

/// The wire object a command serialises to, which is what a shell and the agent
/// actually send. Used as the independent view of a command's payload.
fn wire(command: &impl serde::Serialize) -> BTreeMap<String, Value> {
    match serde_json::to_value(command).expect("a command serialises") {
        Value::Object(map) => map.into_iter().collect(),
        other => panic!("a command is a JSON object, got {other}"),
    }
}

/// The `"type"` tag of a serialised command.
fn tag(command: &impl serde::Serialize) -> String {
    let mut payload = wire(command);
    match payload.remove("type") {
        Some(Value::String(name)) => name,
        other => panic!("a command carries a string type tag, got {other:?}"),
    }
}

/// Every id in one of the tables, as a set.
fn ids<'a>(rows: impl Iterator<Item = &'a str>) -> BTreeSet<&'a str> {
    rows.collect()
}

#[test]
fn every_edit_row_id_is_the_variant_wire_tag() {
    for command in every_edit_command() {
        assert_eq!(
            command.spec().id,
            tag(&command),
            "row id and wire tag disagree for {}",
            command.label()
        );
    }
}

#[test]
fn every_project_row_id_is_the_variant_wire_tag() {
    for command in every_project_command() {
        assert_eq!(
            command.spec().id,
            tag(&command),
            "row id and wire tag disagree for {}",
            command.label()
        );
    }
}

/// The table and the census are two independent lists of the same set. Either
/// growing without the other is the failure this catches, and neither side is
/// written down as a number.
#[test]
fn the_edit_table_covers_exactly_the_census() {
    let tabled = ids(edit_specs().iter().map(|spec| spec.id));
    let names: BTreeSet<String> = every_edit_command().iter().map(|cmd| tag(cmd)).collect();
    let census = ids(names.iter().map(String::as_str));
    assert_eq!(tabled, census);
}

#[test]
fn the_project_table_covers_exactly_the_census() {
    let tabled = ids(project_specs().iter().map(|spec| spec.id));
    let names: BTreeSet<String> = every_project_command().iter().map(|cmd| tag(cmd)).collect();
    let census = ids(names.iter().map(String::as_str));
    assert_eq!(tabled, census);
}

#[test]
fn no_two_rows_share_an_id() {
    let mut seen = BTreeSet::new();
    for id in edit_specs()
        .iter()
        .map(|spec| spec.id)
        .chain(project_specs().iter().map(|spec| spec.id))
    {
        assert!(seen.insert(id), "{id} is declared twice");
    }
}

/// A label is the identity a history surface shows and the key `reach`'s own
/// tests group by, so two commands sharing one makes both unreadable.
#[test]
fn no_two_rows_share_a_label() {
    let mut seen = BTreeMap::new();
    for (id, label) in edit_specs()
        .iter()
        .map(|spec| (spec.id, spec.label))
        .chain(project_specs().iter().map(|spec| (spec.id, spec.label)))
    {
        assert!(!label.is_empty(), "{id} has no label");
        assert_eq!(label.trim(), label, "{id}'s label is padded: {label:?}");
        if let Some(other) = seen.insert(label, id) {
            panic!("{id} and {other} both call themselves {label:?}");
        }
    }
}

/// Every row says what stepping back over it puts back — including a row that
/// records no undo step, which has to say why rather than leaving a reader to
/// guess that the omission was deliberate.
#[test]
fn every_row_states_what_its_undo_does() {
    for (id, inverse) in edit_specs()
        .iter()
        .map(|spec| (spec.id, spec.inverse))
        .chain(project_specs().iter().map(|spec| (spec.id, spec.inverse)))
    {
        let reason = inverse.reason();
        assert!(
            reason.len() > 20,
            "{id} declares {inverse:?} with no usable statement of what it does"
        );
    }
}

/// Every timeline edit is undoable. The engine's own dispatch types enforce it —
/// an edit handler returns an inverse, not an option — so a row that claimed
/// otherwise would be describing a command the engine cannot express.
#[test]
fn every_edit_row_is_undoable() {
    for spec in edit_specs() {
        assert!(
            spec.inverse.is_undoable(),
            "{} declares {:?}, but an edit handler always returns an inverse",
            spec.id,
            spec.inverse
        );
    }
}

/// The lock exemption is a decision per row, and only unlocking and lane display
/// may be taken on a locked lane: a lock the operator cannot undo is a trap, and
/// how a lane is drawn is not an edit to its clips.
#[test]
fn only_the_lock_escape_hatches_ignore_a_locked_lane() {
    let exempt = ids(edit_specs()
        .iter()
        .filter(|spec| spec.ignores_track_lock)
        .map(|spec| spec.id));
    assert_eq!(
        exempt,
        BTreeSet::from(["SetTrackDisplay", "SetTrackLocked"])
    );
}

/// The id numbers a command names on the wire, by field.
fn named_ids(payload: &BTreeMap<String, Value>, field: &str) -> Vec<u64> {
    match payload.get(field) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Number(n)) => vec![n.as_u64().expect("an entity id is a whole number")],
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_u64().expect("an entity id is a whole number"))
            .collect(),
        Some(other) => panic!("{field} is not an entity id: {other}"),
    }
}

/// The id an insert command's `target` holds under `variant`: `{"Clip": id}`
/// names a clip, `{"Track": id}` a lane, and `"Master"` neither.
fn chain_ids(payload: &BTreeMap<String, Value>, variant: &str) -> Vec<u64> {
    match payload.get("target") {
        Some(Value::Object(chain)) => chain
            .get(variant)
            .map(|id| id.as_u64().expect("an entity id is a whole number"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The audio lane a source edit's `sound` lands on: `{"lane": id}` names a
/// lane, `"in_picture"` and `"off"` none.
fn sound_lane(payload: &BTreeMap<String, Value>) -> Vec<u64> {
    match payload.get("sound") {
        Some(Value::Object(placement)) => placement
            .get("lane")
            .map(|id| id.as_u64().expect("an entity id is a whole number"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn raw(ids: &[impl serde::Serialize]) -> Vec<u64> {
    ids.iter()
        .map(|id| {
            serde_json::to_value(id)
                .expect("an id serialises")
                .as_u64()
                .expect("an id is a whole number")
        })
        .collect()
}

/// A row is filed under the shape of its payload, and the reader it gets from
/// that group is the only thing that reads the payload. Filing a command in the
/// wrong group, or the reader's total-function fallback answering for a command
/// it does not cover, both show up as a clip or lane the command plainly names
/// going missing from its targets.
///
/// The independent side is serde: the top-level `clip`, `clips`, `track` and
/// `to_track` fields, the clip or lane an insert command's `target` holds, and
/// the audio lane a source edit's `sound` lands on, are exactly the ids a command addresses directly, which is what a lock
/// policy has to check.
#[test]
fn targets_name_every_clip_and_lane_the_payload_does() {
    for command in every_edit_command() {
        let payload = wire(&command);
        let targets = command.targets();
        let id = command.spec().id;

        let mut expected_clips = named_ids(&payload, "clip");
        expected_clips.extend(named_ids(&payload, "clips"));
        expected_clips.extend(chain_ids(&payload, "Clip"));
        let mut expected_tracks = named_ids(&payload, "track");
        expected_tracks.extend(named_ids(&payload, "to_track"));
        expected_tracks.extend(chain_ids(&payload, "Track"));
        expected_tracks.extend(sound_lane(&payload));

        assert_eq!(
            raw(&targets.clips),
            expected_clips,
            "{id} names clips {expected_clips:?} on the wire"
        );
        assert_eq!(
            raw(&targets.tracks),
            expected_tracks,
            "{id} names lanes {expected_tracks:?} on the wire"
        );
    }
}

/// Reading a row is a property of the variant, not of what it carries: two
/// samples of one variant answer identically.
#[test]
fn a_row_does_not_depend_on_the_payload() {
    let short = EditCommand::SetProjectName { name: "a".into() };
    let long = EditCommand::SetProjectName {
        name: "a much longer project name".into(),
    };
    assert!(std::ptr::eq(short.spec(), long.spec()));
    assert_eq!(short.label(), long.label());
    assert_eq!(short.targets(), long.targets());
}

#[test]
fn every_command_census_roundtrips_json() {
    for cmd in every_edit_command() {
        let json = serde_json::to_string(&cmd).expect("serialize edit command");
        let deserialized: EditCommand =
            serde_json::from_str(&json).expect("deserialize edit command");
        assert_eq!(cmd.spec().id, deserialized.spec().id);
        assert_eq!(cmd.label(), deserialized.label());
    }
    for cmd in every_project_command() {
        let json = serde_json::to_string(&cmd).expect("serialize project command");
        let deserialized: ProjectCommand =
            serde_json::from_str(&json).expect("deserialize project command");
        assert_eq!(cmd.spec().id, deserialized.spec().id);
        assert_eq!(cmd.label(), deserialized.label());
    }
}
