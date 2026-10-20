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
`aria_plugin_wasm::WORLD` holds its name. The [WIT reference](wit-reference.md)
describes every function, type, field and case of the world. The tables below
list the permission each host import requires and the chapter that covers each
function.

Loading a component type-checks it against the world before anything runs. A
component that imports a function the host does not provide, or lacks an
export the world requires, fails to load with `does not match world
aria:plugin/plugin@1.0.0` and the mismatch. `aria plugin check` runs the same
load.

### Host Imports

| Import | Permission | Chapter |
| --- | --- | --- |
| [`log`](wit-reference.md#import-log) | None | [Logging](basics.md#logging) |
| [`storage-get`](wit-reference.md#import-storage-get) | None | |
| [`storage-set`](wit-reference.md#import-storage-set) | None | |
| [`storage-delete`](wit-reference.md#import-storage-delete) | None | |
| [`storage-keys`](wit-reference.md#import-storage-keys) | None | |
| [`setting-get`](wit-reference.md#import-setting-get) | None | [Settings](settings.md) |
| [`register-command`](wit-reference.md#import-register-command) | `mount` only | [Commands](commands.md) |
| [`subscribe`](wit-reference.md#import-subscribe) | `mount` only | [Events](events.md#external-plugins) |
| [`register-importer`](wit-reference.md#import-register-importer) | `mount` only | [Importers and exporters](interchange.md#importers-and-exporters) |
| [`register-exporter`](wit-reference.md#import-register-exporter) | `mount` only | [Importers and exporters](interchange.md#importers-and-exporters) |
| [`register-encoder`](wit-reference.md#import-register-encoder) | `mount` only | [Encoders](interchange.md#encoders) |
| [`register-audio-effect`](wit-reference.md#import-register-audio-effect) | `mount` only | [Audio effects](audio-effects.md) |
| [`register-panel`](wit-reference.md#import-register-panel) | `mount` only | [Panels](panels.md) |
| [`refresh-panel`](wit-reference.md#import-refresh-panel) | Own panels only | [Panels](panels.md) |
| [`register-viewer-overlay`](wit-reference.md#import-register-viewer-overlay) | `mount` only | [Viewer overlays](viewer-overlays.md) |
| [`refresh-viewer-overlay`](wit-reference.md#import-refresh-viewer-overlay) | Own overlays only | [Viewer overlays](viewer-overlays.md) |
| [`project-snapshot`](wit-reference.md#import-project-snapshot) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`editor-view`](wit-reference.md#import-editor-view) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`clip`](wit-reference.md#import-clip) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`clips-in-range`](wit-reference.md#import-clips-in-range) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`clips-at`](wit-reference.md#import-clips-at) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`selected-clips`](wit-reference.md#import-selected-clips) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`markers-in-range`](wit-reference.md#import-markers-in-range) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`in-out`](wit-reference.md#import-in-out) | `project = "read"` or `"read-write"` | [Project](project.md#project) |
| [`project-transact`](wit-reference.md#import-project-transact) | `project = "read-write"` | [Project](project.md#project) |
| [`project-undo`](wit-reference.md#import-project-undo) | `project = "read-write"` | [Project](project.md#project) |
| [`fs-read`](wit-reference.md#import-fs-read) | Active import or decode path, or `permissions.fs` | [Permissions](packages.md#permissions) |
| [`fs-write`](wit-reference.md#import-fs-write) | Active export path or `permissions.fs` | [Permissions](packages.md#permissions) |
| [`start-job`](wit-reference.md#import-start-job) | None | [Jobs](jobs.md) |
| [`register-decoder`](wit-reference.md#import-register-decoder) | `mount` only | [Decoders](interchange.md#decoders) |
| [`fs-read-at`](wit-reference.md#import-fs-read-at) | Active import or decode path, or `permissions.fs` | [Permissions](packages.md#permissions) |
| [`fs-len`](wit-reference.md#import-fs-len) | Active import or decode path, or `permissions.fs` | [Permissions](packages.md#permissions) |
| [`register-ai-tool`](wit-reference.md#import-register-ai-tool) | `mount` only | [AI tools](ai-tools.md) |
| [`register-scope`](wit-reference.md#import-register-scope) | `mount` only | [Scopes](scopes.md) |
| [`set-effect-category`](wit-reference.md#import-set-effect-category) | `mount` only, own effects only | [Effects](effects.md) |
| [`register-effect`](wit-reference.md#import-register-effect) | `mount` only | [Effects](effects.md) |
| [`register-cpu-effect`](wit-reference.md#import-register-cpu-effect) | `mount` only | [CPU effects and generators](effects.md#cpu-effects-and-generators) |
| [`register-transition`](wit-reference.md#import-register-transition) | `mount` only | [Effects](effects.md) |
| [`register-generator`](wit-reference.md#import-register-generator) | `mount` only | [Generators](effects.md#generators) |
| [`register-effect-graph`](wit-reference.md#import-register-effect-graph) | `mount` only | [Effect graphs](effects.md#effect-graphs) |
| [`register-texture`](wit-reference.md#import-register-texture) | `mount` only | [Plugin textures](effects.md#plugin-textures) |
| [`register-clip-type`](wit-reference.md#import-register-clip-type) | `mount` only | [Clip types](clip-types.md) |
| [`define-clip-type`](wit-reference.md#import-define-clip-type) | `mount` only | [Defined types](clip-types.md#defined-types) |
| [`register-driver-language`](wit-reference.md#import-register-driver-language) | `mount` only | [Drivers](drivers.md) |
| [`register-preset`](wit-reference.md#import-register-preset) | `mount` only | [Presets](presets.md) |
| [`register-theme`](wit-reference.md#import-register-theme) | `mount` only | [Theme files](../theme-tokens.md#theme-files) |
| [`register-workspace`](wit-reference.md#import-register-workspace) | `mount` only | [Workspaces](workspaces.md) |
| [`register-analyzer`](wit-reference.md#import-register-analyzer) | `mount` only | [Audio analyzers](analyzers.md) |

### Guest Exports

| Export | Chapter |
| --- | --- |
| [`mount`](wit-reference.md#export-mount) | [Mount order](basics.md#mount-order) |
| [`unmount`](wit-reference.md#export-unmount) | [Mount order](basics.md#mount-order) |
| [`command-run`](wit-reference.md#export-command-run) | [Commands](commands.md) |
| [`event`](wit-reference.md#export-event) | [Events](events.md#external-plugins) |
| [`%import`](wit-reference.md#export-import) | [Importers and exporters](interchange.md#importers-and-exporters) |
| [`%export`](wit-reference.md#export-export) | [Importers and exporters](interchange.md#importers-and-exporters) |
| [`audio-create`](wit-reference.md#export-audio-create) | [Audio effects](audio-effects.md) |
| [`audio-process`](wit-reference.md#export-audio-process) | [Audio effects](audio-effects.md) |
| [`audio-reset`](wit-reference.md#export-audio-reset) | [Audio effects](audio-effects.md) |
| [`audio-drop`](wit-reference.md#export-audio-drop) | [Audio effects](audio-effects.md) |
| [`panel-view`](wit-reference.md#export-panel-view) | [Declarative UI panels](#declarative-ui-panels) |
| [`panel-event`](wit-reference.md#export-panel-event) | [Declarative UI panels](#declarative-ui-panels) |
| [`overlay-scene`](wit-reference.md#export-overlay-scene) | [Viewer overlays](viewer-overlays.md) |
| [`overlay-event`](wit-reference.md#export-overlay-event) | [Viewer overlays](viewer-overlays.md) |
| [`encoder-open`](wit-reference.md#export-encoder-open) | [Encoders](interchange.md#encoders) |
| [`encoder-push-frame`](wit-reference.md#export-encoder-push-frame) | [Encoders](interchange.md#encoders) |
| [`encoder-push-audio`](wit-reference.md#export-encoder-push-audio) | [Encoders](interchange.md#encoders) |
| [`encoder-finish`](wit-reference.md#export-encoder-finish) | [Encoders](interchange.md#encoders) |
| [`encoder-drop`](wit-reference.md#export-encoder-drop) | [Encoders](interchange.md#encoders) |
| [`job-step`](wit-reference.md#export-job-step) | [Jobs](jobs.md) |
| [`job-end`](wit-reference.md#export-job-end) | [Jobs](jobs.md) |
| [`decoder-probe`](wit-reference.md#export-decoder-probe) | [Decoders](interchange.md#decoders) |
| [`decoder-open-video`](wit-reference.md#export-decoder-open-video) | [Decoders](interchange.md#decoders) |
| [`decoder-seek`](wit-reference.md#export-decoder-seek) | [Decoders](interchange.md#decoders) |
| [`decoder-next-frame`](wit-reference.md#export-decoder-next-frame) | [Decoders](interchange.md#decoders) |
| [`decoder-open-audio`](wit-reference.md#export-decoder-open-audio) | [Decoders](interchange.md#decoders) |
| [`decoder-read-audio`](wit-reference.md#export-decoder-read-audio) | [Decoders](interchange.md#decoders) |
| [`decoder-seek-audio`](wit-reference.md#export-decoder-seek-audio) | [Decoders](interchange.md#decoders) |
| [`decoder-close`](wit-reference.md#export-decoder-close) | [Decoders](interchange.md#decoders) |
| [`ai-tool-call`](wit-reference.md#export-ai-tool-call) | [AI tools](ai-tools.md) |
| [`scope-measure`](wit-reference.md#export-scope-measure) | [Scopes](scopes.md) |
| [`cpu-effect-process`](wit-reference.md#export-cpu-effect-process) | [CPU effects and generators](effects.md#cpu-effects-and-generators) |
| [`driver-evaluate`](wit-reference.md#export-driver-evaluate) | [Drivers](drivers.md) |
| [`analyzer-open`](wit-reference.md#export-analyzer-open) | [Audio analyzers](analyzers.md) |
| [`analyzer-push`](wit-reference.md#export-analyzer-push) | [Audio analyzers](analyzers.md) |
| [`analyzer-finish`](wit-reference.md#export-analyzer-finish) | [Audio analyzers](analyzers.md) |
| [`analyzer-drop`](wit-reference.md#export-analyzer-drop) | [Audio analyzers](analyzers.md) |

`subscribe` takes an event kind, and `event` receives the kind and its payload as JSON. The kinds, payload fields, and replies are listed in [Events](events.md#external-plugins); a notification returns `ok(none)`.

### Declarative UI Panels

Because WIT forbids recursive types, `panel-view` returns a flattened `list<ui-node>` rooted at index `0`. Container variants (`column(list<u32>)` and `row(list<u32>)`) store child indices into that list, and `image(ui-image)` stores raw RGBA8 pixel bytes as `list<u8>`. Before constructing `aria_plugin::UiNode`, the host validates the flat list and rejects an empty list, an out-of-range child index, a cycle, a shared child index, or an unreachable non-root node with an error naming the offending node index, then runs `UiNode::check`.

### Viewer Overlays

A guest registers a viewer overlay during `mount` with `register-viewer-overlay(overlay-spec { id, title })` and returns its shapes from `overlay-scene(id) -> list<overlay-shape>`. Each `overlay-shape` variant (`line(overlay-line)`, `rect(overlay-rect)`, `ellipse(overlay-ellipse)`, `path(overlay-path)`, `text(overlay-text)`, `handle(overlay-handle)`) uses `point { x: f32, y: f32 }` coordinates in sequence frame pixels, `overlay-color` (`accent`, `text`, `muted`, `success`, `warning`, `danger`, or `rgba(tuple<u8, u8, u8, u8>)`), and `handle-look` (`dot`, `square`, `cross`). Pointer drags on a handle arrive through `overlay-event(id, overlay-event::drag(overlay-drag { handle, phase, at }))` with `drag-phase` (`start`, `move`, `end`), and out-of-band state changes schedule a scene rebuild with `refresh-viewer-overlay(id)`. The host converts the returned shape list into `aria_plugin::OverlayScene` and runs `OverlayScene::check`.

### Media Encoders

A WebAssembly plugin contributes an output format to the media Export dialog by calling
[`register-encoder`](wit-reference.md#import-register-encoder) during `mount` and implementing
[`encoder-open`](wit-reference.md#export-encoder-open),
[`encoder-push-frame`](wit-reference.md#export-encoder-push-frame),
[`encoder-push-audio`](wit-reference.md#export-encoder-push-audio),
[`encoder-finish`](wit-reference.md#export-encoder-finish), and
[`encoder-drop`](wit-reference.md#export-encoder-drop). The host calls `encoder-drop` when an
export is cancelled or fails before `encoder-finish`. [Encoders](interchange.md#encoders)
describes the export flow.

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
