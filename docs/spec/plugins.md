# Plugins

Every capability that a user sees, from the H.264 decoder to the Gaussian Blur
effect to the assistant, is a plugin on one public API. Shipped plugins use the
same API, the same manifest, and the same permissions as third-party plugins;
the only difference is that they are linked into the binary.

## Tiers

| Tier | Built as | Runs | Isolation | Hot reload |
| --- | --- | --- | --- | --- |
| Native | Rust crate on `aria-sdk`, compiled to a shared library | In process, on host threads | Permissions checked at each API call; faults caught at the boundary | Development mode only |
| WASM | WebAssembly component targeting the `aria:plugin` WIT world | In process, in a Wasmtime instance per plugin | Memory isolation, permission-checked imports, fuel and time limits | Always |

Both tiers see the same API. The native SDK is a safe Rust layer over a stable C
ABI: versioned function tables with a size field, plain C structs, and owned
strings, so a native plugin built by any `rustc` (or any C-compatible compiler)
loads into any Aria with the same API major version. WASM plugins can be
written in any language with component model support.

A WASM effect shader runs on the GPU like a native one: the shader is data, and
only parameter evaluation runs in the instance.

## Package

A plugin is a directory, or an `.ariaplugin` archive (zip) of one:

```
color-tools/
  plugin.toml
  plugin.wasm            # WASM tier, or
  lib/linux-x86_64.so    # native tier, one per platform
  lib/macos-aarch64.dylib
  lib/windows-x86_64.dll
  shaders/*.wgsl
  presets/*.ariapreset
  icons/*.svg
  locales/*.ftl
```

### Manifest

```toml
[plugin]
id = "com.example.color-tools"     # reverse domain, unique
name = "Color Tools"
version = "1.4.0"
api = "1"                          # API major version
publisher = "Example"
description = "Split-tone, LUT stack, and a hue curve scope."
license = "MIT"
requires = ["aria.color >=1"]      # other plugins, with version ranges

[activation]
on = ["effect:com.example.color-tools.split-tone", "command:color-tools.reset-stack", "panel:color-tools.lut-stack", "file:cube"]

[permissions]
project = "read-write"             # none | read | read-write
media = "read"
gpu = true
net = ["api.example.com"]          # hosts; omitted means no network
fs = ["project", "pick"]           # project dir, user-picked paths, plugin data is always allowed
process = false
clipboard = false

[[effects]]
id = "split-tone"
name = "Split Tone"
category = "Color"
shader = "shaders/split_tone.wgsl"

[[commands]]
id = "color-tools.reset-stack"
title = "Reset LUT Stack"
icon = "icons/reset.svg"
enablement = "selection && !playing"
key = "Ctrl+Alt+L"
menu = "Sequence/Color Tools"

[[menus]]
command = "color-tools.reset-stack"
menu = "Clip"

[[keys]]
command = "color-tools.reset-stack"
key = "Ctrl+Shift+L"
route = "home"

[[panels]]
id = "color-tools.lut-stack"
title = "LUT Stack"
location = "inspector"

[[settings]]
key = "lut-directory"
label = "LUT folder"
placeholder = "~/luts"

[[settings]]
key = "strength"
label = "Strength"
type = "number"
min = 0.0
max = 1.0
step = 0.05
default = 0.5
```

Every contribution can be declared in the manifest, so Aria lists effects,
commands, menus, keys, settings, and panels without loading the plugin. Code
registers handlers for them when the plugin activates.

A command's `menu` and `key` are one `[[menus]]` entry and one `[[keys]]`
entry. Both tables name a command declared in `[[commands]]`.

- `menu` is a menu bar menu (`File`, `Edit`, `Sequence`, `Markers`, `Effect`,
  `Animation`, `View`, `Window`, `Help`) or a context menu (`Clip`, `Track`,
  `Media`, `Viewer`, `Transition`, `Effect Header`, `Clip Badge`), optionally
  followed by a group: `Sequence/Color Tools` lists the row as
  `Color Tools: Reset LUT Stack`. A row is drawn below the menu's own rows
  while its command is registered.
