# Native plugins

The native tier (`Tier::Native`) loads a platform shared library at
`lib/<os>-<arch>.<so|dylib|dll>` across the stable C ABI defined in
`crates/aria-plugin-abi` and `crates/aria-plugin-abi/include/aria_plugin.h`.
Rust plugins build as a `cdylib` against `aria-sdk` and export
`aria_plugin_entry` with `aria_sdk::export!`.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
aria-sdk = { path = "../../crates/aria-sdk" }
serde_json = "1"
```

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_sdk::{
    Host, LogLevel, Panel, PanelEvent, PanelLocation, PanelSpec, Plugin, UiNode,
};

pub const NOTES_PANEL_ID: &str = "notes.panel";

struct NotesPanel {
    count: AtomicUsize,
}

impl Panel for NotesPanel {
    fn view(&self, _host: &Host) -> UiNode {
        let count = self.count.load(Ordering::Acquire);
        UiNode::column([
            UiNode::heading(format!("Notes ({count})")),
            UiNode::button("add", format!("Add note ({count})")),
        ])
    }

    fn event(&self, _host: &Host, event: &PanelEvent) -> Result<(), String> {
        match event {
            PanelEvent::Pressed { id } if id.as_ref() == "add" => {
                self.count.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            other => Err(format!("unexpected control event on {}", other.id())),
        }
    }
}

#[derive(Debug, Default)]
pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_command("notes.count", "Record seen command count")?;
        host.register_panel(
            PanelSpec::new(NOTES_PANEL_ID, "Notes", PanelLocation::Sidebar),
            NotesPanel {
                count: AtomicUsize::new(0),
            },
        )?;
        host.log(LogLevel::Info, "mounted example.notes");
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            "notes.count" => {
                host.storage_set("seen_commands", b"1")?;
                host.refresh_panel(NOTES_PANEL_ID)?;
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }
}

aria_sdk::export!(NotesPlugin);
```

See `examples/notes-plugin` for a complete native package that registers
`notes.count`, subscribes to `"resolve-dropped"` and `"command-proposed"`,
and renders a docked sidebar panel.

## Building and packaging

