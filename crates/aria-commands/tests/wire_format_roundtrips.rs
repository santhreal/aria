//! Roundtrips over the whole command census.
//!
//! Every command goes out as JSON and comes back equal, and its `"type"` is the
//! tag its registry row declares. The census and the registry are also compared
//! as sets, so a command that exists but was never sampled fails here.
use super::*;

// --- roundtrips -------------------------------------------------------------

/// The census and the registry are two independently written lists of the same
/// commands, so comparing them catches either growing alone. Both sides are
/// derived here: no count is written down, because a written count only ever
/// records what someone believed on the day they typed it.
#[test]
fn the_census_and_the_registry_describe_the_same_commands() {
    let mut census: Vec<&str> = project_samples().iter().map(project_variant_name).collect();
    let mut tabled: Vec<&str> = aria_commands::project_specs()
        .iter()
        .map(|spec| spec.id)
        .collect();
    census.sort_unstable();
    tabled.sort_unstable();
    assert_eq!(census, tabled, "project commands");

    let mut census: Vec<&str> = edit_samples().iter().map(edit_variant_name).collect();
    let mut tabled: Vec<&str> = aria_commands::edit_specs()
        .iter()
        .map(|spec| spec.id)
        .collect();
    census.sort_unstable();
    tabled.sort_unstable();
    assert_eq!(census, tabled, "edit commands");
}

/// A sample per variant, not two samples of one: a duplicate would let a
/// variant drop out of the census while the length stayed put.
#[test]
fn the_census_holds_each_command_once() {
    for mut names in [
        project_samples()
            .iter()
            .map(project_variant_name)
            .collect::<Vec<_>>(),
        edit_samples().iter().map(edit_variant_name).collect(),
    ] {
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "the census repeats a command");
    }
}

#[test]
fn every_project_command_roundtrips_through_command() {
    for sample in project_samples() {
        let wrapped = Command::Project(sample.clone());
        let json = serde_json::to_string(&wrapped).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, wrapped, "roundtrip failed for {json}");

        // The wire tag is exactly the variant name.
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], project_variant_name(&sample));
    }
}

#[test]
fn every_edit_command_roundtrips_through_command() {
    for sample in edit_samples() {
        let wrapped = Command::Edit(sample.clone());
        let json = serde_json::to_string(&wrapped).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, wrapped, "roundtrip failed for {json}");

        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], edit_variant_name(&sample));
    }
}

#[test]
fn command_layer_is_flat_json() {
    // The untagged `Command` never adds a Project/Edit envelope.
    let json =
        serde_json::to_value(Command::Edit(EditCommand::RemoveClip { clip: clip(9) })).unwrap();
    assert_eq!(json, json!({"type": "RemoveClip", "clip": 9}));
}
