# WebAssembly Plugin Tier (`aria-plugin-wasm`)

## Synopsis

`aria-plugin-wasm` loads WebAssembly Component Model packages (`Tier::Wasm`) into `Box<dyn aria_plugin::Plugin>` instances backed by `wasmtime` without WASI. Every guest call runs under configurable instruction fuel, linear memory, and wall-clock epoch deadlines.

## Package Layout

A WebAssembly plugin package is a directory containing `plugin.toml` and `plugin.wasm`:

```text
my-plugin/
  plugin.toml
  plugin.wasm
  shaders/*.wgsl
  icons/*.svg
```

`plugin.toml` declares the package metadata, API major version (`api = 1`), permissions, and optional static contributions:

```toml
[plugin]
id = "com.example.wasm-tools"
name = "Example WASM Tools"
version = "0.1.0"
api = 1
publisher = "Aria Examples"
description = "Reference WebAssembly component plugin for Aria"
license = "Apache-2.0"

[permissions]
project = "read-write"
fs = ["pick"]

[[commands]]
id = "example.add-marker"
title = "Add Example Marker"
icon = "icons/marker.svg"
enablement = "!playing"
menu = "Sequence/Examples"

[[keys]]
command = "example.add-marker"
key = "Ctrl+Alt+G"

[[settings]]
key = "marker-label"
label = "Marker label"
default = "Example"
```

`[[commands]]` lists what the component registers with `register-command`.
A row's `icon` and `enablement` apply to the registration with the same id.
`[[menus]]`, `[[keys]]`, and `[[settings]]` are read by the host: see
[Commands](commands.md) and the sections after it. The
setting above is stored as `com.example.wasm-tools.marker-label`.

## WIT World (`aria:plugin/plugin@1.0.0`)

The WIT definition is at `crates/aria-plugin-wasm/wit/plugin.wit`, and
`aria_plugin_wasm::WORLD` holds its name.

Loading a component type-checks it against the world before anything runs. A
component that imports a function the host does not provide, or lacks an
export the world requires, fails to load with `does not match world
aria:plugin/plugin@1.0.0` and the mismatch. `aria plugin check` runs the same
load.

### Host Imports

