//! Model-facing tool descriptions and behavioral case groups.
//!
//! Command identity is declared once by `wire_tool_registry!` in the parent
//! module. That registry generates the enum, DTO bindings, schemas, groups,
//! decode, and reverse names. Each domain module here contains the prose shown
//! to the model and the positive/refusal case table for its registered names.

use schemars::JsonSchema;

/// One LLM tool: name, model-facing description, and a JSON Schema for its
/// arguments.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Owned so host/MCP tool catalogs discovered at runtime can share the
    /// same provider wire as the static edit vocabulary.
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

pub(super) fn spec<T: JsonSchema>(name: &'static str, description: &'static str) -> ToolSpec {
    // Subschemas are inlined (no `$defs` / `$ref`): small local models
    // routinely fail to follow reference indirection and then guess the
    // argument shape (e.g. passing a bare string where the tagged
    // `WireGenerator` object is required).
    let mut settings = schemars::generate::SchemaSettings::draft2020_12();
    settings.inline_subschemas = true;
    let parameters = serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("tool argument schemas are plain data and always serialize");
    ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    }
}

/// A corrective example appended to argument-decode rejections, for the
/// tools whose nested shapes models most often get wrong. The model reads
/// this and retries; without it, weak models tend to give up and ask the
/// user instead.
fn argument_hint(tool: &str) -> Option<&'static str> {
    match tool {
        "add_generated" | "set_generator" => Some(
            "'generator' must be a tagged object: {\"type\": \"text\", \"content\": \"Hello\"} \
             or {\"type\": \"solid\", \"rgba\": [0, 0, 0, 255]} \
             or {\"type\": \"shape\", \"shape\": \"ellipse\", \"rgba\": [255, 0, 0, 255]}",
        ),
        _ => None,
    }
}

/// Decode one row's arguments. Malformed arguments come back as a
/// model-readable message naming the tool and, where the nested shape is a
/// known stumbling block, an example of the right one.
pub(super) fn decode_args<T: serde::de::DeserializeOwned>(
    name: &'static str,
    arguments: &serde_json::Value,
) -> Result<T, String> {
    // `serde_json::from_value` consumes its input, so the form this replaces
    // deep-cloned the entire argument tree on every tool call. `&Value` is
    // itself a `Deserializer`, so only the leaves `T` actually owns get copied.
    <T as serde::Deserialize>::deserialize(arguments).map_err(|e: serde_json::Error| {
        let hint = argument_hint(name)
            .map(|h| format!(" ({h})"))
            .unwrap_or_default();
        format!("invalid arguments for {name}: {e}{hint}")
    })
}

/// The read-only tool: returns the current project summary + editor
/// context. Not a [`crate::wire::WireCommand`] — the agent loop answers it without
/// touching dispatch, so it carries no registry row.
pub fn describe_project_spec() -> ToolSpec {
    ToolSpec {
        name: "describe_project".into(),
        description: "Get the current state of the project: tracks, clips with ids and \
                      times in seconds, the media pool, and the user's selection and \
                      playhead. Call this whenever you are unsure about ids or timing. \
                      A project too long to describe at once is described around the \
                      playhead and says how many clips it left out; pass from_seconds \
                      and to_seconds to read a different stretch."
            .into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "from_seconds": {
                    "type": "number",
                    "description": "Start of the stretch of timeline to describe, in \
                                    seconds. Omit with to_seconds to describe the \
                                    stretch around the playhead.",
                },
                "to_seconds": {
                    "type": "number",
                    "description": "End of the stretch of timeline to describe, in \
                                    seconds. Omit to run to the end of the project.",
                },
            },
        }),
    }
}

/// One domain of the edit vocabulary.
pub struct ToolGroup {
    /// The group name, which matches the module containing its descriptions and cases.
    pub name: &'static str,
    /// The tool names this group contains, in registry order.
    pub tools: &'static [&'static str],
}

impl ToolGroup {
    /// The group's tool specs, cached from the registry DTOs.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .map(|name| {
                super::tool_spec(name)
                    .cloned()
                    .expect("a group name comes from the registry")
            })
            .collect()
    }
}

/// Keep model-facing prose beside each domain's behavioral cases. Command
/// identity and dispatch are generated by `wire_tool_registry!` in the parent.
macro_rules! tool_group {
    ($( $name:literal => $variant:ident ( $args:ty ), $desc:literal; )+) => {
        pub(super) const DESCRIPTIONS: &[(&str, &str)] = &[ $(($name, $desc)),+ ];
    };
}

/// Declare the description/case modules. The command registry supplies group
/// membership; this table only resolves the prose for a registered name.
macro_rules! registry {
    ($( $group:ident ),+ $(,)?) => {
        $( mod $group; )+

        pub(super) fn description(name: &str) -> Option<&'static str> {
            [$( $group::DESCRIPTIONS ),+]
                .into_iter()
                .flatten()
                .find_map(|(candidate, description)| (*candidate == name).then_some(*description))
        }
    };
}

registry!(
    tracks,
    clips,
    timeline,
    effects,
    transitions,
    animation,
    retime,
    audio,
    routing,
    look,
    markers,
    bake,
    project,
);

#[cfg(test)]
mod cases;
#[cfg(test)]
mod registry_tests;
