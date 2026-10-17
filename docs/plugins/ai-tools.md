# AI tools

A plugin offers the assistant a tool: a name, a description, a JSON Schema for
its arguments, a tier, and a handler. The assistant lists every registered tool
beside its own and calls one as `plugin_<name>`. The handler runs on the
assistant's turn thread through the plugin's own view of the context, and
returns text and PNG images for the model.

```rust
use std::sync::atomic::{AtomicBool, Ordering};

use aria_plugin::{
    AiToolOutput, AiToolSpec, AiToolTier, PluginContext, PluginError, ProjectHost,
};

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_ai_tool(
        AiToolSpec {
            name: "count_markers".to_owned(),
            description: "Count the markers on the open sequence.".to_owned(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
            tier: AiToolTier::ReadOnly,
        },
        |ctx: &PluginContext,
         _arguments: &serde_json::Value,
         cancel: &AtomicBool|
         -> Result<AiToolOutput, String> {
            if cancel.load(Ordering::Relaxed) {
                return Err("stopped".to_owned());
            }
            let host = ctx.try_get::<ProjectHost>().ok_or("no project is open")?;
            let markers = host.snapshot().timeline().markers().len();
            Ok(AiToolOutput::text(format!("{markers} marker(s)")))
        },
    )?;
    Ok(())
}
```

The model sees the description followed by `(from the <plugin name> plugin)`.
An `Err` is returned to the model as written. The handler reaches only the
services the manifest declares under `uses`; the example declares
`"ProjectHost"`.

## Names and schemas

| Field | Rule |
| --- | --- |
| `name` | 1 to 56 bytes of lowercase ASCII letters, digits, and `_`, unique across every mounted plugin |
| `description` | Not blank |
| `parameters` | A JSON object with `"type": "object"`; `properties`, when present, is an object; every name in `required` is a key of `properties` |
| Size | The description plus the schema's JSON text is at most 16 KiB |

`register_ai_tool` returns `PluginError::AiToolRefused` with the reason when a
rule fails. A registration made during mount drops when the plugin unmounts,
and the tool leaves the assistant's next turn.

## Tiers

| Tier | Runs |
| --- | --- |
| `ReadOnly` | Without asking. Reads state and changes nothing. |
| `Workspace` | Without asking. Changes the project or the editor in a way an undo reverses. |
| `System` | After the operator allows the call, unless Settings > AI > Autonomy is Full. Deletes data, writes outside the project, or reaches another system. |

The host authorizes a call at the tier the plugin registers when the call is
made, not the tier listed when the turn began. A call runs at no more trust
than its authorization: a tool re-registered at a higher tier between the two
fails with `<call name> changed since it was allowed and now needs <tier>
trust; call it again`.

## Output

`AiToolOutput` holds `text` and `images`. Each `AiToolImage` is a PNG and a
label of a few words. An image that is not a PNG, or is over 2048 pixels
(`MAX_AI_TOOL_IMAGE_SIDE`) on a side, fails the call.

## Edits

A tool edits the project through `ProjectHost` (`docs/plugins/project.md`). The
assistant's turn holds one edit group on its thread, so an edit a tool applies
joins the turn's undo step, and rolling the turn back rolls the edit back.

## Failure

A handler that panics fails its plugin for the session: the plugin unmounts,
its tools leave the list, and the model receives `the tool panicked: <message>`.
Long work checks `cancel`, which is set when the turn stops.

## Listing

Settings > AI counts the plugin tools beside the app tools. The assistant's
tool list shows one group per plugin, `from the <plugin name> plugin`. The
Plugins list shows each tool as an `AI tool` contribution by its call name.

## WebAssembly plugins

A component registers a tool during `mount` with `register-ai-tool` and answers
calls in the `ai-tool-call` export:

```wit
enum ai-tool-tier { read-only, workspace, system }

record ai-tool-spec {
    name: string,
    description: string,
    parameters-json: string,
    tier: ai-tool-tier,
}

record ai-tool-image { label: string, png: list<u8> }

record ai-tool-output { text: string, images: list<ai-tool-image> }

import register-ai-tool: func(spec: ai-tool-spec) -> result<_, string>;

export ai-tool-call: func(name: string, arguments-json: string) -> result<ai-tool-output, string>;
```

`name` in `ai-tool-call` is the name the spec registered, without `plugin_`.
The call runs under the plugin's fuel and memory limits. A trap fails the
plugin.

## Native plugins

An `aria-sdk` plugin registers a tool with `Host::register_ai_tool` and answers
calls in `Plugin::ai_tool_call`:

```rust
use aria_sdk::{AiToolOutput, AiToolSpec, AiToolTier, Host, Plugin};

impl Plugin for Notes {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_ai_tool(&AiToolSpec {
            name: "note_count".to_owned(),
            description: "Count the stored notes.".to_owned(),
            parameters: serde_json::json!({ "type": "object" }),
            tier: AiToolTier::ReadOnly,
        })
    }

    fn ai_tool_call(
        &self,
        host: &Host,
        name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<AiToolOutput, String> {
        match name {
            "note_count" => Ok(AiToolOutput::text(host.storage_keys()?.len().to_string())),
            other => Err(format!("no tool {other}")),
        }
    }
}
```

Through the C ABI, `register_ai_tool` in `AriaHostV1` takes the spec as JSON,
with `tier` one of `"read-only"`, `"workspace"`, or `"system"`. The host calls
`ai_tool_call` in `AriaPluginV1` with the tool's name and its arguments as JSON
text, and reads the value as JSON:
`{"text": "...", "images": [{"label": "...", "png": [137, 80, ...]}]}`, with
`png` an array of byte values. A table whose `struct_size` ends before
`ai_tool_call` cannot register tools.
