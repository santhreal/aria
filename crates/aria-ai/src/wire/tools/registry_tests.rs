//! Runtime invariants around the compile-time tool registry.
//!
//! `wire_tool_registry!` is the single declaration of command identity and
//! generates the enum, DTO binding, schema, group, decode, and reverse name.
//! These tests cover the data attached to those rows: unique names, complete
//! descriptions, provider schemas, and one positive/refusal case per row.

use std::collections::{BTreeSet, HashSet};

use super::super::{tool_groups, tool_names, tool_specs};
use super::cases::ToolCase;
use crate::wire::WireCommand;

/// Every group's case table. A group missing from this list leaves its rows
/// with no test case, which
/// [`every_registry_row_has_a_test_case`](every_registry_row_has_a_test_case)
/// reports.
const CASE_TABLE: &[(&str, &[ToolCase])] = &[
    ("tracks", super::tracks::CASES),
    ("clips", super::clips::CASES),
    ("timeline", super::timeline::CASES),
    ("effects", super::effects::CASES),
    ("transitions", super::transitions::CASES),
    ("animation", super::animation::CASES),
    ("retime", super::retime::CASES),
    ("audio", super::audio::CASES),
    ("routing", super::routing::CASES),
    ("look", super::look::CASES),
    ("markers", super::markers::CASES),
    ("bake", super::bake::CASES),
    ("project", super::project::CASES),
];

/// The `command` tag of every [`WireCommand`] variant, read from the schema
/// the derive generates. The tag is the snake_case of the variant name, which
/// is the tool name for every variant except the ones [`TAG_ALIASES`] names.
fn variant_census() -> BTreeSet<String> {
    fn walk(schema: &serde_json::Value, out: &mut BTreeSet<String>) {
        match schema {
            serde_json::Value::Object(map) => {
                if let Some(tag) = map.get("properties").and_then(|p| p.get("command")) {
                    let named = tag
                        .get("const")
                        .and_then(|c| c.as_str())
                        .or_else(|| tag.get("enum")?.as_array()?.first()?.as_str());
                    if let Some(name) = named {
                        out.insert(name.to_string());
                    }
                }
                for value in map.values() {
                    walk(value, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }

    let schema = serde_json::to_value(schemars::schema_for!(WireCommand))
        .expect("a generated schema is plain data");
    let mut census = BTreeSet::new();
    walk(&schema, &mut census);
    assert!(
        !census.is_empty(),
        "the variant census came out empty, so it proves nothing: {schema}"
    );
    census
}

/// Variants whose `command` tag is not the name of their tool.
///
/// `SetClipLayerStyles` serializes as `set_clip_layer_styles`, while the tool
/// a provider calls is `set_layer_styles`. Both spellings are on the wire: a
/// plan carries the tag, a tool call carries the name, so neither can change
/// without breaking one of the two. Every other variant spells both the same.
const TAG_ALIASES: &[(&str, &str)] = &[("set_clip_layer_styles", "set_layer_styles")];

/// The tool name a variant tag belongs to.
fn tool_name_of_tag(tag: &str) -> &str {
    TAG_ALIASES
        .iter()
        .find(|(from, _)| *from == tag)
        .map_or(tag, |(_, to)| *to)
}

fn registry_census() -> BTreeSet<String> {
    tool_names().map(str::to_string).collect()
}

/// An alias that names a tag no variant emits, or a tool no row declares, is
/// stale, and a tag that is also a tool name would hide a missing row.
#[test]
fn every_tag_alias_is_live() {
    let tags = variant_census();
    let registry = registry_census();
    for (tag, name) in TAG_ALIASES {
        assert!(
            tags.contains(*tag),
            "alias {tag:?} names no WireCommand variant tag"
        );
        assert!(
            registry.contains(*name),
            "alias {tag:?} points at {name:?}, which no registry row declares"
        );
        assert!(
            !registry.contains(*tag),
            "{tag:?} is a tool name of its own, so aliasing it hides a row"
        );
    }
}

#[test]
fn every_wire_command_has_a_registry_row() {
    let variants: BTreeSet<String> = variant_census()
        .iter()
        .map(|tag| tool_name_of_tag(tag).to_string())
        .collect();
    let registry = registry_census();

    let unregistered: Vec<&String> = variants.difference(&registry).collect();
    assert!(
        unregistered.is_empty(),
        "these WireCommand variants have no registry row, so no model can \
         call them: {unregistered:?}"
    );

    let phantom: Vec<&String> = registry.difference(&variants).collect();
    assert!(
        phantom.is_empty(),
        "these registry rows name no WireCommand variant: {phantom:?}"
    );
}

#[test]
fn every_registry_row_answers_its_own_name() {
    for name in tool_names() {
        // Empty arguments are wrong for almost every tool; what matters is
        // that dispatch recognises the name rather than reporting it unknown.
        let refusal = WireCommand::from_tool_call(name, serde_json::json!({}))
            .err()
            .unwrap_or_default();
        assert!(
            !refusal.contains("unknown tool"),
            "{name} is a registry row that dispatch does not answer: {refusal}"
        );
    }

    let unknown = WireCommand::from_tool_call("open_project", serde_json::json!({}))
        .expect_err("open_project is outside the closed vocabulary");
    assert!(unknown.contains("unknown tool 'open_project'"), "{unknown}");
}

#[test]
fn groups_and_rows_are_named_once() {
    let mut groups = HashSet::new();
    let mut tools = HashSet::new();
    for group in tool_groups() {
        assert!(
            groups.insert(group.name),
            "two groups are named {}",
            group.name
        );
        assert!(!group.tools.is_empty(), "group {} has no rows", group.name);
        for tool in group.tools {
            assert!(
                tools.insert(*tool),
                "{tool} is a row of more than one group, so dispatch is ambiguous"
            );
        }
    }
}

#[test]
fn tool_specs_follow_the_registry() {
    let specs = tool_specs();
    let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    assert_eq!(
        names,
        tool_names().collect::<Vec<_>>(),
        "the offered specs are not the registry's rows, in order"
    );

    for spec in &specs {
        assert!(
            !spec.description.is_empty(),
            "{} has no description",
            spec.name
        );
        assert_eq!(
            spec.parameters.get("type").and_then(|t| t.as_str()),
            Some("object"),
            "{} arguments are not an object schema",
            spec.name
        );
    }
}

#[test]
fn every_registry_row_has_a_test_case() {
    let tested: BTreeSet<&str> = CASE_TABLE
        .iter()
        .flat_map(|(_, cases)| cases.iter().map(|case| case.tool))
        .collect();
    let registry: BTreeSet<&str> = tool_names().collect();

    let untested: Vec<&&str> = registry.difference(&tested).collect();
    assert!(
        untested.is_empty(),
        "these registry rows ship with no test case: {untested:?}"
    );

    for (group, cases) in CASE_TABLE {
        assert!(
            tool_groups()
                .iter()
                .any(|candidate| candidate.name == *group),
            "{group} has cases but is not a registry group"
        );
        assert!(!cases.is_empty(), "group {group} has an empty case table");
    }
}