| Import | Signature | Permission |
| --- | --- | --- |
| `log` | `func(level: log-level, text: string)`; kept in the plugin's log in Settings > Plugins ([Logging](basics.md#logging)) | None |
| `storage-get` | `func(key: string) -> result<option<list<u8>>, string>` | None (`<data_root>/plugin-data/<id>`) |
| `storage-set` | `func(key: string, value: list<u8>) -> result<_, string>` | None (`<data_root>/plugin-data/<id>`) |
| `storage-delete` | `func(key: string) -> result<_, string>` | None (`<data_root>/plugin-data/<id>`) |
| `storage-keys` | `func() -> result<list<string>, string>` | None (`<data_root>/plugin-data/<id>`) |
| `setting-get` | `func(key: string) -> result<option<string>, string>` | None |
| `register-command` | `func(id: string, title: string) -> result<_, string>` | `mount` only |
| `subscribe` | `func(event-kind: string) -> result<_, string>` (append `:<priority>` on `resolve-dropped` to set handler priority) | `mount` only |
| `register-importer` | `func(spec: importer-spec) -> result<_, string>` | `mount` only |
| `register-exporter` | `func(spec: exporter-spec) -> result<_, string>` | `mount` only |
| `register-audio-effect` | `func(spec: audio-effect-spec) -> result<_, string>` | `mount` only |
| `register-panel` | `func(spec: panel-spec) -> result<_, string>` | `mount` only |
| `refresh-panel` | `func(id: string) -> result<_, string>` | Own panels only |
| `register-viewer-overlay` | `func(spec: overlay-spec) -> result<_, string>` | `mount` only |
| `refresh-viewer-overlay` | `func(id: string) -> result<_, string>` | Own overlays only |
| `start-job` | `func(label: string) -> result<u64, string>`; runs the guest's `job-step` on a job thread ([Jobs](jobs.md)) | None |
| `register-decoder` | `func(spec: decoder-spec) -> result<_, string>` | `mount` only |
| `register-ai-tool` | `func(spec: ai-tool-spec) -> result<_, string>` ([AI tools](ai-tools.md)) | `mount` only |
| `register-scope` | `func(spec: scope-spec) -> result<_, string>` ([Scopes](scopes.md)) | `mount` only |
| `set-effect-category` | `func(id: string, category: string) -> result<_, string>` | `mount` only, own effects only |
| `register-effect` | `func(spec: effect-spec, passes: list<string>) -> result<_, string>` ([Effects](effects.md)) | `mount` only |
| `register-cpu-effect` | `func(spec: effect-spec) -> result<_, string>` ([Effects](effects.md)) | `mount` only |
| `register-transition` | `func(spec: transition-spec, wgsl: string) -> result<_, string>` ([Effects](effects.md)) | `mount` only |
| `register-generator` | `func(spec: effect-spec, passes: list<string>) -> result<_, string>` ([Effects](effects.md)) | `mount` only |
| `register-effect-graph` | `func(spec: effect-spec, traits: effect-traits, passes: list<graph-pass>) -> result<_, string>`; a generator when `spec.generator` is set ([Effect graphs](effects.md#effect-graphs)) | `mount` only |
| `register-texture` | `func(id: string, width: u32, height: u32, pixels: list<u8>) -> result<_, string>`; straight-alpha RGBA8, top row first ([Plugin textures](effects.md#plugin-textures)) | `mount` only |
| `register-clip-type` | `func(spec: clip-type-spec) -> result<_, string>` ([Clip types](clip-types.md)) | `mount` only |
| `define-clip-type` | `func(spec: clip-type-spec, schema: clip-type-schema, render: clip-type-render) -> result<_, string>`; `clip-type-schema` holds `fields`, `groups`, and `time-dependent`, and `clip-type-render` is `cpu` (drawn by `cpu-effect-process` called with the type's id) or `passes(list<graph-pass>)` ([Defined types](clip-types.md#defined-types)) | `mount` only |
| `register-driver-language` | `func(spec: driver-language-spec) -> result<_, string>` ([Drivers](drivers.md)) | `mount` only |
| `register-preset` | `func(preset-json: string) -> result<_, string>` ([Presets](presets.md)) | `mount` only |
| `register-theme` | `func(id: string, toml: string) -> result<_, string>` | `mount` only |
| `register-workspace` | `func(spec: workspace-spec) -> result<_, string>` ([Workspaces](workspaces.md)) | `mount` only |
| `project-snapshot` | `func() -> result<string, string>` | `project = "read"` or `"read-write"` |
| `editor-view` | `func() -> result<string, string>` | `project = "read"` or `"read-write"` |
| `clip` | `func(id: u64) -> result<option<clip-ref>, string>` | `project = "read"` or `"read-write"` |
| `clips-in-range` | `func(sequence: option<u64>, range: time-range) -> result<list<clip-ref>, string>` | `project = "read"` or `"read-write"` |
| `clips-at` | `func(sequence: option<u64>, tick: rational-time) -> result<list<clip-ref>, string>` | `project = "read"` or `"read-write"` |
| `selected-clips` | `func() -> result<list<clip-ref>, string>` | `project = "read"` or `"read-write"` |
| `markers-in-range` | `func(sequence: option<u64>, range: time-range) -> result<list<marker>, string>` | `project = "read"` or `"read-write"` |
| `in-out` | `func() -> result<in-out-marks, string>` | `project = "read"` or `"read-write"` |
| `project-transact` | `func(label: string, commands-json: string) -> result<string, string>` | `project = "read-write"` |
| `project-undo` | `func() -> result<bool, string>` | `project = "read-write"` |
| `fs-read` | `func(path: string) -> result<list<u8>, string>` | Active import or decode path, or `permissions.fs` |
| `fs-read-at` | `func(path: string, offset: u64, len: u32) -> result<list<u8>, string>`; up to `len` bytes from `offset`, fewer only at end of file, at most 64 MiB per call | Active import or decode path, or `permissions.fs` |
| `fs-len` | `func(path: string) -> result<u64, string>` | Active import or decode path, or `permissions.fs` |
| `fs-write` | `func(path: string, bytes: list<u8>) -> result<_, string>` | Active export path or `permissions.fs` |

### Guest Exports

| Export | Signature | Description |
| --- | --- | --- |
| `mount` | `func() -> result<_, string>` | Initializes the plugin and registers contributions. |
| `unmount` | `func() -> result<_, string>` | Releases guest state before unload or reload. |
| `command-run` | `func(id: string, args-json: string) -> result<_, string>` | Executes a registered palette command. |
| `event` | `func(kind: string, event-json: string) -> result<option<string>, string>` | Receives subscribed notifications, waterfall proposals, and serial claims. |
| `%import` | `func(importer-id: string, path: string) -> result<import-result, string>` | Parses `path` and returns `Vec<aria_commands::Command>` JSON plus skipped items. |
| `%export` | `func(exporter-id: string, project-json: string, path: string) -> result<report, string>` | Serializes `project-json` to `path` and returns skipped items. |
| `audio-create` | `func(effect-id: string, sample-rate: u32) -> result<instance, string>` | Allocates an audio DSP instance at `sample-rate`. |
| `audio-process` | `func(instance: instance, params: list<audio-param>, block: audio-block) -> result<stereo-buffers, string>` | Processes one planar stereo block. |
| `audio-reset` | `func(instance: instance) -> result<_, string>` | Resets DSP state after a transport seek. |
| `audio-drop` | `func(instance: instance) -> result<_, string>` | Releases an audio DSP instance. |
| `panel-view` | `func(id: string) -> list<ui-node>` | Builds the flattened `ui-node` tree (`root = index 0`) for panel `id`. |
| `panel-event` | `func(id: string, event: panel-event) -> result<_, string>` | Handles a user control interaction on panel `id`. |
| `overlay-scene` | `func(id: string) -> list<overlay-shape>` | Builds the vector shape list for viewer overlay `id`. |
| `overlay-event` | `func(id: string, event: overlay-event) -> result<_, string>` | Handles a handle drag interaction on viewer overlay `id`. |
| `job-step` | `func(job: u64) -> result<job-step, string>` | Runs one step of job `job` and returns `job-step { progress, detail, done, commit }`. |
| `job-end` | `func(job: u64, cancelled: bool)` | Releases the state of job `job` after it finishes, fails, or is cancelled. |
| `decoder-probe` | `func(decoder-id: string, path: string) -> result<media-probe, decode-error>` | Describes `path` without decoding it. |
| `decoder-open-video` | `func(decoder-id: string, path: string) -> result<opened-video, decode-error>` | Opens the video stream of `path`: an instance and its `video-info`. |
| `decoder-seek` | `func(instance: instance, pts: s64) -> result<_, string>` | Positions the stream at the sync frame at or before `pts`. |
| `decoder-next-frame` | `func(instance: instance) -> result<option<decoded-frame>, string>` | Returns the next frame, or `none` at end of stream. |
| `decoder-open-audio` | `func(decoder-id: string, path: string, sample-rate: u32, channels: u16) -> result<instance, decode-error>` | Opens the audio of `path`, resampled to `sample-rate` and `channels`. |
| `decoder-read-audio` | `func(instance: instance, max-samples: u32) -> result<list<f32>, string>` | Returns up to `max-samples` interleaved samples. |
| `decoder-seek-audio` | `func(instance: instance, frame: s64) -> result<_, string>` | Positions the next read at sample frame `frame`. |
| `decoder-close` | `func(instance: instance)` | Releases a video or audio stream. |
| `ai-tool-call` | `func(name: string, arguments-json: string) -> result<ai-tool-output, string>` | Runs the AI tool registered as `name` on the assistant's turn thread and returns its text and PNG images. |
| `scope-measure` | `func(id: string, frame: scope-frame) -> result<scope-drawing, string>` | Measures the Viewer frame on the preview worker and returns the scope's image and readout ([Scopes](scopes.md)). |
| `cpu-effect-process` | `func(id: string, params: list<effect-param>, frame: video-frame) -> result<video-frame, string>` | Renders one premultiplied `RGBA16F` frame (`pixels: list<u16>`, IEEE-754 binary16 bit patterns, row-major, no row padding) for CPU effect or generator `id` ([Effects](effects.md)). |
| `driver-evaluate` | `func(id: string, expr: string, vars: driver-vars) -> result<param-value, string>` | Evaluates driver expression `expr` for driver language `id` ([Drivers](drivers.md)). |

For notification events, `event` returns `ok(none)`. For `command-proposed`, returning `some("{\"veto\":\"reason\"}")` refuses the command and returning a serialized `aria_commands::Command` JSON object rewrites it in place. For `resolve-dropped` (or `resolve-dropped-path`), returning `some("\"open-project\"")`, `some("\"import-media\"")`, `some("\"apply-lut\"")`, `some("\"install-skill\"")`, `some("\"import-timeline\"")`, or `some("\"handled\"")` claims the dropped path or URL.

### Declarative UI Panels

Because WIT forbids recursive types, `panel-view` returns a flattened `list<ui-node>` rooted at index `0`. Container variants (`column(list<u32>)` and `row(list<u32>)`) store child indices into that list, and `image(ui-image)` stores raw RGBA8 pixel bytes as `list<u8>`. Before constructing `aria_plugin::UiNode`, the host validates the flat list and rejects an empty list, an out-of-range child index, a cycle, a shared child index, or an unreachable non-root node with an error naming the offending node index, then runs `UiNode::check`.

### Viewer Overlays

A guest registers a viewer overlay during `mount` with `register-viewer-overlay(overlay-spec { id, title })` and returns its shapes from `overlay-scene(id) -> list<overlay-shape>`. Each `overlay-shape` variant (`line(overlay-line)`, `rect(overlay-rect)`, `ellipse(overlay-ellipse)`, `path(overlay-path)`, `text(overlay-text)`, `handle(overlay-handle)`) uses `point { x: f32, y: f32 }` coordinates in sequence frame pixels, `overlay-color` (`accent`, `text`, `muted`, `success`, `warning`, `danger`, or `rgba(tuple<u8, u8, u8, u8>)`), and `handle-look` (`dot`, `square`, `cross`). Pointer drags on a handle arrive through `overlay-event(id, overlay-event::drag(overlay-drag { handle, phase, at }))` with `drag-phase` (`start`, `move`, `end`), and out-of-band state changes schedule a scene rebuild with `refresh-viewer-overlay(id)`. The host converts the returned shape list into `aria_plugin::OverlayScene` and runs `OverlayScene::check`.

### Media Encoders

A WebAssembly plugin contributes an output format to the media Export dialog by calling `register-encoder` during `mount` and implementing the five `encoder-*` guest exports in `aria:plugin/plugin@1.0.0`:

| Import / Export | Signature | Description |
| --- | --- | --- |
| `import register-encoder` | `func(spec: encoder-spec) -> result<_, string>` | Registers an output format (`id`, `name`, `extension`, `audio`) during `mount`. |
| `export encoder-open` | `func(encoder-id: string, path: string, config: encoder-config) -> result<instance, string>` | Opens an encoding session writing to `path` and returns a guest instance handle. |
| `export encoder-push-frame` | `func(instance: instance, frame: encoder-frame) -> result<_, string>` | Encodes one straight-alpha RGBA8 frame (`rgba`, `width`, `height`, `stride`, `pts-value`, `pts-rate`). |
| `export encoder-push-audio` | `func(instance: instance, audio: encoder-audio) -> result<_, string>` | Encodes one interleaved `f32` audio block (`samples`, `sample-rate`, `channels`, `pts-value`, `pts-rate`). |
| `export encoder-finish` | `func(instance: instance) -> result<_, string>` | Finalizes the container and flushes output to `path`. |
| `export encoder-drop` | `func(instance: instance) -> result<_, string>` | Releases guest state when an export is cancelled or fails before `encoder-finish`. |

During `encoder-open`, `encoder-push-frame`, `encoder-push-audio`, and `encoder-finish`, the host permits `fs-write` to the active staging `path` passed to `encoder-open` without requiring `permissions.fs` in `plugin.toml`. Writing to any other path is denied and records a `CapabilityDenial` unless covered by `permissions.fs`. Every encoder call runs under `Limits::fuel_per_call`, `Limits::memory_bytes`, and `Limits::call_deadline`; a guest trap, fuel exhaustion, or deadline interruption fails the plugin for the session, drops its registrations, and discards the staged file.

### Media Decoders

A guest reads a media format by calling `register-decoder(decoder-spec { id, name, extensions, priority })` during `mount` and implementing the `decoder-*` exports. Import, the Viewer, export, thumbnails, and analysis open a file whose extension the spec lists through the guest, on the terms of [Decoders](interchange.md#decoders). A guest that registers no decoder returns `decode-error::unsupported` from `decoder-probe`, `decoder-open-video`, and `decoder-open-audio`.

`video-info` states the frame size (even, at most 16384), `frame-format` (`nv12` or `yuv420p`), `frame-color` (`bt709`, `bt601`, `bt2020-pq`, or `srgb`), the frame rate, and the time base: every pts counts ticks at `time-base-num / time-base-den` ticks per second. `decoder-next-frame` returns each plane in plane order with its rows packed at the plane's row width: `width` bytes over `height` rows for Y, `width` bytes over `height / 2` rows for NV12's UV plane, and `width / 2` bytes over `height / 2` rows for each YUV 4:2:0 chroma plane. A frame with a missing plane or a plane of any other length fails the decode.

During every `decoder-*` call the host permits `fs-read`, `fs-read-at`, and `fs-len` on the file being decoded without `permissions.fs`. `decode-error::unsupported` passes the file to the next decoder. A trap, fuel exhaustion, or deadline interruption fails the plugin for the session and drops its decoders. A decoder call made from inside one of the plugin's own calls fails without entering the guest.

The example plugin (`examples/wasm-plugin/src/y4m.rs`) reads 8-bit 4:2:0 YUV4MPEG2 files.

### AI Tools

A guest offers the assistant a tool by calling `register-ai-tool(ai-tool-spec { name, description, parameters-json, tier })` during `mount` and answering in `ai-tool-call`. The host lists the tool as `plugin_<name>` on the terms of [AI tools](ai-tools.md). Each call runs under `Limits::fuel_per_call`, `Limits::memory_bytes`, and `Limits::call_deadline`; a trap fails the plugin and takes its tools off the list. A tool call made from inside one of the plugin's own calls fails without entering the guest. The example plugin offers `example_add_marker` (Workspace) and `example_swatch` (ReadOnly).

## Host Loader and Resource Limits

```rust
use std::time::Duration;
use aria_plugin::PluginContext;
use aria_plugin::package::Package;
use aria_plugin_wasm::{Limits, Reloader, load_with_data_root};

let package = Package::open(pkg_dir)?;
let limits = Limits {
    fuel_per_call: 500_000_000,
    memory_bytes: 64 * 1024 * 1024,
    call_deadline: Duration::from_secs(2),
};
let plugin = load_with_data_root(&package, &limits, data_root)?;
ctx.mount(plugin.as_ref())?;
```

- **Fuel**: Before each guest export call, `Store::set_fuel` sets the instruction budget to `Limits::fuel_per_call`.
- **Linear memory**: `StoreLimitsBuilder::new().memory_size(limits.memory_bytes)` caps linear memory growth across the component instance.
- **Wall-clock deadline**: A background epoch ticker advances the engine epoch every 1 ms while at least one guest call is active and sleeps on a condition variable while idle. Each call sets `Store::set_epoch_deadline` from `Limits::call_deadline`.
- **Fault isolation**: A guest trap, fuel exhaustion, or epoch deadline interruption fails the call, calls `PluginContext::fail` to drop all registrations for the plugin, posts `PluginDeactivated`, and records the failure reason in `PluginContext::failures()`.
- **Audio deadline bypass**: Audio processor buffers are pre-allocated to `AUDIO_HOST_BLOCK` frames when the processor instance is created. If a processor misses three consecutive block deadlines, the insert is bypassed and a `ProcessorFault` is recorded in `PluginContext::faults()` while the rest of the plugin stays mounted.

### Limits in the editor

The editor mounts WASM packages with the fuel and memory set in Settings >
Plugins and stored in the `[plugin_limits]` table of `config.toml`:

```toml
[plugin_limits]
wasm_fuel_per_call = 500000000   # 1,000,000 to 100,000,000,000
wasm_memory_mib = 64             # 16 to 4,096
```

An absent table gives the defaults above, which are the `Limits::default()`
values. A value outside its range keeps the default and is listed among the
rejected keys. Changing a limit in Settings saves it and remounts every
mounted WASM package under the new limits; a package that does not mount
under them is shown as failed. The call deadline is not configurable and
stays at `DEFAULT_CALL_DEADLINE`.

## Hot Reloading

The editor reloads a WASM package in the plugin folder when its
`plugin.toml`, `plugin.wasm`, or a declared shader changes. A change waits
until the files have been unchanged for 300 ms, then the package unmounts and
mounts from disk. A component that no longer loads is shown as failed in the
Plugins list and mounts again once it is fixed. A disabled package is not
reloaded; enabling it mounts it as it is on disk.

`Reloader` does the same for a headless host. `Reloader::poll(&mut self, ctx: &PluginContext)` hashes `plugin.toml` and `plugin.wasm` in the watched directories, unmounts and remounts any package whose manifest or component changed, mounts newly added packages, and unmounts packages whose directory was removed.

## Building a Guest Plugin in Rust

1. Create a `cdylib` crate targeting `wasm32-unknown-unknown` with `wit-bindgen`:

```toml
[package]
name = "my-wasm-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde_json = "1.0"
wit-bindgen = "0.51"

[profile.release]
opt-level = "s"
lto = true
strip = true
codegen-units = 1
panic = "abort"
```

2. Bind the WIT world and implement `Guest`:

```rust
wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/aria-plugin-wasm/wit",
});

struct MyPlugin;

impl Guest for MyPlugin {
    fn mount() -> Result<(), String> {
        register_command("my.cmd", "Run My Command")?;
        Ok(())
    }
    // ...
}

export!(MyPlugin);
```

3. Compile the core WebAssembly module and encode it as a Component Model binary using `examples/wasm-plugin/rebuild.sh`:

```bash
./examples/wasm-plugin/rebuild.sh
```