Scaffold, run, validate, and archive a native plugin with the `aria plugin`
CLI (see [Command line](../command-line.md#plugin) and [Packages](packages.md)):

```text
aria plugin new com.example.notes --tier native
aria plugin dev com.example.notes
aria plugin check com.example.notes
aria plugin pack com.example.notes
```

`aria plugin dev` and `aria plugin pack` build the `cdylib` and copy the
artifact to `lib/<os>-<arch>.<ext>` inside the package directory:

| Platform | Library path in package |
| --- | --- |
| Linux (`x86_64`) | `lib/linux-x86_64.so` |
| Linux (`aarch64`) | `lib/linux-aarch64.so` |
| macOS (`aarch64`) | `lib/macos-aarch64.dylib` |
| macOS (`x86_64`) | `lib/macos-x86_64.dylib` |
| Windows (`x86_64`) | `lib/windows-x86_64.dll` |
| Windows (`aarch64`) | `lib/windows-aarch64.dll` |

A single `.ariaplugin` archive may ship multiple files under `lib/` so one
archive installs across operating systems and architectures. `aria plugin check`
inspects every file in `lib/` to verify that its binary header (ELF, Mach-O, or
PE), CPU architecture, and exported `aria_plugin_entry` symbol match its
`<os>-<arch>.<ext>` filename.

Native plugins execute in-process without a sandbox. Installing a native
package, or updating a data or WASM package to `Tier::Native`, always displays
an unsandboxed-code confirmation prompt in the editor.

## `aria-sdk` host API

`aria_sdk::export!(PluginType)` requires `PluginType: Plugin + Default + Send +
Sync + 'static` (or pass an explicit initializer with
`aria_sdk::export!(PluginType, init_expr)`). During `Plugin::mount(&self, host:
&Host)` and inside trait or handler callbacks, `Host` methods forward through
the `AriaHostV1` function table:

| Group | `Host` calls | `Plugin` callbacks |
| --- | --- | --- |
| Lifecycle | `log(LogLevel, text)` | `mount(&self, host)`, `unmount(&self, host)` |
| Storage | `storage_get(key)`, `storage_set(key, bytes)`, `storage_delete(key)`, `storage_keys()` (backed by `<data dir>/plugin-data/<id>`) | |
| Settings | `setting_get(key)` (`Result<Option<Value>, String>`), `setting_get_json(key)` | |
| Commands | `register_command(id, title)` | `command_run(&self, host, id, args_json)` |
| Events | `subscribe(event_kind)`, `subscribe_with_priority(event_kind, priority)` ([kinds and payloads](events.md#external-plugins)) | `event(&self, host, kind, event_json) -> Result<EventResponse, String>` |
| Project & view | `project_snapshot()`, `project_snapshot_json()`, `editor_view()`, `editor_view_json()`, `clip(id)`, `clips_in_range(sequence, range)`, `clips_at(sequence, tick)`, `selected_clips()`, `markers_in_range(sequence, range)`, `in_out()`, `project_transact(label, commands)`, `project_transact_json(label, commands_json)`, `project_undo()` | |
| Interchange | `register_importer(&ImporterSpec)`, `register_exporter(&ExporterSpec)`, `register_encoder(EncoderSpec, impl Encoder)`, `register_encoder_spec(&EncoderSpec)` | `import(&self, host, importer_id, path)`, `export(&self, host, exporter_id, project_json, path)`, `encoder_open(&self, host, encoder_id, path, config)` |
| Audio effects | `register_audio_effect(&AudioEffectSpec)` | `audio_create(&self, host, effect_id, sample_rate) -> Result<Box<dyn AudioProcessor>, String>` |
| Panels | `register_panel(PanelSpec, impl Panel)`, `register_panel_spec(&PanelSpec)`, `refresh_panel(id)` | `panel_view(&self, host, panel_id)`, `panel_event(&self, host, panel_id, event)` |
| Viewer overlays | `register_viewer_overlay(id, title)`, `refresh_viewer_overlay(id)` | `overlay_scene(&self, host, id) -> OverlayScene`, `overlay_event(&self, host, id, event)` |
| Jobs | `start_job(label) -> Result<u64, String>` | `job_step(&self, host, job) -> Result<JobStep, String>`, `job_end(&self, host, job, cancelled)` |
| Decoders | `register_decoder(DecoderSpec, impl Decoder)` | `Decoder::probe`, `Decoder::open_video -> (VideoInfo, Box<dyn VideoStream>)`, `Decoder::open_audio -> Box<dyn AudioStream>` |
| AI tools | `register_ai_tool(&AiToolSpec)` | `ai_tool_call(&self, host, name, arguments) -> Result<AiToolOutput, String>` ([AI tools](ai-tools.md)) |
| Scopes | `register_scope(&ScopeSpec)` | `scope_measure(&self, host, id, &ScopeFrame, &mut ScopeImage) -> Result<String, String>` ([Scopes](scopes.md)) |
| Effects & transitions | `register_effect(&EffectSpec, wgsl)`, `register_effect_passes(&EffectSpec, &[&str])`, `register_effect_graph(&EffectSpec, &[GraphPass])`, `register_cpu_effect(EffectSpec, impl CpuEffect)`, `register_cpu_effect_spec(&EffectSpec)`, `register_transition(&TransitionSpec, wgsl)`, `register_generator(&EffectSpec, wgsl)`, `register_generator_passes(&EffectSpec, &[&str])`, `register_generator_graph(&EffectSpec, &[GraphPass])`, `register_cpu_generator(EffectSpec, impl CpuEffect)`, `register_cpu_generator_spec(&EffectSpec)`, `register_texture(id, width, height, &[u8])` | `cpu_effect_process(&self, host, id, &CpuFrameIn, &mut CpuFrameOut) -> Result<(), String>` over premultiplied `RGBA16F` (`&[u16]` / `&mut [u16]`, `aria_sdk::f16::{to_f32, from_f32}`) ([Effects](effects.md)) |
| Clip types | `register_clip_type(&ClipTypeSpec)`, `define_clip_type(&ClipTypeSpec, &ClipTypeSchema, &[GraphPass])`, `define_cpu_clip_type(&ClipTypeSpec, &ClipTypeSchema, impl CpuEffect)`, `define_cpu_clip_type_spec(&ClipTypeSpec, &ClipTypeSchema)` | `cpu_effect_process` called with the type's id for a CPU type ([Clip types](clip-types.md#defined-types)) |
| Drivers | `register_driver_language(DriverLanguageSpec, impl DriverLanguage)`, `register_driver_language_spec(&DriverLanguageSpec)` | `driver_evaluate(&self, host, id, expr, &DriverVars) -> Result<ParamValue, String>` ([Drivers](drivers.md)) |
| Presets, themes & workspaces | `register_preset(&PresetSpec)`, `register_preset_json(preset_json)`, `register_theme(id, toml)`, `register_workspace(&WorkspaceSpec)` | ([Presets](presets.md), [Workspaces](workspaces.md)) |

`aria_sdk::GraphPass`, `PassStage`, `PassInput`, and `OutputSize` mirror the
host's [effect graph](effects.md#effect-graphs) types, and `EffectSpec` sets the
traits with `with_bounds`, `with_reads_outside`, and `with_time_dependent`.
Across the C ABI, the `wgsl` argument of `register_effect` and
`register_generator` is WGSL source, a JSON array of WGSL strings (a chain of
fragment passes), or a JSON array of graph pass objects (`name`, `stage`,
`inputs`, `output`, `source`).

Across the C ABI, `register_clip_type` defines a type that draws itself when
the spec object holds a `schema` object: `fields` and `groups` (effect
parameter and group objects), `time_dependent`, and `render`, either `"cpu"`
(drawn by `cpu_effect_process` called with the type's id, which requires
`has_contributions()`) or `{"passes": [...]}` (graph pass objects). The
`generator` field is then empty or the type's id.

Declared contributions in `plugin.toml` (`[[effects]]`, `[[generators]]`,
`[[transitions]]`, `[[textures]]`, `[[menus]]`, `[[keys]]`, `[[settings]]`,
`[[workspaces]]`, `[[clip_types]]`,
`presets/*.ariapreset`, and `themes/*.toml`) are registered by `register_declared` before
`Plugin::mount` runs. If `[[commands]]` declares a command ID that
`Plugin::mount` did not register with `host.register_command`, the host binds
that ID to `Plugin::command_run` automatically after `mount` returns.

## C ABI tables and size negotiation

The C ABI (`aria-plugin-abi` and `include/aria_plugin.h`) avoids Rust compiler
vtable layouts and shared allocators. A library compiled with any `rustc`
version or C/C++ compiler exports:

```c
const AriaPluginV1 *aria_plugin_entry(const AriaHostV1 *host, uint32_t api_major);
```

Both `AriaHostV1` and `AriaPluginV1` start with `AriaTableHeader`
(`struct_size`, `api_major`, `reserved`) followed by `free_string` and
`free_bytes` release functions so whichever side allocates an `AriaOwnedStr` or
`AriaOwnedBytes` buffer frees it with its own allocator.

`validate_plugin_table` requires `api_major == ARIA_API_MAJOR` (`1`) and checks
`struct_size` against the cumulative v1 table prefixes:

| Prefix constant | `AriaHostV1` fields added | `AriaPluginV1` fields added | Capability predicate |
| --- | --- | --- | --- |
| `ARIA_HOST_V1_MIN_SIZE` / `ARIA_PLUGIN_V1_MIN_SIZE` | Core host calls (`log`, `storage_*`, `setting_get`, `register_command`, `subscribe`, `register_importer`, `register_exporter`, `register_audio_effect`, `project_*`, `editor_view`) | Core lifecycle and callbacks (`mount`, `unmount`, `command_run`, `event`, `import`, `export`, `audio_*`) | Minimum valid v1 table |
| `ARIA_HOST_V1_PANELS_SIZE` / `ARIA_PLUGIN_V1_PANELS_SIZE` | `register_panel`, `refresh_panel` | `panel_view`, `panel_event` | `has_panels()` |
| `ARIA_HOST_V1_ENCODERS_SIZE` / `ARIA_PLUGIN_V1_ENCODERS_SIZE` | `register_encoder` | `encoder_open`, `encoder_push_frame`, `encoder_push_audio`, `encoder_finish`, `encoder_drop` | `has_encoders()` |
| `ARIA_HOST_V1_OVERLAYS_SIZE` / `ARIA_PLUGIN_V1_OVERLAYS_SIZE` | `register_viewer_overlay`, `refresh_viewer_overlay` | `overlay_scene`, `overlay_event` | `has_overlays()` |
| `ARIA_HOST_V1_JOBS_SIZE` / `ARIA_PLUGIN_V1_JOBS_SIZE` | `start_job` | `job_step`, `job_end` | `has_jobs()` |
| `ARIA_HOST_V1_DECODERS_SIZE` / `ARIA_PLUGIN_V1_DECODERS_SIZE` | `register_decoder` | `decoder_probe`, `decoder_open_video`, `decoder_seek`, `decoder_next_frame`, `decoder_open_audio`, `decoder_read_audio`, `decoder_seek_audio`, `decoder_close` | `has_decoders()` |
| `ARIA_HOST_V1_AI_TOOLS_SIZE` / `ARIA_PLUGIN_V1_AI_TOOLS_SIZE` | `register_ai_tool` | `ai_tool_call` | `has_ai_tools()` |
| `ARIA_HOST_V1_SCOPES_SIZE` / `ARIA_PLUGIN_V1_SCOPES_SIZE` | `register_scope` | `scope_measure` | `has_scopes()` |
| `ARIA_HOST_V1_CONTRIBUTIONS_SIZE` / `sizeof(AriaPluginV1)` | `register_effect`, `register_cpu_effect`, `register_transition`, `register_generator`, `register_clip_type`, `register_driver_language`, `register_preset`, `register_theme`, `register_workspace`, `register_analyzer` | `cpu_effect_process`, `driver_evaluate` | `has_contributions()` |
| `ARIA_HOST_V1_QUERIES_SIZE` | `clip`, `clips_in_range`, `clips_at`, `selected_clips`, `markers_in_range`, `in_out` | | `has_project_queries()` |
| `sizeof(AriaHostV1)` | `register_texture` | | `has_textures()` |

When a plugin returns a prefix shorter than `sizeof(AriaPluginV1)` (for example
`ARIA_PLUGIN_V1_MIN_SIZE`, `ARIA_PLUGIN_V1_PANELS_SIZE`,
`ARIA_PLUGIN_V1_ENCODERS_SIZE`, `ARIA_PLUGIN_V1_OVERLAYS_SIZE`,
`ARIA_PLUGIN_V1_JOBS_SIZE`, `ARIA_PLUGIN_V1_DECODERS_SIZE`,
`ARIA_PLUGIN_V1_AI_TOOLS_SIZE`, or `ARIA_PLUGIN_V1_SCOPES_SIZE`), `AriaPluginV1::copy_from_prefix` copies the
supported prefix and fills trailing panel, encoder, viewer-overlay, job,
decoder, AI tool, scope, CPU effect, or driver slots with stubs that return an error, or
`ARIA_STATUS_UNSUPPORTED` for the decoder calls. Calling
`Host::register_panel`, `Host::register_encoder`,
`Host::register_viewer_overlay`, `Host::start_job`, `Host::register_decoder`,
`Host::register_ai_tool`, `Host::register_scope`, `Host::register_cpu_effect`,
`Host::register_driver_language`, or `Host::register_texture` against a table
whose `struct_size` is smaller than the corresponding prefix fails with a size
diagnostic.

## Panic containment and library unload

Every guest callback invoked across `AriaPluginV1` is wrapped in
`std::panic::catch_unwind` inside `aria-sdk` and converted to
`ARIA_STATUS_PANIC`, `ARIA_BYTES_PANIC`, or `ARIA_INSTANCE_PANIC`. When the
host receives a panic status from any callback:

1. The host records a `PluginFault` with `FaultKind::Panic` in `ctx.faults()`.
2. The plugin is marked failed for the remainder of the session (`is_failed() ==
   true`), preventing further callback entry.
3. All registrations held by the plugin drop immediately, removing its
   commands, panels, viewer overlays, scopes, importers, exporters, encoders,
   decoders, and audio effects from the active registries.

Manifest strings and registrations are stored in host-allocated memory
(`Cow::Owned`), so no `'static` slice borrows read-only data inside the mapped
shared object. `NativePluginState` stores `Option<libloading::Library>` as its
final struct field inside an `Arc` shared by the `NativePlugin` handle and
every registered callback closure and audio, encoder, or decoder session. When the editor
mounts a native package, it first copies `lib/<os>-<arch>.<ext>` into
`<data dir>/plugin-cache/` so rebuilding or updating the package on disk does
not collide with a mapped file. Once the plugin unmounts (or faults) and the
last `Plugin` and callback handles drop, the `Arc` reference count reaches zero
and the operating system unloads the shared library.