- `icon` is an SVG file in the package, at most 64 KiB, drawn tinted in the
  palette and menus. `enablement` is a condition over editor state, such as
  `selection && !playing`; while it does not hold, the palette dims the
  command and menus leave it out. The keys are listed under
  [Enablement](../plugins/commands.md#enablement).
- `key` is a chord such as `Ctrl+Alt+L` on a key Aria binds. `route` is
  `editor` (the default) or `home`. A chord another binding holds on the same
  route is refused and listed under Settings > Keys, where the command can be
  given another chord.
- `[[settings]]` fields are drawn in Settings in a section titled with the
  plugin's name. `type` is `text` (the default), `secret`, `toggle`, `number`
  (with `min`, `max`, and `step`), or `choice` (with `options`). `key` is
  stored under the plugin id: `lut-directory` is
  `com.example.color-tools.lut-directory`. Values are stored as text: `true`
  or `false`, a decimal number, or the chosen option. An unset key reads as
  `default`.
- `[[panels]]` lists the panels the plugin registers, each with an `id`, a
  `title`, and a `location` of `sidebar`, `inspector`, or `timeline`.
- `[activation] on` lists the events the plugin loads on:

  | Event | Fires when |
  | --- | --- |
  | `startup` | Aria starts |
  | `command:<id>` | the declared command runs from the palette, a menu, or a key |
  | `file:<ext>` | a file with that extension is dropped, imported, or read as a timeline |
  | `effect:<id>`, `transition:<id>`, `generator:<id>` | the open project uses it |
  | `panel:<id>` | the declared panel is opened |
  | `setting:<key>` | the declared setting is read or saved |

  `command:`, `panel:`, and `setting:` name an entry the manifest declares. An
  extension is matched without case.

## Lifecycle

```
installed -> (activation event) -> active -> (disable / uninstall / reload) -> inactive
                                     |
                                     +-> failed (fault, timeout, denied load)
```

1. **Discovery.** At start Aria reads the manifests in `<data dir>/plugins/`
   and the shipped set. Reading a manifest runs no plugin code.
2. **Activation.** A plugin loads on the first of its activation events: start
   up, a command, a file type opened or dropped, an effect, transition, or
   generator used in an open project, a panel opened, or a setting read or
   saved. Until then its declared contributions are listed and its declared
   panels show an Open button. Plugins with no activation section, plugins
   with no code, and plugins a plugin that loads at start requires load at
   start. A waiting plugin's requirements that also wait load before it.
3. **Mount.** The plugin registers handlers for its contributions and services,
   and subscribes to events. Mount time is measured and shown in the Plugins
   panel.
4. **Unload.** Disable, uninstall, and reload drop every registration, wait for
   in-flight calls into the plugin to return, then unload its code. Nothing the
   host holds borrows from plugin memory, so unloading frees it.
5. **Reload.** A WASM plugin reloads when its package changes on disk. Native
   plugins reload in development mode (`aria plugin dev`), each load from a fresh
   copy of the library. Plugin state that must survive a reload is written to
   plugin storage.

Install: drop an `.ariaplugin` on the window or on the Plugins panel, or File >
Install Plugin. Aria shows the manifest's name, publisher, version, tier, and
permissions, and installs on confirmation. An update that adds permissions asks
again.

## Faults

- A panic in a native plugin is caught at the API boundary; a trap ends the WASM
  call. Either way the call fails, the plugin is marked failed for the session,
  its registrations are dropped, and the Plugins panel shows the message and
  backtrace. The rest of Aria keeps running.
- Calls on the render and audio paths have deadlines. An effect that misses the
  frame deadline is reported in the Plugins panel with its measured cost; an
  audio processor that misses three consecutive block deadlines is bypassed and
  reported.
- WASM calls run with fuel and memory limits from Settings > Plugins.
- An effect, transition, generator, or clip type in a project whose plugin is
  missing, failed, or disabled is kept: the clip renders it as passthrough (or a
  slate for a missing generator), its parameters and keyframes stay in the
  project unchanged, and the clip shows a missing-plugin badge naming the id.

## Permissions

| Permission | Grants |
| --- | --- |
| `project` | Reading the project; with `read-write`, proposing transactions |
| `media` | Probing and decoding media referenced by the project, thumbnails, waveforms |
| `gpu` | Creating GPU resources and submitting compute and render passes |
| `net` | HTTP and WebSocket connections to the listed hosts |
| `fs` | `project`: the project folder; `pick`: paths picked in a host dialog; `paths`: listed absolute paths |
| `process` | Spawning listed executables |
| `clipboard` | Reading the clipboard (writing needs no permission) |

Plugin storage (`<data dir>/plugin-data/<id>/`), settings, logging, events, UI
contributions, and the plugin's own package directory need no permission.

WASM plugins cannot reach anything not granted. A native plugin runs native
code in the process; its permissions are enforced at the API, and the install
dialog states that a native plugin can access the system beyond them.

## Contribution points

| Point | Contributes |
| --- | --- |
| Commands | Named actions with title, icon, enablement condition, and handler; listed in the command palette |
| Menus | Items in the menu bar and in the clip, track, media, viewer, transition, effect header, and clip badge context menus, each bound to a command ([menus.md](../plugins/menus.md)) |
| Keys | Default chords for commands, per context ([keyboard.md](keyboard.md#customizing)) |
| Settings | A typed settings schema; Aria draws the settings page and stores the values |
| Panels | Dockable panels: a declarative UI tree, a GPU canvas, or both |
| Viewer overlays | On-canvas controls and drawing over the Viewer frame ([viewer.md](viewer.md#on-canvas-controls)) |
| Effects | Video effects ([Effects](#effects)) |
| Transitions | Two-input effects with a progress parameter |
| Generators | Clips with no source media: solids, shapes, text, noise, titles |
| Clip types | New clip kinds with their own data, property rows, and render ([Clip types](#clip-types)) |
| Property drivers | Code that computes a property's value per frame ([Property drivers](#property-drivers)) |
| Audio processors | Audio effects and analyzers ([Audio processors](#audio-processors)) |
| Decoders | Containers and codecs for import and playback |
| Importers | Files that become project content: EDL, FCPXML, OTIO, subtitles, LUTs, presets |
| Exporters and encoders | Output formats in the Export dialog, including timeline interchange formats |
| Scopes | Analysis views of the presented frame |
| Jobs | Background work with progress, cancel, and results |
| AI tools | Tools the assistant can call, with a JSON schema and a handler |
| Drop handlers | Claiming dropped files and URLs |
| Workspaces | Named panel arrangements |
| Themes | Token sets ([look.md](look.md)) |
| Presets | Animation and effect presets |
| Services | Typed interfaces that other plugins consume, declared in the manifest with a version |

Any number of plugins can contribute to each point. Where several handlers apply
to one input, they run in a defined order: drop handlers and decoders by
declared priority, then mount order; command and event handlers in mount order.

### Effects

An effect declares parameters and a render graph.

**Parameters.** Types: float (with range, soft range, unit, and display:
slider, angle dial, percentage), int, bool, choice, colour (linear RGBA), point
2D and 3D (shown as Viewer handles), size, angle, curve, gradient, text, font,
file, and layer reference (another clip in the sequence, read as an input).
Every numeric, colour, and point parameter is keyframeable and drivable. Groups,
visibility conditions, and tooltips are declared. Parameter count and size are
unbounded: values reach the shader through a storage buffer, not a fixed
uniform block.

**Render graph.** One or more passes, each a WGSL compute or fragment shader
with named inputs (the source frame, a previous pass, a layer reference, a
plugin texture) and an output size (same as input, scaled, or a fixed size).
The graph declares how the effect expands or shrinks the frame bounds, whether
it reads pixels outside the output region, and whether it depends on time other
than through its parameters. Aria compiles the graph once, caches pipelines,
and runs it on the compositor's device with the same textures as shipped
effects; there is no copy into or out of the plugin.

**CPU path.** An effect without a shader implements a CPU callback that
receives the input frame as a mapped RGBA16F buffer and writes the output. Aria
uploads the result. The CPU path is for effects that cannot be written as
shaders; it does not take part in GPU caching.

**Determinism.** Frame cache keys include the plugin id, version, and parameter
values. An effect that is not a pure function of those and its inputs declares
`stateful = true`, and its frames are not cached.

### Clip types

A clip type defines a data schema stored in the project, its property rows, its
Viewer handles, its icon and colour on the timeline, and a render callback or
graph. Text, shape, adjustment, and multicam clips are shipped clip types.

### Property drivers

A driver computes one property's value at a time `t`. It receives `t`, the
property's own keyframed value at `t`, the values of other properties it
declares as inputs (on any clip in the sequence), and the clip's local time and
frame rate. It returns a value of the property's type. Drivers are evaluated
during render on the render thread and must not block; a driver that errors
leaves the property at its keyframed value and reports the error in Info
([animation.md](animation.md#drivers)).

Aria resolves driver inputs as a dependency graph and rejects cycles at the time
a driver is attached. `aria.expressions` is a shipped driver plugin with a small
expression language (`wiggle`, `loopOut`, `linear`, `ease`, `valueAtTime`,
vector maths).

### Audio processors

An audio processor declares channel layout in and out, latency in samples,
parameters (same types and keyframing as effects), and optional sidechain
inputs. Its process callback receives one block of planar f32 samples and the
block's parameter values, sample-accurate when they change inside the block. It
runs on the real-time audio thread: no allocation, locking, or I/O. Analyzers
(beat, silence, loudness) are processors that write results through a job
instead of producing audio.

## Host API

Every call below is available to both tiers. Calls marked with a permission
need it.

| Namespace | Calls |
| --- | --- |
| `project` (`project`) | A read-only snapshot of the project: sequences, tracks, clips, effects, keyframes, markers, media, bins; queries by id, time range, and selection; the current selection, playhead, In/Out |
| `transact` (`project` read-write) | Applies a list of commands as one undo step with a name; each command passes `CommandProposed` and is validated as a whole; returns the ids it created |
| `events` | Subscribing to [events](#events) |
| `render` | The frame of a sequence or clip at a time, at a size, as a GPU texture or an RGBA image; render ranges |
| `media` (`media`) | Probe, decode a frame or audio range, thumbnails, waveform peaks |
| `gpu` (`gpu`) | The shared device and queue, texture and buffer creation, compute dispatch |
| `storage` | Read and write files in plugin storage; a per-project store saved in the project file |
| `settings` | The plugin's settings values and change events |
| `ui` | Notifications, the status message, modal dialogs from a declarative tree, file and folder pickers, opening a panel, revealing a clip or property, the command palette |
| `jobs` | Starting a job with progress and cancel |
| `net` (`net`) | HTTP requests and WebSockets to granted hosts |
| `fs` (`fs`) | File access in granted scopes |
| `process` (`process`) | Spawning granted executables with arguments, piped I/O |
| `services` | Getting another plugin's service by interface name and version |
| `log` | Log lines at levels, shown in the Plugins panel |

The project snapshot is immutable and cheap to take. Edits happen only through
`transact`, so every plugin edit is undoable, appears in History with the
plugin's name, and is visible to every `CommandProposed` handler.

### Events

| Event | Kind |
| --- | --- |
| `CommandProposed` | Waterfall: handlers may rewrite or refuse a command before it applies |
| `CommandApplied` | Notification with the command, its undo step, and the ids it changed |
| `ProjectOpened`, `ProjectClosed`, `ProjectSaved` | Notification |
| `SelectionChanged`, `PlayheadMoved` (at most once per frame), `InOutChanged` | Notification |
| `PlaybackStarted`, `PlaybackStopped` | Notification |
| `MediaImported`, `MediaRelinked`, `MediaOffline` | Notification |
| `RenderFinished`, `ExportStarted`, `ExportFinished` | Notification with result |
| `JobProgress`, `JobFinished` | Notification |
| `PluginActivated`, `PluginDeactivated` | Notification |
| `ResolveDropped` | Serial: handlers ordered by priority then mount order claim a dropped path or URL |
| `ContextMenuRequested` | Waterfall: handlers add items for the clicked target |

Handlers for notifications run off the UI thread unless they ask for it.

## Developer workflow

| Command | Result |
| --- | --- |
| `aria plugin new <id> --tier native\|wasm` | Creates a plugin directory with a manifest, a sample effect, and a sample command |
| `aria plugin dev <dir>` | Starts Aria with the plugin loaded from source and reloads it on every build |
| `aria plugin check <dir or package>` | Validates the manifest, shaders, permissions, and the ABI or WIT version without starting Aria |
| `aria plugin pack <dir>` | Writes the `.ariaplugin` package |

The SDK reference (`aria-sdk` for Rust, `aria:plugin` WIT for WASM) is generated
from the API definitions; `docs/plugins/README.md` is the guide.

## Shipped plugins

The shipped set is every capability Aria has: media decoders and encoders,
the effect and transition catalogs, text and shape clip types, colour, scopes,
audio effects and analysis, the assistant and its tools, `aria.expressions`,
importers and exporters for EDL, FCPXML, and OTIO, and the shipped themes and
workspaces. Each appears in the Plugins panel and can be disabled, except the
project model and the plugin host.
